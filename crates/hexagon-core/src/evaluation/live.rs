//! D03: owner price attestation plus actual metered model/tool receipts.
use super::{budget, config, err, EvaluationBatch};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::io;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReceipt {
    pub id: String,
    pub batch_id: String,
    pub batch_fingerprint: String,
    pub host: String,
    pub workspace: String,
    pub evidence_kind: String,
    pub state: String,
    pub request_ids: Vec<String>,
    pub observed_models: Vec<String>,
    pub tools_passed: bool,
    pub price_fingerprint: String,
    pub isolation_id: String,
    pub reason: Option<String>,
    pub nonce: String,
    pub exchanges: Vec<ProbeExchange>,
}

pub(crate) fn price(batch: &EvaluationBatch) -> io::Result<budget::live::LivePrice> {
    // Also check legacy frozen batches before preflight, generation or dispatch.
    config::require_safety_contracts(&batch.request)?;
    let model = batch
        .runtime
        .models
        .get(&batch.request.main_slot)
        .ok_or_else(|| err("preflight_model_missing"))?;
    let rate = batch
        .request
        .prices
        .get(&batch.request.main_slot)
        .ok_or_else(|| err("preflight_price_missing"))?;
    if !model.enabled
        || model.context_window.is_none()
        || model.max_output.is_none()
        || rate.provider_id != model.provider_id
        || rate.model != model.model
        || rate.billing_scope != "text_input_output_only"
    {
        return Err(err("preflight_model_or_price_unbounded"));
    }
    if !batch.verification.observations.iter().any(|o| {
        matches!(o.dimension, config::VerificationDimension::Price)
            && matches!(o.outcome, config::ObservedOutcome::ReportedPass)
            && o.batch_fingerprint == batch.fingerprint
            && o.source_url == rate.source_url
            && o.checked_at == rate.checked_at
    }) {
        return Err(err("preflight_owner_price_verification_required"));
    }
    Ok(budget::live::LivePrice {
        model: model.model.clone(),
        provider_id: model.provider_id.clone(),
        source_fingerprint: config::digest(rate)?,
        tariff: super::DebugPrice {
            prompt_per_1k_mc: rate.prompt_per_1k_mc,
            completion_per_1k_mc: rate.completion_per_1k_mc,
            prompt_bound: model.context_window.unwrap_or(0),
            output_bound: model.max_output.unwrap_or(0),
        },
    })
}

