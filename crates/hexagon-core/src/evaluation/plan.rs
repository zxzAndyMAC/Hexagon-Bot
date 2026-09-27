//! Ticket 07 / D04: persisted balanced order; dispatch never selects by outcome.
use super::{config, err};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationArm {
    Fast,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Pilot,
    Formal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedState {
    Planned,
    Started,
    Completed,
    Incomplete,
    Failed,
    NotRun,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStopReason {
    OwnerStopped,
    BudgetInsufficient,
    ConfigurationBlocked,
}

pub(crate) fn stop(db: &Db, id: &str, reason: PlanStopReason) -> io::Result<EvaluationPlan> {
    let started = std::time::Instant::now();
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    read(db, id)?;
    let code = match reason {
        PlanStopReason::OwnerStopped => "owner_stopped",
        PlanStopReason::BudgetInsufficient => "budget_insufficient",
        PlanStopReason::ConfigurationBlocked => "configuration_blocked",
    };
    // Ticket 07 review: stopping must preserve unstarted coverage as facts.
    // In-flight runs are not relabelled or cancelled here (tickets 12/13).
    tx.execute("UPDATE evaluation_plan_runs SET state='not_run',reason=?2 WHERE plan_id=?1 AND state='planned'",rusqlite::params![id,code]).map_err(err)?;
    tx.commit().map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_plan_stop",
        code,
        started,
    );
    read(db, id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedRun {
    pub position: usize,
    pub task_id: String,
    pub repetition: u32,
    pub arm: EvaluationArm,
    pub state: PlannedState,
    pub run_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationPlan {
    pub id: String,
    pub batch_id: String,
    pub batch_fingerprint: String,
    pub kind: PlanKind,
    pub entries: Vec<PlannedRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugActivation {
    #[serde(default)]
    pub request_baseline_merge: bool,
    pub role: String,
    pub writes: std::collections::BTreeMap<String, String>,
}

fn shuffle<T>(items: &mut [T]) -> io::Result<()> {
    for end in (1..items.len()).rev() {
        let range = (end + 1) as u64;
        let limit = u64::MAX - u64::MAX % range;
        let index = loop {
            let mut bytes = [0; 8];
            getrandom::fill(&mut bytes).map_err(err)?;
            let n = u64::from_le_bytes(bytes);
            if n < limit {
                break (n % range) as usize;
            }
        };
        items.swap(end, index);
    }
    Ok(())
}

fn entries(batch: &super::EvaluationBatch, kind: PlanKind) -> io::Result<Vec<PlannedRun>> {
    let mut pairs = Vec::new();
    for case in batch.request.corpora.iter().flat_map(|c| &c.cases) {
        let selected = match kind {
            PlanKind::Pilot => case.pilot,
            PlanKind::Formal => case.split == "heldout",
        };
        if selected {
            for repetition in 1..=if kind == PlanKind::Formal { 3 } else { 1 } {
                pairs.push((case.task.id.clone(), repetition));
            }
        }
    }
    order_pairs(pairs)
}

fn order_pairs(mut pairs: Vec<(String, u32)>) -> io::Result<Vec<PlannedRun>> {
    shuffle(&mut pairs)?;
    let mut first: Vec<bool> = (0..pairs.len()).map(|i| i < pairs.len() / 2).collect();
    shuffle(&mut first)?;
    let mut entries = Vec::new();
    for ((task_id, repetition), fast_first) in pairs.into_iter().zip(first) {
        let arms = if fast_first {
            [EvaluationArm::Fast, EvaluationArm::Full]
        } else {
            [EvaluationArm::Full, EvaluationArm::Fast]
        };
        for arm in arms {
            entries.push(PlannedRun {
                position: entries.len(),
                task_id: task_id.clone(),
                repetition,
                arm,
                state: PlannedState::Planned,
                run_id: None,
                reason: Some("awaiting_admission".into()),
            });
        }
    }
    Ok(entries)
}

pub(crate) fn create(db: &Db, batch_id: &str, kind: PlanKind) -> io::Result<EvaluationPlan> {
    let batch = config::read(db, batch_id)?;
    let kind_json = match kind {
        PlanKind::Pilot => "pilot",
        PlanKind::Formal => "formal",
    };
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    use rusqlite::OptionalExtension;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM evaluation_plans WHERE batch_id=?1 AND kind=?2",
            rusqlite::params![batch_id, kind_json],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = existing {
        tx.commit().map_err(err)?;
        return read(db, &id);
    }
    let plan = EvaluationPlan {
        id: format!("plan-{}", db.next_id("evaluation_plan").map_err(err)?),
        batch_id: batch_id.into(),
        batch_fingerprint: batch.fingerprint.clone(),
        kind,
        entries: entries(&batch, kind)?,
    };
    tx.execute("INSERT INTO evaluation_plans(id,batch_id,kind,plan_json,fingerprint) VALUES (?1,?2,?3,?4,?5)",rusqlite::params![plan.id,batch_id,kind_json,serde_json::to_string(&plan)?,config::digest(&plan)?]).map_err(err)?;
    for entry in &plan.entries {
        tx.execute("INSERT INTO evaluation_plan_runs(plan_id,position,state,reason) VALUES (?1,?2,'planned','awaiting_admission')",rusqlite::params![plan.id,i64::try_from(entry.position).map_err(err)?]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    read(db, &plan.id)
}

pub(crate) fn read(db: &Db, id: &str) -> io::Result<EvaluationPlan> {
    let (json, fingerprint): (String, String) = db
        .conn()
        .query_row(
            "SELECT plan_json,fingerprint FROM evaluation_plans WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    let mut plan: EvaluationPlan = serde_json::from_str(&json)?;
    if plan.id != id || config::digest(&plan)? != fingerprint {
        return Err(err("damaged evaluation plan"));
    }
    let batch = config::read(db, &plan.batch_id)?;
    if batch.fingerprint != plan.batch_fingerprint {
        return Err(err("plan batch identity mismatch"));
    }
    for entry in &mut plan.entries {
        let (state, run, reason): (String,Option<String>,Option<String>) = db.conn().query_row("SELECT state,run_id,reason FROM evaluation_plan_runs WHERE plan_id=?1 AND position=?2",rusqlite::params![id,i64::try_from(entry.position).map_err(err)?],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
        entry.state = serde_json::from_value(serde_json::Value::String(state))?;
        entry.run_id = run;
        entry.reason = reason;
    }
    Ok(plan)
}

fn next_position(entries: &[PlannedRun]) -> io::Result<usize> {
    // D04: a false rejection costs a retry; a false admission duplicates an
    // attempt and can spend twice. Any active or explicitly stopped row blocks.
    if entries.iter().any(|e| e.state == PlannedState::Started) {
        return Err(err("plan already has an active run"));
    }
    let entry = entries
        .iter()
        .find(|e| matches!(e.state, PlannedState::Planned | PlannedState::NotRun))
        .ok_or_else(|| err("plan has no unstarted run"))?;
    if entry.state == PlannedState::NotRun {
        return Err(err("plan stopped; original coverage is retained"));
    }
    Ok(entry.position)
}

/// Claim only the next original entry. A crash cannot make a new attempt appear
/// unstarted; recovery later resolves the durable started record (tickets 12/13).
pub(crate) fn claim(
    db: &Db,
    root: &std::path::Path,
    id: &str,
) -> io::Result<(EvaluationPlan, usize, String, std::fs::File)> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    super::budget::before_claim(db, root, id)?;
    let plan = read(db, id)?;
    let started = std::time::Instant::now();
    let position = next_position(&plan.entries).inspect_err(|_| {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_plan_admission",
            "active_stopped_or_exhausted",
            started,
        );
    })?;
    let batch = config::read(db, &plan.batch_id)?;
    if plan.kind == PlanKind::Formal
        && batch
            .blocks
            .contains(&super::AdmissionBlock::HeldoutRetired)
    {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_plan_admission",
            "heldout_retired",
            started,
        );
        return Err(err(
            "heldout tasks are retired; regression use cannot qualify",
        ));
    }
    let run_id = format!("eval-{}", db.next_id("evaluation").map_err(err)?);
    let owner = super::recovery::new_lease(db, root, "driver", &run_id, &run_id)?;
    tx.execute("UPDATE evaluation_plan_runs SET state='started',run_id=?3,reason=NULL WHERE plan_id=?1 AND position=?2 AND state='planned'",rusqlite::params![id,i64::try_from(position).map_err(err)?,run_id]).map_err(err)?;
    let task = &batch
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == plan.entries[position].task_id)
        .ok_or_else(|| err("planned task missing"))?
        .task;
    super::isolation::record_use(db, &batch.id, &run_id, task)?;
    tx.commit().map_err(err)?;
    #[cfg(test)]
    super::recovery::crash_at(super::recovery::CrashPoint::Claimed);
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_plan_admission",
        &format!("started:{run_id}"),
        started,
    );
    Ok((plan, position, run_id, owner))
}

pub(crate) fn finish(db: &Db, id: &str, position: usize, run_id: &str) -> io::Result<()> {
    let result = super::read(db, run_id)?;
    if !matches!(result.state.as_str(), "completed" | "incomplete" | "failed") {
        return Err(err("run is not terminal"));
    }
    let changed = db.conn().execute("UPDATE evaluation_plan_runs SET state=?4,reason=?5 WHERE plan_id=?1 AND position=?2 AND run_id=?3 AND state='started'",rusqlite::params![id,i64::try_from(position).map_err(err)?,run_id,result.state,result.error]).map_err(err)?;
    if changed != 1 {
        let same:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_plan_runs WHERE plan_id=?1 AND position=?2 AND run_id=?3 AND state=?4)",rusqlite::params![id,i64::try_from(position).map_err(err)?,run_id,result.state],|r|r.get(0)).map_err(err)?;
        if !same {
            return Err(err("plan run no longer active"));
        }
    }
    Ok(())
}

/// D10: admission survived but no result was materialized. Keep the original
/// identity and denominator; do not manufacture a successful/empty result.
pub(crate) fn abandon_missing(db: &Db, id: &str) -> io::Result<()> {
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_runs WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if exists {
        return Err(err("run already has a result"));
    }
    db.conn().execute("UPDATE evaluation_plan_runs SET state='incomplete',reason='interrupted_before_result' WHERE run_id=?1 AND state='started'",[id]).map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn balanced_pairs_stay_adjacent_and_active_run_always_blocks(half in 1usize..13, active in 0usize..48) {
            let pairs: Vec<_> = (0..half*2).map(|n|(format!("task-{n}"),1)).collect();
            let mut entries = order_pairs(pairs).unwrap();
            prop_assert_eq!(entries.len(),half*4);
            prop_assert_eq!(entries.iter().step_by(2).filter(|e|e.arm==EvaluationArm::Fast).count(),half);
            let unique: std::collections::BTreeSet<_> = entries.iter().step_by(2).map(|e|e.task_id.clone()).collect();
            prop_assert_eq!(unique.len(),half*2);
            for pair in entries.as_chunks::<2>().0 {
                prop_assert_eq!(&pair[0].task_id,&pair[1].task_id);
                prop_assert_eq!(pair[0].repetition,pair[1].repetition);
                prop_assert_ne!(pair[0].arm,pair[1].arm);
            }
            let index = active % entries.len();
            entries[0].state=PlannedState::NotRun;
            prop_assert!(next_position(&entries).is_err());
            entries[0].state=PlannedState::Planned;
            entries[index].state=PlannedState::Started;
            prop_assert!(next_position(&entries).is_err());
        }
    }
}
