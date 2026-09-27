//! Durable tool actions (reliability 08 / D05). An execution intent is not proof
//! of success or failure. False uncertainty costs a reconciliation; false success
//! or retry can duplicate an unreviewed side effect. Bias toward uncertainty.

use crate::db::Db;
use crate::tools::{CallOutcome, ToolContext, ToolError};
use crate::trace::EventKind;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

#[derive(Debug)]
pub(crate) struct Action {
    pub id: String,
    pub agent_id: String,
    pub stage_run_id: Option<String>,
    pub tool: String,
    pub input: Value,
    pub state: String,
    pub question_id: Option<String>,
    output: Option<String>,
    error: Option<String>,
}

pub(crate) fn get(db: &Db, project: &str, id: &str) -> Result<Action, ToolError> {
    db.conn()
        .query_row(
            "SELECT id,agent_id,stage_run_id,tool,input_json,state,question_id,output_json,error
         FROM tool_actions WHERE project_id=?1 AND id=?2",
            [project, id],
            |r| {
                let input: String = r.get(4)?;
                Ok(Action {
                    id: r.get(0)?,
                    agent_id: r.get(1)?,
                    stage_run_id: r.get(2)?,
                    tool: r.get(3)?,
                    input: serde_json::from_str(&input).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
                    state: r.get(5)?,
                    question_id: r.get(6)?,
                    output: r.get(7)?,
                    error: r.get(8)?,
                })
            },
        )
        .map_err(Into::into)
}

pub(crate) fn prepare(
    db: &Db,
    ctx: &ToolContext,
    tool: &str,
    input: &Value,
    seq: Option<&str>,
) -> Result<Action, ToolError> {
    let nonce;
    let seq = match seq {
        Some(s) => s,
        None => {
            nonce = format!("direct:{}", db.next_id("action_call")?);
            &nonce
        }
    };
    let identity = json!([ctx.project_id, ctx.agent_id, ctx.stage_run_id, seq]).to_string();
    let canonical = input.to_string();
    let id = format!("action{}", db.next_id("action")?);
    // New model dispatches use [request event id, provider tool-call id]. Legacy
    // explicit seq callers remain supported without fabricating a request id.
    let request: Option<(i64, String)> = serde_json::from_str(seq).ok();
    db.conn().execute("INSERT OR IGNORE INTO tool_actions
        (id,identity,project_id,agent_id,stage_run_id,request_id,tool_call_id,tool,input_json,input_digest,state)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending')",
        rusqlite::params![id,identity,ctx.project_id,ctx.agent_id,ctx.stage_run_id,request.as_ref().map(|r| r.0),
            request.as_ref().map(|r| r.1.as_str()).unwrap_or(seq),tool,canonical,format!("{:016x}",crate::tools::fnv64(&canonical))])?;
    let id: String = db.conn().query_row(
        "SELECT id FROM tool_actions WHERE identity=?1",
        [&identity],
        |r| r.get(0),
    )?;
    let action = get(db, &ctx.project_id, &id)?;
    // The digest is an index aid, not an authorization proof: compare canonical
    // input too, so a hash collision cannot substitute a different operation.
    if action.tool != tool || action.input != *input {
        return Err(ToolError::BadInput(
            "action identity reused with different parameters".into(),
        ));
    }
    Ok(action)
}

pub(crate) fn replay(action: &Action) -> Result<Option<CallOutcome>, ToolError> {
    match action.state.as_str() {
        "succeeded" => Ok(Some(CallOutcome::Done(
            serde_json::from_str(action.output.as_deref().unwrap_or("null"))
                .map_err(|_| ToolError::Exec("stored action result is invalid".into()))?,
        ))),
        "denied" => Ok(Some(CallOutcome::Denied(
            action
                .error
                .clone()
                .unwrap_or_else(|| "authorization denied".into()),
        ))),
        "failed" => Err(ToolError::Exec(
            action
                .error
                .clone()
                .unwrap_or_else(|| "action failed".into()),
        )),
        "unknown" => Err(ToolError::OutcomeUnknown(action.id.clone())),
        "executing" => Err(ToolError::ActionInProgress(action.id.clone())),
        "pending" if action.question_id.is_some() => Ok(Some(CallOutcome::Asked(
            action.question_id.clone().unwrap(),
        ))),
        _ => Ok(None),
    }
}

