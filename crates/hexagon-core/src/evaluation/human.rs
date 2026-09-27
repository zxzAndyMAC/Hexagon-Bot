//! D07: opaque monotonic attention handles and persistent intervention facts.
use super::{err, now_ms};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationActor {
    Human,
    Scripted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvaluationDecision {
    ApproveStamp,
    FinishReview,
    RejectStamp,
    ContinueRework,
    RejectFinal { stage: String, note: String },
    Permission { question_id: String, allow: bool },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionEnd {
    Submitted,
    Away,
    Finished,
}

/// Deliberately not Deserialize/Clone: callers cannot supply elapsed time or
/// manufacture a handle from a reported timestamp. CLI keeps it while handling.
pub struct AttentionHandle {
    pub(crate) id: String,
    pub(crate) run_id: String,
    host: PathBuf,
    started: Instant,
    _lease: std::fs::File,
}
impl AttentionHandle {
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanInterval {
    pub id: String,
    pub run_id: String,
    pub actor: EvaluationActor,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub end_reason: Option<String>,
    pub guidance_count: u32,
    pub manual_changes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTiming {
    pub run_id: String,
    pub human_ms: Option<u64>,
    pub active_ms: Option<u64>,
    pub waiting_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub human_benefit_eligible: bool,
    pub timing_complete: bool,
    pub intervals: Vec<HumanInterval>,
}

pub(crate) fn begin(
    db: &Db,
    host: &Path,
    run_id: &str,
    actor: EvaluationActor,
) -> io::Result<AttentionHandle> {
    let started = Instant::now();
    // D07: read the waiting state inside the same writer transaction as
    // attention admission. Otherwise submission could start work between
    // this read and the unique-owner check, double-counting active time.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let run = super::read(db, run_id)?;
    if !matches!(run.state.as_str(), "waiting_human")
        || super::control::read(db, run_id)?.state != super::EvaluationControlState::WaitingHuman
    {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_attention",
            "run_not_active",
            started,
        );
        return Err(err("attention requires an active or waiting run"));
    }
    let host = host.canonicalize()?;
    let fingerprint = super::fingerprint(Path::new(&run.workspace))?;
    let active: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_human_intervals WHERE ended_at_ms IS NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if active {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_attention",
            "owner_already_processing",
            started,
        );
        return Err(err("owner already has an active attention interval"));
    }
    // D07/T09: edits while away are observable, but their duration is not.
    // Preserve the sample and refuse a measured human-benefit conclusion.
    tx.execute("UPDATE evaluation_cursors SET unmeasured_changes=1 WHERE run_id=?1 AND wait_fingerprint IS NOT NULL AND wait_fingerprint!=?2",rusqlite::params![run_id,fingerprint]).map_err(err)?;
    let id = format!(
        "attention-{}",
        db.next_id("evaluation_attention").map_err(err)?
    );
    let actor = match actor {
        EvaluationActor::Human => "human",
        EvaluationActor::Scripted => "scripted",
    };
    tx.execute("INSERT INTO evaluation_human_intervals(id,run_id,actor,started_at_ms,fingerprint_before) VALUES (?1,?2,?3,?4,?5)",rusqlite::params![id,run_id,actor,i64::try_from(now_ms()?).map_err(err)?,fingerprint]).map_err(err)?;
    let lease = super::recovery::new_lease(db, &host, "attention", &id, run_id)?;
    tx.commit().map_err(err)?;
    Ok(AttentionHandle {
        id,
        run_id: run_id.into(),
        host,
        started: Instant::now(),
        _lease: lease,
    })
}

pub(crate) fn end(
    db: &Db,
    host: &Path,
    handle: &AttentionHandle,
    reason: AttentionEnd,
) -> io::Result<HumanInterval> {
    validate_handle(db, host, handle)?;
    let run = super::read(db, &handle.run_id)?;
    let fingerprint = super::fingerprint(Path::new(&run.workspace))?;
    let reason = match reason {
        AttentionEnd::Submitted => "submitted",
        AttentionEnd::Away => "away",
        AttentionEnd::Finished => "finished",
    };
    let duration = i64::try_from(handle.started.elapsed().as_millis()).map_err(err)?;
    let changed=db.conn().execute("UPDATE evaluation_human_intervals SET ended_at_ms=?3,duration_ms=?4,end_reason=?5,fingerprint_after=?6 WHERE id=?1 AND run_id=?2 AND ended_at_ms IS NULL",rusqlite::params![handle.id,handle.run_id,i64::try_from(now_ms()?).map_err(err)?,duration,reason,fingerprint]).map_err(err)?;
    if changed != 1 {
        return Err(err("attention interval already closed or missing"));
    }
    db.conn()
        .execute(
            "UPDATE evaluation_cursors SET wait_fingerprint=?2 WHERE run_id=?1",
            rusqlite::params![handle.run_id, fingerprint],
        )
        .map_err(err)?;
    interval(db, &handle.id)
}

fn interval(db: &Db, id: &str) -> io::Result<HumanInterval> {
    let (run,actor,start,end,duration,reason):(String,String,u64,Option<u64>,Option<u64>,Option<String>)=db.conn().query_row("SELECT run_id,actor,started_at_ms,ended_at_ms,duration_ms,end_reason FROM evaluation_human_intervals WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,sql_time(r,2)?,sql_optional_time(r,3)?,sql_optional_time(r,4)?,r.get(5)?))).map_err(err)?;
    let actor = match actor.as_str() {
        "human" => EvaluationActor::Human,
        "scripted" => EvaluationActor::Scripted,
        _ => return Err(err("invalid attention actor")),
    };
    let (guidance_count,manual_changes):(u32,bool)=db.conn().query_row("SELECT guidance_count,COALESCE(fingerprint_before!=fingerprint_after,0) FROM evaluation_human_intervals WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
    Ok(HumanInterval {
        id: id.into(),
        run_id: run,
        actor,
        started_at_ms: start,
        ended_at_ms: end,
        duration_ms: duration,
        end_reason: reason,
        guidance_count,
        manual_changes,
    })
}

pub(crate) fn validate_handle(db: &Db, host: &Path, handle: &AttentionHandle) -> io::Result<()> {
    let started = Instant::now();
    let active:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_human_intervals WHERE id=?1 AND run_id=?2 AND ended_at_ms IS NULL)",rusqlite::params![handle.id,handle.run_id],|r|r.get(0)).map_err(err)?;
    if !active || host.canonicalize()? != handle.host {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_attention",
            "invalid_handle",
            started,
        );
        return Err(err("attention handle closed or belongs to another host"));
    }
    Ok(())
}

pub(crate) fn timing(db: &Db, run_id: &str) -> io::Result<RunTiming> {
    let run = super::read(db, run_id)?;
    let mut query = db
        .conn()
        .prepare(
            "SELECT id FROM evaluation_human_intervals WHERE run_id=?1 ORDER BY started_at_ms,id",
        )
        .map_err(err)?;
    let ids = query
        .query_map([run_id], |r| r.get::<_, String>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let intervals = ids
        .iter()
        .map(|id| interval(db, id))
        .collect::<io::Result<Vec<_>>>()?;
    let unmeasured:bool=db.conn().query_row("SELECT COALESCE((SELECT unmeasured_changes FROM evaluation_cursors WHERE run_id=?1),1)",[run_id],|r|r.get(0)).map_err(err)?;
    let recovered_unknown: bool = db
        .conn()
        .query_row(
            "SELECT COALESCE((SELECT recovery_unknown FROM evaluation_controls WHERE run_id=?1),1)",
            [run_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let complete = !recovered_unknown
        && !unmeasured
        && !intervals.is_empty()
        && intervals.iter().all(|i| i.duration_ms.is_some());
    let eligible = complete && human_eligible(&run.evidence_kind, &intervals);
    let sum = intervals
        .iter()
        .filter_map(|i| i.duration_ms)
        .fold(0u64, u64::saturating_add);
    let times=db.conn().query_row("SELECT started_at_ms,ended_at_ms,waiting_since_ms,waiting_ms FROM evaluation_cursors WHERE run_id=?1",[run_id],|r|Ok((sql_time(r,0)?,sql_optional_time(r,1)?,sql_optional_time(r,2)?,sql_time(r,3)?))).optional().map_err(err)?;
    let (elapsed, waiting) = match times {
        Some((start, end, since, wait)) => {
            let until = end.unwrap_or(now_ms()?);
            (
                Some(until.saturating_sub(start)),
                Some(
                    wait.saturating_add(since.map(|s| until.saturating_sub(s)).unwrap_or(0))
                        .saturating_sub(sum),
                ),
            )
        }
        None => (None, None),
    };
    Ok(RunTiming {
        run_id: run_id.into(),
        human_ms: eligible.then_some(sum),
        active_ms: (!recovered_unknown).then_some(run.elapsed_ms.saturating_add(sum)),
        waiting_ms: waiting,
        elapsed_ms: elapsed,
        human_benefit_eligible: eligible,
        timing_complete: complete,
        intervals,
    })
}

fn human_eligible(evidence: &str, intervals: &[HumanInterval]) -> bool {
    // D07/T09: false negatives require another measured sample; false
    // positives invent a benefit. Missing or scripted timing fails closed.
    evidence == "live_model"
        && !intervals.is_empty()
        && intervals.iter().all(|i| {
            i.actor == EvaluationActor::Human && i.duration_ms.is_some() && i.ended_at_ms.is_some()
        })
}
use rusqlite::OptionalExtension;

fn sql_time(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
fn sql_optional_time(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|v| {
            u64::try_from(v).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn missing_or_scripted_attention_never_claims_human_benefit(ms in 0u64..100000, scripted in any::<bool>(), missing in any::<bool>()) {
            let interval=HumanInterval{id:"i".into(),run_id:"r".into(),actor:if scripted {EvaluationActor::Scripted}else{EvaluationActor::Human},started_at_ms:0,ended_at_ms:Some(ms),duration_ms:(!missing).then_some(ms),end_reason:Some("submitted".into()),guidance_count:0,manual_changes:false};
            prop_assert!(!human_eligible("scripted_debug",std::slice::from_ref(&interval)));
            if scripted || missing {prop_assert!(!human_eligible("live_model",&[interval]));}
            prop_assert!(!human_eligible("live_model",&[]));
        }
    }
}
