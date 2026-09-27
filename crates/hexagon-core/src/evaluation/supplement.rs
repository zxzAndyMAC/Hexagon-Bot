//! D11: at most one whole-pair supplement; originals remain immutable.
use super::{config, err, plan, EvaluationPlan};
use crate::db::Db;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplementOrigin {
    pub plan_id: String,
    pub position: usize,
    pub run_id: String,
    pub request_id: String,
}
fn reject(code: &str) -> io::Error {
    err(code)
}
// False rejection costs manual investigation; false admission enables choosing
// a better result after seeing failure. Only the frozen external status set wins.
fn eligible(status: u16, frozen: &[u16], state: &str) -> bool {
    [502, 503, 504].contains(&status)
        && frozen.contains(&status)
        && matches!(state, "failed" | "incomplete")
}

fn unclassified(run: &str, code: &str) {
    // D11: unclassifiable failure remains a failure; it cannot abort terminal
    // persistence or mint permission to supplement from a different request.
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_service_classification",
        &format!("{code}:{run}"),
        std::time::Instant::now(),
    );
}

/// Only the driver handling a typed terminal provider failure calls this.
/// A recoverable earlier error or model-authored text cannot mint eligibility.
pub(crate) fn record_failure(
    db: &Db,
    run: &super::EvaluationResult,
    status: u16,
    request_id: Option<&str>,
) -> io::Result<()> {
    let plan_id: Option<String> = db
        .conn()
        .query_row(
            "SELECT plan_id FROM evaluation_plan_runs WHERE run_id=?1",
            [&run.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let Some(plan_id) = plan_id else {
        return Ok(());
    };
    let plan = plan::read(db, &plan_id)?;
    let batch = config::read(db, &plan.batch_id)?;
    if !eligible(
        status,
        &batch.runtime.host_policy.supplement_statuses,
        "failed",
    ) {
        unclassified(&run.id, "status_not_frozen");
        return Ok(());
    }
    let worker = Db::open_current(std::path::Path::new(&run.workspace).join(".hexagon/state.db"))
        .map_err(err)?;
    let receipt:Option<(String,Option<u16>,String)>=worker.conn().query_row("SELECT request_id,http_status,request_state FROM usage WHERE record_kind='request' AND request_id=?1",[request_id.unwrap_or("")],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
    let Some((request, Some(observed), state)) = receipt else {
        unclassified(&run.id, "service_receipt_missing");
        return Ok(());
    };
    if observed != status || state != "failed" {
        unclassified(&run.id, "service_receipt_mismatch");
        return Ok(());
    }
    let fingerprint = config::digest(&(&run.id, &request, status, &batch.fingerprint))?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_service_failures(run_id,request_id,status,batch_fingerprint,receipt_fingerprint) VALUES (?1,?2,?3,?4,?5)",rusqlite::params![run.id,request,status,batch.fingerprint,fingerprint]).map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_service_classification",
        &format!("bound_external_failure:{}:{request}", run.id),
        std::time::Instant::now(),
    );
    Ok(())
}

pub(crate) fn create(db: &Db, original: &str, position: usize) -> io::Result<EvaluationPlan> {
    let started = std::time::Instant::now();
    let result = create_inner(db, original, position);
    let reason = match &result {
        Ok(_) => "supplement_created",
        Err(e) => match e.to_string().as_str() {
            "invalid_original_pair" => "invalid_original_pair",
            "supplement_already_created" => "supplement_already_created",
            "original_pair_unfinished" => "original_pair_unfinished",
            "service_receipt_corrupt" => "service_receipt_corrupt",
            "no_frozen_external_failure_receipt" => "no_frozen_external_failure_receipt",
            _ => "evidence_unreadable",
        },
    };
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_supplement",
        &format!("{reason}:{original}:{position}"),
        started,
    );
    result
}
fn create_inner(db: &Db, original: &str, position: usize) -> io::Result<EvaluationPlan> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let parent = plan::read(db, original)?;
    if parent.supplement.is_some()
        || !position.is_multiple_of(2)
        || position + 1 >= parent.entries.len()
    {
        return Err(reject("invalid_original_pair"));
    }
    let exists:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_supplements WHERE original_plan=?1 AND original_position=?2)",rusqlite::params![original,position as i64],|r|r.get(0)).map_err(err)?;
    if exists {
        return Err(reject("supplement_already_created"));
    }
    let pair = &parent.entries[position..position + 2];
    if pair.iter().any(|r| {
        matches!(
            r.state,
            plan::PlannedState::Planned | plan::PlannedState::Started
        )
    }) {
        return Err(reject("original_pair_unfinished"));
    }
    let batch = config::read(db, &parent.batch_id)?;
    let mut origin = None;
    for entry in pair {
        let Some(id) = &entry.run_id else { continue };
        let run = super::read(db, id)?;
        // D11: hitting a limit or an owner stop cannot be relabelled as an
        // external failure merely because a service error also occurred.
        if super::control::read(db, id)?.reason.is_some() {
            continue;
        }

        let mut q=db.conn().prepare("SELECT request_id,status,batch_fingerprint,receipt_fingerprint FROM evaluation_service_failures WHERE run_id=?1 ORDER BY request_id").map_err(err)?;
        let receipts = q
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u16>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        for (request, status, binding, fingerprint) in receipts {
            if binding != batch.fingerprint
                || fingerprint != config::digest(&(id, &request, status, &binding))?
            {
                return Err(reject("service_receipt_corrupt"));
            }
            if eligible(
                status,
                &batch.runtime.host_policy.supplement_statuses,
                &run.state,
            ) {
                origin = Some(SupplementOrigin {
                    plan_id: original.into(),
                    position,
                    run_id: id.clone(),
                    request_id: request,
                });
                break;
            }
        }
    }
    let origin = origin.ok_or_else(|| reject("no_frozen_external_failure_receipt"))?;
    let mut entries = pair.to_vec();
    for (i, e) in entries.iter_mut().enumerate() {
        e.position = i;
        e.state = plan::PlannedState::Planned;
        e.run_id = None;
        e.reason = Some("awaiting_admission".into());
    }
    let result = EvaluationPlan {
        id: format!("plan-{}", db.next_id("evaluation_plan").map_err(err)?),
        batch_id: parent.batch_id,
        batch_fingerprint: parent.batch_fingerprint,
        kind: parent.kind,
        entries,
        supplement: Some(origin),
    };
    db.conn().execute("INSERT INTO evaluation_plans(id,batch_id,kind,plan_json,fingerprint) VALUES (?1,?2,?3,?4,?5)",rusqlite::params![result.id,result.batch_id,format!("supplement:{original}:{position}"),serde_json::to_string(&result)?,config::digest(&result)?]).map_err(err)?;
    for e in &result.entries {
        db.conn().execute("INSERT INTO evaluation_plan_runs(plan_id,position,state,reason) VALUES (?1,?2,'planned','awaiting_admission')",rusqlite::params![result.id,e.position as i64]).map_err(err)?;
    }
    db.conn().execute("INSERT INTO evaluation_supplements(original_plan,original_position,supplemental_plan) VALUES (?1,?2,?3)",rusqlite::params![original,position as i64,result.id]).map_err(err)?;
    // Same round, unchanged price and pilot cap; no reset or unknown-fee refund.
    db.conn().execute("INSERT INTO evaluation_budget_plans(plan_id,price_json) SELECT ?2,price_json FROM evaluation_budget_plans WHERE plan_id=?1",rusqlite::params![original,result.id]).map_err(err)?;
    tx.commit().map_err(err)?;
    plan::read(db, &result.id)
}

pub(crate) fn list(db: &Db, original: &str) -> io::Result<Vec<EvaluationPlan>> {
    plan::read(db, original)?;
    let mut q=db.conn().prepare("SELECT supplemental_plan FROM evaluation_supplements WHERE original_plan=?1 ORDER BY original_position").map_err(err)?;
    let ids = q
        .query_map([original], |r| r.get::<_, String>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    ids.into_iter().map(|id| plan::read(db, &id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn missing_or_nonexternal_failure_never_qualifies(status in prop_oneof![Just(502u16),Just(503u16),Just(504u16),any::<u16>()], state in prop_oneof![Just("failed".to_string()),Just("incomplete".to_string()),".{0,25}"]) {
            prop_assert!(!eligible(status,&[],&state));
            for allowed in [502,503,504] {
                prop_assert!(eligible(allowed,&[502,503,504],"failed"));
            }
            if eligible(status,&[502,503,504],&state) {
                prop_assert!([502,503,504].contains(&status));
                prop_assert!(state=="failed" || state=="incomplete");
            }
            prop_assert!(!eligible(status,&[502,503,504],"completed"));
        }
    }
}