pub(crate) fn authorize(db: &Db, ctx: &ToolContext, id: &str) -> Result<(), ToolError> {
    db.conn().execute("UPDATE tool_actions SET state='authorized' WHERE project_id=?1 AND id=?2 AND state='pending'", [&ctx.project_id,id])?;
    Ok(())
}

pub(crate) fn set_question(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    qid: &str,
) -> Result<(), ToolError> {
    db.conn().execute(
        "UPDATE tool_actions SET question_id=?1 WHERE project_id=?2 AND id=?3 AND state='pending'",
        [qid, &ctx.project_id, id],
    )?;
    Ok(())
}

pub(crate) fn start(db: &Db, ctx: &ToolContext, id: &str) -> Result<(), ToolError> {
    let n = db.conn().execute("UPDATE tool_actions SET state='executing' WHERE project_id=?1 AND id=?2 AND state='authorized'", [&ctx.project_id,id])?;
    if n != 1 {
        return Err(ToolError::ActionInProgress(id.into()));
    }
    #[cfg(test)]
    checkpoint(CrashPoint::Intent);
    Ok(())
}

pub(crate) fn denied(db: &Db, ctx: &ToolContext, id: &str, reason: &str) -> Result<(), ToolError> {
    let n = db.conn().execute("UPDATE tool_actions SET state='denied',error=?1 WHERE project_id=?2 AND id=?3 AND state IN ('pending','authorized')", [reason,&ctx.project_id,id])?;
    if n > 0 {
        clear_ready_card(db, ctx, id)?;
    }
    Ok(())
}

pub(crate) fn finish(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    result: &Result<Value, ToolError>,
    uncertain: bool,
) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    #[cfg(test)]
    checkpoint(CrashPoint::Effect);
    let state = if uncertain {
        "unknown"
    } else if result.is_ok() {
        "succeeded"
    } else {
        "failed"
    };
    let output = result.as_ref().ok().map(Value::to_string);
    let error = result.as_ref().err().map(ToString::to_string);
    let tx = db.conn().unchecked_transaction()?;
    let n = db.conn().execute("UPDATE tool_actions SET state=?1,output_json=?2,error=?3 WHERE project_id=?4 AND id=?5 AND state='executing'",
        rusqlite::params![state,output,error,ctx.project_id,id])?;
    if n != 1 {
        return Err(ToolError::OutcomeUnknown(id.into()));
    }
    let action = get(db, &ctx.project_id, id)?;
    db.append_event(&ctx.project_id,EventKind::ToolResult,
        json!({"action_id":id,"tool":action.tool,"ok":if uncertain {None} else {Some(result.is_ok())},
            "state":state,"result":{"tool":action.tool,"output":result.as_ref().ok(),"error":error,
                "code":result.as_ref().err().map(crate::errcode::ErrorCode::code)}}),
        Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
    clear_ready_card(db, ctx, id)?;
    if uncertain {
        expose_unknown(db, ctx, id, started)?;
    }
    tx.commit()?;
    Ok(())
}

fn expose_unknown(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    started: std::time::Instant,
) -> Result<(), ToolError> {
    let key = format!("unknown:{id}");
    if let Some(card) = crate::cards::find_by_idem(db, &ctx.agent_id, &key)? {
        if card.state != crate::cards::CardState::Queued {
            crate::cards::requeue_action_recovery(db, &card.id)?;
        }
    } else {
        crate::cards::enqueue(
            db,
            &ctx.project_id,
            Some(&ctx.agent_id),
            crate::cards::CardKind::Recovery,
            json!({"sub":"tool_outcome_unknown","action_id":id,"stage_run_id":ctx.stage_run_id}),
            Some(&key),
        )?;
    }
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "tool_action",
        "outcome_unknown",
        started,
    );
    Ok(())
}