pub(crate) fn read(db: &Db, id: &str) -> io::Result<PreflightReceipt> {
    let (batch, json, fingerprint): (String, String, String) = db
        .conn()
        .query_row(
            "SELECT batch_id,receipt_json,fingerprint FROM evaluation_preflights WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(err)?;
    let receipt: PreflightReceipt = serde_json::from_str(&json)?;
    if receipt.id != id || receipt.batch_id != batch || config::digest(&receipt)? != fingerprint {
        return Err(err("preflight_source_corrupt"));
    }
    Ok(receipt)
}

pub(crate) fn proof(db: &Db, batch: &EvaluationBatch) -> io::Result<bool> {
    let Some(id) = &batch.verification.price_receipt else {
        return Ok(false);
    };
    let receipt = read(db, id)?;
    let price = price(batch)?;
    let root: String = db
        .conn()
        .query_row(
            "SELECT dir FROM projects WHERE id=?1",
            [crate::PROJECT_ID],
            |r| r.get(0),
        )
        .map_err(err)?;
    if receipt.host
        != std::path::Path::new(&root)
            .canonicalize()?
            .to_string_lossy()
    {
        return Ok(false);
    }

    let domain = receipt.evidence_kind == "live_model"
        || (cfg!(test) && receipt.evidence_kind == "provider_boundary_fixture");
    if !domain
        || receipt.state != "passed"
        || receipt.batch_id != batch.id
        || receipt.batch_fingerprint != batch.fingerprint
        || receipt.price_fingerprint != price.source_fingerprint
        || !receipt.tools_passed
        || !probe_valid(
            &receipt.nonce,
            &batch.request.main_slot,
            &price.model,
            &receipt.exchanges,
        )?
        || receipt.observed_models != vec![price.model.clone(), price.model]
        || receipt.request_ids.len() != 2
        || receipt.request_ids[0] == receipt.request_ids[1]
        || batch.verification.model_request_id.as_deref() != Some(&receipt.request_ids[0])
        || batch.verification.tools_request_id.as_deref() != Some(&receipt.request_ids[1])
    {
        return Ok(false);
    }
    let isolation = super::isolation::read_check(db, &receipt.isolation_id)?;
    if !isolation.passed || isolation.runtime_fingerprint != config::code_fingerprint()? {
        return Ok(false);
    }
    let paid = budget::paid_summary()?;
    let bound = paid.runs.iter().find(|r| {
        r.host == receipt.host
            && r.plan_id == receipt.id
            && r.run_id.as_deref() == Some(&receipt.id)
            && r.workspace.as_deref() == Some(&receipt.workspace)
    });
    let Some(bound) = bound else {
        return Ok(false);
    };
    if !bound.closed
        || bound.requests != 2
        || bound.confirmed_requests != 2
        || bound.unknown_mc != 0
        || bound.in_flight_mc != 0
    {
        return Ok(false);
    }
    let worker =
        Db::open_current(std::path::Path::new(&receipt.workspace).join(".hexagon/state.db"))
            .map_err(err)?;
    for request in &receipt.request_ids {
        let valid:bool=worker.conn().query_row("SELECT EXISTS(SELECT 1 FROM usage WHERE request_id=?1 AND purpose='evaluation_preflight' AND request_state='succeeded')",[request],|r|r.get(0)).map_err(err)?;
        if !valid {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn enable_plan(db: &Db, project: &str, plan_id: &str) -> io::Result<()> {
    let plan = super::plan::read(db, plan_id)?;
    let batch = config::read(db, &plan.batch_id)?;
    let checked = config::check(db, project, &batch.id, &batch.request)?;
    let preserved = plan.kind == super::PlanKind::Candidate
        && checked
            .blocks
            .iter()
            .all(|b| *b == super::AdmissionBlock::HeldoutRetired)
        && super::candidate::validate_start(
            db,
            std::path::Path::new(&host_root(db, project)?),
            plan_id,
        )?;
    if !checked.ready && !preserved {
        return Err(err("live_configuration_unverified"));
    }
    let price = price(&batch)?;
    use rusqlite::OptionalExtension;
    let existing: Option<(String, String)> = db
        .conn()
        .query_row(
            "SELECT scope,price_json FROM evaluation_budget_plans WHERE plan_id=?1",
            [plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    if let Some((scope, json)) = existing {
        if scope == "paid_first_round" && json == serde_json::to_string(&price)? {
            return Ok(());
        }
        return Err(err("plan_budget_already_frozen"));
    }
    if plan
        .entries
        .iter()
        .any(|r| r.state != super::PlannedState::Planned)
    {
        return Err(err("live_budget_must_precede_execution"));
    }
    db.conn().execute("INSERT INTO evaluation_budget_plans(plan_id,price_json,scope) VALUES (?1,?2,'paid_first_round')",rusqlite::params![plan_id,serde_json::to_string(&price)?]).map_err(err)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn corrupt_receipt_for_test(db: &Db, id: &str) {
    let mut receipt = read(db, id).unwrap();
    receipt.tools_passed = false;
    db.conn()
        .execute(
            "UPDATE evaluation_preflights SET receipt_json=?2 WHERE id=?1",
            rusqlite::params![id, serde_json::to_string(&receipt).unwrap()],
        )
        .unwrap();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeExchange {
    request_fingerprint: String,
    content: Vec<crate::provider::ContentBlock>,
    stop: String,
    observed_model: Option<String>,
}
impl ProbeExchange {
    pub(crate) fn capture(
        request: &crate::provider::ChatRequest,
        response: &crate::provider::ChatResponse,
    ) -> io::Result<Self> {
        Ok(Self {
            request_fingerprint: config::digest(&(
                &request.model_slot,
                &request.messages,
                &request.tools,
            ))?,
            content: response.content.clone(),
            stop: match response.stop {
                crate::provider::StopReason::ToolUse => "tool_use",
                crate::provider::StopReason::EndTurn => "end_turn",
                _ => "unverified",
            }
            .into(),
            observed_model: response.usage.observed_model.clone(),
        })
    }
}

pub(crate) fn probe_request(nonce: &str, slot: &str) -> crate::provider::ChatRequest {
    use crate::provider::{ChatRequest, ContentBlock, Message, Role, ToolDef};
    ChatRequest {
        model_slot:slot.into(),
        messages:vec![Message {role:Role::User,content:vec![ContentBlock::Text{text:format!("Call evaluation_probe exactly once with nonce {nonce}. After its result, return exactly the nonce as plain text.")}]}],
        tools:vec![ToolDef{name:"evaluation_probe".into(),description:"Host-owned harmless echo probe".into(),input_schema:serde_json::json!({"type":"object","properties":{"nonce":{"type":"string","enum":[nonce]}},"required":["nonce"],"additionalProperties":false})}],
    }
}

// D03/D12: a false rejection costs another manual inspection; a false pass
// authorizes paid work on an unproved tool protocol. Only the exact two actual
// exchanges qualify; cached pass booleans cannot replace response evidence.
fn probe_valid(
    nonce: &str,
    slot: &str,
    model: &str,
    exchanges: &[ProbeExchange],
) -> io::Result<bool> {
    use crate::provider::{ContentBlock, Message, Role};
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) || exchanges.len() != 2 {
        return Ok(false);
    }
    let first = &exchanges[0];
    let second = &exchanges[1];
    let mut request = probe_request(nonce, slot);
    if first.request_fingerprint
        != config::digest(&(&request.model_slot, &request.messages, &request.tools))?
        || first.stop != "tool_use"
        || second.stop != "end_turn"
        || exchanges
            .iter()
            .any(|e| e.observed_model.as_deref() != Some(model))
    {
        return Ok(false);
    }
    let calls: Vec<_> = first
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::ToolUse { id, name, input } = b {
                Some((id, name, input))
            } else {
                None
            }
        })
        .collect();
    if calls.len() != 1
        || calls[0].0.is_empty()
        || calls[0].1 != "evaluation_probe"
        || calls[0].2 != &serde_json::json!({"nonce":nonce})
        || first.content.iter().any(|b| {
            !matches!(
                b,
                ContentBlock::ToolUse { .. }
                    | ContentBlock::Text { .. }
                    | ContentBlock::Thinking { .. }
            )
        })
    {
        return Ok(false);
    }
    request.messages.push(Message {
        role: Role::Assistant,
        content: first.content.clone(),
    });
    request.messages.push(Message {
        role: Role::Tool,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: calls[0].0.clone(),
            content: nonce.into(),
            is_error: false,
            images: vec![],
        }],
    });
    request.tools.clear();
    let text = second
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    Ok(second.request_fingerprint
        == config::digest(&(&request.model_slot, &request.messages, &request.tools))?
        && text.trim() == nonce
        && second
            .content
            .iter()
            .all(|b| matches!(b, ContentBlock::Text { .. } | ContentBlock::Thinking { .. })))
}

pub(crate) fn save(db: &Db, receipt: &PreflightReceipt) -> io::Result<()> {
    let changed = db
        .conn()
        .execute(
            "UPDATE evaluation_preflights SET receipt_json=?2,fingerprint=?3 WHERE id=?1",
            rusqlite::params![
                receipt.id,
                serde_json::to_string(receipt)?,
                config::digest(receipt)?
            ],
        )
        .map_err(err)?;
    if changed != 1 {
        return Err(err("preflight_receipt_missing"));
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static ACTIVITY_CRASH:std::cell::RefCell<Option<String>>=const {std::cell::RefCell::new(None)}; }
#[cfg(test)]
pub(crate) fn arm_activity_crash(point: &str) {
    ACTIVITY_CRASH.with(|p| *p.borrow_mut() = Some(point.into()));
}
#[cfg(test)]
pub(crate) fn activity_crash(point: &str) {
    if std::env::var("HEXAGON_TEST_ACTIVITY_CRASH_POINT").as_deref() == Ok(point) {
        std::process::exit(86);
    }
    let crash = ACTIVITY_CRASH.with(|p| {
        if p.borrow().as_deref() == Some(point) {
            p.borrow_mut().take();
            true
        } else {
            false
        }
    });
    assert!(!crash, "simulated activity owner loss at {point}");
}

fn host_root(db: &Db, project: &str) -> io::Result<String> {
    db.conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
            r.get(0)
        })
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatResponse, ContentBlock, Message, Role, StopReason, Usage};
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn probe_requires_original_requests_nonce_model_and_closed_protocol(nonce in "[0-9a-f]{32}", mutation in 0u8..7) {
            let model="fixture-model";
            let mut request=probe_request(&nonce,"default");
            let first=ChatResponse{content:vec![ContentBlock::ToolUse{id:"call".into(),name:"evaluation_probe".into(),input:serde_json::json!({"nonce":nonce})}],stop:StopReason::ToolUse,usage:Usage{observed_model:Some(model.into()),..Default::default()}};
            let mut exchanges=vec![ProbeExchange::capture(&request,&first).unwrap()];
            request.messages.push(Message{role:Role::Assistant,content:first.content});
            request.messages.push(Message{role:Role::Tool,content:vec![ContentBlock::ToolResult{tool_use_id:"call".into(),content:nonce.clone(),is_error:false,images:vec![]}]});
            request.tools.clear();
            let second=ChatResponse{content:vec![ContentBlock::Text{text:nonce.clone()}],stop:StopReason::EndTurn,usage:Usage{observed_model:Some(model.into()),..Default::default()}};
            exchanges.push(ProbeExchange::capture(&request,&second).unwrap());
            prop_assert!(probe_valid(&nonce,"default",model,&exchanges).unwrap());
            match mutation {
                0=>exchanges[0].observed_model=None,
                1=>exchanges[1].request_fingerprint.push('x'),
                2=>exchanges[0].content.clear(),
                3=>exchanges[1].content=vec![ContentBlock::Text{text:format!("{nonce}x")}],
                4=>exchanges[1].stop="max_tokens".into(),
                5=>{exchanges.pop();},
                _=>exchanges[0].content.push(ContentBlock::ToolUse{id:"extra".into(),name:"evaluation_probe".into(),input:serde_json::json!({"nonce":nonce})}),
            }
            prop_assert!(!probe_valid(&nonce,"default",model,&exchanges).unwrap());
        }
    }
}