/// Called by Workbench startup or D10 explicit evaluation recovery holding the
/// exclusive driver/work leases. Read-only/control opens never recover live work.
pub(crate) fn recover(db: &Db, project: &str) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    migrate_legacy_cards(db, project)?;
    let ids: Vec<String> = {
        let mut st = db.conn().prepare(
            "SELECT id FROM tool_actions WHERE project_id=?1 AND state IN ('executing','unknown','authorized') AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=tool_actions.id)",
        )?;
        let rows = st
            .query_map([project], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        rows
    };
    for id in ids {
        let action = get(db, project, &id)?;
        let ctx = ToolContext {
            project_id: project.into(),
            agent_id: action.agent_id,
            stage_run_id: action.stage_run_id,
            ..Default::default()
        };
        let tx = db.conn().unchecked_transaction()?;
        let recovered_retry = action.state == "authorized" && is_retry(db, &id)?;
        // D05 / review11: an unsent retry does not prove the original action
        // was unexecuted. Recover it as unknown, never as a first-send ready card.
        if action.state == "executing" || recovered_retry {
            db.conn()
                .execute("UPDATE tool_actions SET state='unknown' WHERE id=?1", [&id])?;
            db.append_event(project,EventKind::ToolResult,json!({"action_id":id,"tool":action.tool,"state":"unknown","ok":null,"result":{"error":"execution interrupted; outcome requires reconciliation"}}),Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
        }
        if action.state == "authorized" && !recovered_retry {
            let key = format!("ready:{id}");
            if crate::cards::find_by_idem(db, &ctx.agent_id, &key)?.is_none() {
                crate::cards::enqueue(
                    db,
                    project,
                    Some(&ctx.agent_id),
                    crate::cards::CardKind::Recovery,
                    json!({"sub":"tool_action_ready","action_id":id,"stage_run_id":ctx.stage_run_id}),
                    Some(&key),
                )?;
            }
        } else {
            // Every interrupted intent supersedes a prior ready card, including
            // a first execution resumed from an earlier authorization crash.
            clear_ready_card(db, &ctx, &id)?;
            expose_unknown(db, &ctx, &id, started)?;
        }
        tx.commit()?;
    }
    Ok(())
}

fn migrate_legacy_cards(db: &Db, project: &str) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    let ids = crate::cards::legacy_permission_ids(db, project)?;
    for qid in ids {
        let card = crate::cards::get(db, &qid)?;
        let ctx = ToolContext {
            project_id: project.into(),
            agent_id: card.agent_id.unwrap_or_default(),
            ..Default::default()
        };
        let tx = db.conn().unchecked_transaction()?;
        let action = prepare(
            db,
            &ctx,
            card.payload["tool"].as_str().unwrap_or("legacy_unknown"),
            &card.payload["raw_input"],
            Some(&format!("legacy:{qid}")),
        )?;
        set_question(db, &ctx, &action.id, &qid)?;
        crate::cards::annotate(db, &qid, &[("action_id", json!(action.id))])?;
        if card.state != crate::cards::CardState::Queued {
            let denied: bool = db.conn().query_row("SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND kind='permission_denied' AND json_extract(payload,'$.question_id')=?2)",
                [project,&qid],|r|r.get(0))?;
            if denied {
                self::denied(db, &ctx, &action.id, "(owner): owner denied")?;
            } else {
                // reliability 08: an adjacent tool_result cannot identify a legacy
                // action. Keep history and require reconciliation, never backfill success.
                db.conn().execute(
                    "UPDATE tool_actions SET state='unknown' WHERE id=?1",
                    [&action.id],
                )?;
                expose_unknown(db, &ctx, &action.id, started)?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

fn clear_ready_card(db: &Db, ctx: &ToolContext, id: &str) -> Result<(), ToolError> {
    crate::cards::answer_queued_where(
        db,
        &ctx.project_id,
        crate::cards::CardKind::Recovery,
        "action_id",
        id,
        "action_result",
    )?;
    Ok(())
}

pub(crate) fn ensure_clear(db: &Db, ctx: &ToolContext) -> Result<(), ToolError> {
    ensure_clear_except(db, ctx, "")
}

pub(crate) fn ensure_clear_except(
    db: &Db,
    ctx: &ToolContext,
    allowed: &str,
) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    // reliability 08: another instance may work independently. Explicitly
    // resuming a proven-unstarted authorization must not deadlock against a
    // second unstarted authorization after a concurrent-child crash. Unknown
    // effects still block every resume; fresh actions also wait for ready ones.
    let resuming: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_actions WHERE project_id=?1 AND agent_id=?2 AND id=?3 AND state='authorized')",
        [&ctx.project_id, &ctx.agent_id, allowed], |r| r.get(0))?;
    let blocked: Option<(String,String)> = db.conn().query_row("SELECT id,state FROM tool_actions WHERE project_id=?1 AND (agent_id=?2 OR agent_id='') AND ((state='unknown' AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=tool_actions.id)) OR (state='authorized' AND NOT ?4) OR (state='pending' AND EXISTS(SELECT 1 FROM action_resolutions r WHERE r.next_action_id=tool_actions.id))) AND id<>?3 ORDER BY state='unknown' DESC,id LIMIT 1",
        rusqlite::params![ctx.project_id,ctx.agent_id,allowed,resuming],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((_, state)) = &blocked {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "tool_action_gate",
            if state == "authorized" {
                "action_ready_blocks_chain"
            } else {
                "unknown_blocks_chain"
            },
            started,
        );
    }
    match blocked {
        Some((id, state)) if state == "authorized" => Err(ToolError::ActionReady(id)),
        Some((id, _)) => Err(ToolError::OutcomeUnknown(id)),
        None => Ok(()),
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrashPoint {
    Authorization,
    Intent,
    Effect,
}
#[cfg(test)]
thread_local! { static CRASH: std::cell::Cell<Option<CrashPoint>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn crash_at(point: CrashPoint) {
    CRASH.set(Some(point));
}
#[cfg(test)]
pub(crate) fn checkpoint(point: CrashPoint) {
    if CRASH.get() == Some(point) {
        CRASH.set(None);
        panic!("synthetic action crash");
    }
}

/// D05: retain the earlier unknown event and append receipt evidence. Only a
/// trusted observation changes the current result; an owner's guess cannot.
pub(crate) fn record_reconciliation(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    observed: crate::tools::Reconciliation,
) -> Result<CallOutcome, ToolError> {
    use crate::tools::Reconciliation;
    let started = std::time::Instant::now();
    let (state, output, evidence) = match observed {
        Reconciliation::Unresolved { evidence } => ("unknown", None, evidence),
        Reconciliation::Succeeded { output, evidence } => ("succeeded", Some(output), evidence),
        Reconciliation::NotExecuted { evidence } => ("failed", None, evidence),
    };
    if evidence.trim().is_empty() {
        return Err(ToolError::OutcomeUnknown(id.into()));
    }
    let tx = db.conn().unchecked_transaction()?;
    let n = db.conn().execute("UPDATE tool_actions SET state=?1,output_json=?2 WHERE project_id=?3 AND id=?4 AND state='unknown' AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=?4)",
        rusqlite::params![state, output.as_ref().map(Value::to_string), ctx.project_id, id])?;
    if n == 0 {
        return replay(&get(db, &ctx.project_id, id)?)?
            .ok_or_else(|| ToolError::OutcomeUnknown(id.into()));
    }
    let action = get(db, &ctx.project_id, id)?;
    db.append_event(&ctx.project_id, EventKind::ToolResult,
        json!({"action_id":id,"tool":action.tool,"state":state,"ok":match state { "succeeded"=>Some(true), "failed"=>Some(false), _=>None },"reconciliation_evidence":evidence,"result":{"tool":action.tool,"output":output}}),
        Some(&ctx.agent_id), ctx.stage_run_id.as_deref())?;
    if state != "unknown" {
        clear_ready_card(db, ctx, id)?;
    } else if let Some(card) =
        crate::cards::find_by_idem(db, &ctx.agent_id, &format!("unknown:{id}"))?
    {
        crate::cards::annotate(
            db,
            &card.id,
            &[("reconciliation_evidence", json!(evidence))],
        )?;
    }
    tx.commit()?;
    crate::diag::note(
        if state == "unknown" {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        state == "unknown",
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "action_reconciliation",
        state,
        started,
    );
    match output {
        Some(output) => Ok(CallOutcome::Done(output)),
        None if state == "failed" => Err(ToolError::NotExecuted(
            "receipt proves action not executed".into(),
        )),
        None => Err(ToolError::OutcomeUnknown(id.into())),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(i64::MAX)
}

pub(crate) fn capture_retry_contract(
    db: &Db,
    id: &str,
    contract: Option<crate::tools::IdempotencyContract>,
) -> Result<(), ToolError> {
    if let Some(contract) = contract.filter(|c| !c.version.is_empty() && !c.validity.is_zero()) {
        let expires =
            now_ms().saturating_add(contract.validity.as_millis().min(i64::MAX as u128) as i64);
        // INSERT OR IGNORE is deliberate: retries/restarts cannot extend a
        // provider's guarantee beyond the first attempt (D05 / ticket 11).
        db.conn().execute("INSERT OR IGNORE INTO action_retry_contracts(action_id,contract_version,stable_key,expires_at_ms) VALUES (?1,?2,?1,?3)", rusqlite::params![id,contract.version,expires])?;
    }
    Ok(())
}

pub(crate) fn authorize_idempotent_retry(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    contract: Option<crate::tools::IdempotencyContract>,
) -> Result<bool, ToolError> {
    let started = std::time::Instant::now();
    let Some(contract) = contract.filter(|c| !c.version.is_empty() && !c.validity.is_zero()) else {
        return Ok(false);
    };
    // Fail closed: a false negative needs an owner; a false positive duplicates
    // a side effect. Only the persisted, still-live exact contract is accepted.
    let tx = db.conn().unchecked_transaction()?;
    let changed = db.conn().execute("UPDATE tool_actions SET state='authorized' WHERE id=?1 AND project_id=?2 AND state='unknown' AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=?1) AND EXISTS(SELECT 1 FROM action_retry_contracts c WHERE c.action_id=?1 AND c.stable_key=?1 AND c.contract_version=?3 AND c.expires_at_ms>?4)", rusqlite::params![id,ctx.project_id,contract.version,now_ms()])?;
    if changed == 1 {
        db.conn().execute(
            "UPDATE action_retry_contracts SET retry_authorized=1 WHERE action_id=?1",
            [id],
        )?;
    }
    tx.commit()?;
    crate::diag::note(
        if changed == 0 {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        changed == 0,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "idempotent_retry",
        if changed == 1 {
            "live_contract"
        } else {
            "no_live_contract"
        },
        started,
    );
    Ok(changed == 1)
}

pub(crate) fn abandon(db: &Db, project: &str, id: &str, reason: &str) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    if reason.trim().is_empty() {
        return Err(ToolError::BadInput(
            "owner resolution requires a reason".into(),
        ));
    }
    let tx = db.conn().unchecked_transaction()?;
    let action = get(db, project, id)?;
    if action.state != "unknown" {
        return Err(ToolError::BadInput(
            "only an unknown action can be abandoned".into(),
        ));
    }
    let existing: Option<String> = db
        .conn()
        .query_row(
            "SELECT decision FROM action_resolutions WHERE action_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.as_deref().is_some_and(|d| d != "abandoned") {
        return Err(ToolError::BadInput(
            "action already has a linked attempt".into(),
        ));
    }
    let changed = db.conn().execute("INSERT OR IGNORE INTO action_resolutions(action_id,decision,reason,actor) VALUES (?1,'abandoned',?2,'owner')", [id,reason])?;
    let ctx = ToolContext {
        project_id: project.into(),
        agent_id: action.agent_id,
        stage_run_id: action.stage_run_id,
        ..Default::default()
    };
    if changed > 0 {
        db.append_event(project, EventKind::ToolResult, json!({"action_id":id,"tool":action.tool,"state":"unknown","ok":null,"resolution":"abandoned","actor":"owner","reason":reason}), Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
        clear_ready_card(db, &ctx, id)?;
    }
    tx.commit()?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "action_resolution",
        "owner_abandoned",
        started,
    );
    Ok(())
}

pub(crate) fn has_resolution(db: &Db, id: &str) -> Result<bool, ToolError> {
    Ok(db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM action_resolutions WHERE action_id=?1)",
        [id],
        |r| r.get(0),
    )?)
}

pub(crate) fn new_owner_attempt(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    reason: &str,
    accepts_duplicate: bool,
) -> Result<Action, ToolError> {
    let started = std::time::Instant::now();
    if !accepts_duplicate || reason.trim().is_empty() {
        return Err(ToolError::BadInput(
            "new attempt requires owner reason and explicit duplicate-risk acceptance".into(),
        ));
    }
    let tx = db.conn().unchecked_transaction()?;
    let original = get(db, &ctx.project_id, id)?;
    if original.state != "unknown" || original.agent_id != ctx.agent_id {
        return Err(ToolError::BadInput(
            "original action is not unknown for this instance".into(),
        ));
    }
    let existing: Option<(String, Option<String>)> = db
        .conn()
        .query_row(
            "SELECT decision,next_action_id FROM action_resolutions WHERE action_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((decision, next)) = existing {
        if decision == "new_attempt" {
            return get(
                db,
                &ctx.project_id,
                &next.ok_or_else(|| ToolError::BadInput("missing linked attempt".into()))?,
            );
        }
        return Err(ToolError::BadInput("action already abandoned".into()));
    }
    // Reliability 11: reject other unresolved actions before linking a pending
    // attempt; linking first can make two recovery attempts block each other.
    ensure_clear_except(db, ctx, id)?;
    let next = prepare(
        db,
        ctx,
        &original.tool,
        &original.input,
        Some(&format!("owner-new-attempt:{id}")),
    )?;
    db.conn().execute("INSERT INTO action_resolutions(action_id,decision,reason,actor,next_action_id) VALUES (?1,'new_attempt',?2,'owner',?3)", [id,reason,&next.id])?;
    db.append_event(&ctx.project_id,EventKind::ToolResult,json!({"action_id":id,"tool":original.tool,"state":"unknown","ok":null,"resolution":"new_attempt","actor":"owner","reason":reason,"next_action_id":next.id}),Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
    // Keep the original recovery card until dispatch succeeds. A crash between
    // linking and sending leaves a durable pending attempt and an owner entry.
    tx.commit()?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "action_resolution",
        "owner_new_attempt",
        started,
    );
    Ok(next)
}

pub(crate) fn close_resolved_card(db: &Db, ctx: &ToolContext, id: &str) -> Result<(), ToolError> {
    clear_ready_card(db, ctx, id)
}

pub(crate) fn fail_unstarted_attempt(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    error: &ToolError,
) -> Result<(), ToolError> {
    // Reliability 11: a rejected preflight used to strand the linked pending
    // attempt forever. Only definite input/precondition rejection is terminal;
    // persistence and sequencing errors retain the recovery entry. The CAS
    // must not overwrite a concurrent permission card or dispatched attempt.
    if !matches!(
        error,
        ToolError::BadInput(_) | ToolError::PathEscape(_) | ToolError::NotExecuted(_)
    ) {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let tx = db.conn().unchecked_transaction()?;
    let changed = db.conn().execute(
        "UPDATE tool_actions SET state='failed',error=?3 WHERE project_id=?1 AND id=?2 AND state='pending' AND question_id IS NULL",
        rusqlite::params![ctx.project_id, id, error.to_string()],
    )?;
    if changed == 1 {
        let action = get(db, &ctx.project_id, id)?;
        db.append_event(&ctx.project_id, EventKind::ToolResult,
            json!({"action_id":id,"tool":action.tool,"state":"failed","ok":false,"error":error.to_string(),"reason":"preflight_rejected"}),
            Some(&ctx.agent_id), ctx.stage_run_id.as_deref())?;
    }
    tx.commit()?;
    if changed == 1 {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "owner_new_attempt",
            "preflight_rejected",
            started,
        );
    }
    Ok(())
}

pub(crate) fn is_retry(db: &Db, id: &str) -> Result<bool, ToolError> {
    Ok(db.conn().query_row("SELECT EXISTS(SELECT 1 FROM action_retry_contracts WHERE action_id=?1 AND retry_authorized=1)",[id],|r|r.get(0))?)
}

/// Every actual resend rechecks the original guarantee, including after an
/// authorized-retry crash. This transition is the dispatch linearization point.
pub(crate) fn start_retry(
    db: &Db,
    ctx: &ToolContext,
    id: &str,
    contract: Option<crate::tools::IdempotencyContract>,
    permitted: bool,
) -> Result<bool, ToolError> {
    let started = std::time::Instant::now();
    let tx = db.conn().unchecked_transaction()?;
    let version = contract
        .filter(|c| !c.version.is_empty() && !c.validity.is_zero())
        .map(|c| c.version);
    let changed = db.conn().execute("UPDATE tool_actions SET state='executing' WHERE id=?1 AND project_id=?2 AND state='authorized' AND ?5 AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=?1) AND EXISTS(SELECT 1 FROM action_retry_contracts c WHERE c.action_id=?1 AND c.retry_authorized=1 AND c.stable_key=?1 AND c.contract_version=?3 AND c.expires_at_ms>?4)",rusqlite::params![id,ctx.project_id,version,now_ms(),permitted])?;
    if changed == 0 {
        db.conn().execute("UPDATE tool_actions SET state='unknown' WHERE id=?1 AND project_id=?2 AND state='authorized'",[id,&ctx.project_id])?;
        if !has_resolution(db, id)? {
            clear_ready_card(db, ctx, id)?;
            expose_unknown(db, ctx, id, started)?;
        }
    }
    tx.commit()?;
    crate::diag::note(
        if changed == 0 {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        changed == 0,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "retry_dispatch",
        if changed == 1 {
            "live_authorized_contract"
        } else {
            "retry_no_longer_authorized"
        },
        started,
    );
    Ok(changed == 1)
}

/// D08 / reliability-19: delivery acceptance consumes stage effects. An owner
/// check/review exception cannot resolve an unknown tool outcome; the separate
/// recovery decision remains authoritative. Unrelated stage work can continue.
pub(crate) fn unresolved_delivery_actions(
    db: &Db,
    project: &str,
    run: Option<&str>,
) -> rusqlite::Result<Vec<String>> {
    let mut query = db.conn().prepare("SELECT id FROM tool_actions WHERE project_id=?1
        AND (?2 IS NULL OR stage_run_id=?2 OR stage_run_id IS NULL)
        AND (state='executing' OR (state='unknown' AND NOT EXISTS(SELECT 1 FROM action_resolutions r WHERE r.action_id=tool_actions.id))) ORDER BY id")?;
    let rows = query
        .query_map(rusqlite::params![project, run], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
