//! D10: inspect original evaluation facts without dispatching or rewriting them.
use super::{err, EvaluationControlState};
use crate::db::Db;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    Running,
    WaitingHuman,
    Paused,
    Ended,
    NeedsReconciliation,
    MissingRun,
    Unverified,
    Corrupt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEntry {
    pub run_id: String,
    pub state: RecoveryState,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub version: u32,
    pub entries: Vec<RecoveryEntry>,
}

pub(crate) fn inspect(db: &Db, root: &std::path::Path) -> io::Result<RecoveryReport> {
    let mut q=db.conn().prepare("SELECT id FROM evaluation_runs UNION SELECT run_id FROM evaluation_plan_runs WHERE run_id IS NOT NULL UNION SELECT run_id FROM evaluation_leases WHERE kind IN ('driver','activity') UNION SELECT id FROM evaluation_preflights UNION SELECT id FROM evaluation_policy_generations ORDER BY 1").map_err(err)?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let entries = ids
        .into_iter()
        .map(|id| inspect_one(db, root, &id))
        .collect::<io::Result<_>>()?;
    Ok(RecoveryReport {
        version: 1,
        entries,
    })
}
fn inspect_one(db: &Db, root: &std::path::Path, id: &str) -> io::Result<RecoveryEntry> {
    match activity(db, id) {
        Ok(Some(activity)) => return inspect_activity(db, root, id, &activity),
        Ok(None) => (),
        Err(_) => {
            return Ok(RecoveryEntry {
                run_id: id.into(),
                state: RecoveryState::Corrupt,
                reasons: vec!["activity_record_invalid".into()],
            })
        }
    }
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_runs WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let (state, reason) = if !exists {
        (RecoveryState::MissingRun, Some("claimed_without_result"))
    } else {
        match super::read(db, id) {
            Err(_) => (RecoveryState::Corrupt, Some("run_record_invalid")),
            Ok(run) => {
                if let Some(attention) = open_attention(db, id)? {
                    if !matches!(
                        probe_lease(db, root, "attention", &attention)?,
                        LeaseProbe::Live
                    ) {
                        return Ok(RecoveryEntry {
                            run_id: id.into(),
                            state: RecoveryState::NeedsReconciliation,
                            reasons: vec!["owner_attention_unverifiable".into()],
                        });
                    }
                }

                if run.state == "started" {
                    match probe_lease(db, root, "driver", id)? {
                        LeaseProbe::Live => {
                            return Ok(RecoveryEntry {
                                run_id: id.into(),
                                state: RecoveryState::Running,
                                reasons: vec![],
                            })
                        }
                        LeaseProbe::Missing | LeaseProbe::Invalid => {
                            return Ok(RecoveryEntry {
                                run_id: id.into(),
                                state: RecoveryState::Unverified,
                                reasons: vec!["execution_lease_unverifiable".into()],
                            })
                        }
                        LeaseProbe::Available(_) => (),
                    }
                }

                if validate_run(db, root, &run).is_err() {
                    return Ok(RecoveryEntry {
                        run_id: id.into(),
                        state: RecoveryState::Corrupt,
                        reasons: vec!["worker_or_task_binding_invalid".into()],
                    });
                }
                let control: Option<String> = db
                    .conn()
                    .query_row(
                        "SELECT state FROM evaluation_controls WHERE run_id=?1",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(err)?;
                match control {
                    None => (RecoveryState::Unverified, Some("legacy_control_missing")),
                    Some(_) => match super::control::read(db, id) {
                        Err(_) => (RecoveryState::Corrupt, Some("control_or_workspace_invalid")),
                        Ok(c) => match c.state {
                            EvaluationControlState::WaitingHuman
                                if run.state == "waiting_human" =>
                            {
                                (RecoveryState::WaitingHuman, None)
                            }
                            EvaluationControlState::Paused => (RecoveryState::Paused, None),
                            EvaluationControlState::Ended | EvaluationControlState::Interrupted => {
                                (RecoveryState::Ended, None)
                            }
                            _ => (
                                RecoveryState::NeedsReconciliation,
                                Some("execution_owner_not_verified"),
                            ),
                        },
                    },
                }
            }
        }
    };
    Ok(RecoveryEntry {
        run_id: id.into(),
        state,
        reasons: reason.into_iter().map(str::to_string).collect(),
    })
}

pub(crate) enum LeaseProbe {
    Live,
    Available(std::fs::File),
    Missing,
    Invalid,
}
fn lease_dir(root: &std::path::Path) -> io::Result<std::path::PathBuf> {
    let dir = root.join(".hexagon/evaluation-leases");
    if dir.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
        return Err(err("lease directory is an alias"));
    }
    Ok(dir)
}
pub(crate) fn new_lease(
    db: &Db,
    root: &std::path::Path,
    kind: &str,
    owner: &str,
    run: &str,
) -> io::Result<std::fs::File> {
    use std::io::Write;
    let dir = lease_dir(root)?;
    std::fs::create_dir_all(&dir)?;
    let (mut file, path) = tempfile::Builder::new()
        .prefix("execution-")
        .tempfile_in(&dir)?
        .keep()
        .map_err(err)?;
    file.try_lock().map_err(err)?;
    file.write_all(&serde_json::to_vec(&(kind, owner, run))?)?;
    file.sync_all()?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| err("invalid lease file"))?;
    db.conn()
        .execute(
            "INSERT INTO evaluation_leases(kind,owner_id,run_id,file_name) VALUES (?1,?2,?3,?4)",
            rusqlite::params![kind, owner, run, name],
        )
        .map_err(err)?;
    Ok(file)
}
pub(crate) fn probe_lease(
    db: &Db,
    root: &std::path::Path,
    kind: &str,
    owner: &str,
) -> io::Result<LeaseProbe> {
    use std::io::Read;
    let record: Option<(String, String)> = db
        .conn()
        .query_row(
            "SELECT run_id,file_name FROM evaluation_leases WHERE kind=?1 AND owner_id=?2",
            [kind, owner],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    let Some((run, name)) = record else {
        return Ok(LeaseProbe::Missing);
    };
    if std::path::Path::new(&name).components().count() != 1 || !name.starts_with("execution-") {
        return Ok(LeaseProbe::Invalid);
    }
    let path = lease_dir(root)?.join(name);
    if !path
        .symlink_metadata()
        .is_ok_and(|m| m.is_file() && !m.is_symlink())
    {
        return Ok(LeaseProbe::Invalid);
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut identity = Vec::new();
    file.by_ref().take(4096).read_to_end(&mut identity)?;
    if !lease_identity_matches(&identity, kind, owner, &run) {
        return Ok(LeaseProbe::Invalid);
    }
    match file.try_lock() {
        Ok(()) => Ok(LeaseProbe::Available(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(LeaseProbe::Live),
        Err(std::fs::TryLockError::Error(_)) => Ok(LeaseProbe::Invalid),
    }
}
// D12: false rejection needs inspection; false acceptance can steal live work.
fn lease_identity_matches(bytes: &[u8], kind: &str, owner: &str, run: &str) -> bool {
    serde_json::to_vec(&(kind, owner, run)).is_ok_and(|expected| bytes == expected)
}
pub(crate) fn resume_driver(
    db: &Db,
    root: &std::path::Path,
    id: &str,
) -> io::Result<std::fs::File> {
    match probe_lease(db, root, "driver", id)? {
        LeaseProbe::Available(file) => Ok(file),
        _ => Err(refusal(id, "driver_lease_unverifiable")),
    }
}

pub(crate) fn reconcile(db: &Db, root: &std::path::Path, id: &str) -> io::Result<RecoveryEntry> {
    if let Some(activity) = activity(db, id)? {
        return reconcile_activity(db, root, id, activity);
    }
    let _owner = resume_driver(db, root, id)?;
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_runs WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if !exists {
        let tx = rusqlite::Transaction::new_unchecked(
            db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(err)?;
        super::plan::abandon_missing(db, id)?;
        db.conn().execute("INSERT OR IGNORE INTO evaluation_reconciliations(run_id,kind,observed_at_ms) VALUES (?1,'result_missing',?2)",rusqlite::params![id,i64::try_from(super::now_ms()?).map_err(err)?]).map_err(err)?;
        tx.commit().map_err(err)?;
        return inspect_one(db, root, id);
    }
    let mut run = super::read(db, id)?;
    validate_run(db, root, &run).map_err(|_| refusal(id, "worker_or_task_binding_invalid"))?;
    reconcile_attention(db, root, id)?;
    let Some(next_state) = recovery_outcome(&run.state) else {
        if matches!(run.state.as_str(), "completed" | "failed" | "incomplete") {
            let tx = rusqlite::Transaction::new_unchecked(
                db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(err)?;
            super::settle_terminal(db, &run)?;
            tx.commit().map_err(err)?;
        }
        return inspect_one(db, root, id);
    };
    let _work = super::control::idle_leases(std::path::Path::new(&run.workspace))?;
    let worker = Db::open_current(std::path::Path::new(&run.workspace).join(".hexagon/state.db"))
        .map_err(err)?;
    crate::usage::recover_requests(
        &worker,
        std::path::Path::new(&run.workspace),
        crate::PROJECT_ID,
    )
    .map_err(err)?;
    crate::actions::recover(&worker, crate::PROJECT_ID).map_err(err)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    super::budget::recover_abandoned(db, root, std::path::Path::new(&run.workspace))?;
    // D10: a missing driver is not proof every detached effect has stopped.
    // Preserve the unknown cleanup boundary and all fee holds; never replay.
    super::control::abandon(db, &run)?;
    run.state = next_state.into();
    run.flow_completed = false;
    run.error = Some("execution_owner_lost".into());
    super::update_started(db, &run)?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_reconciliations(run_id,kind,observed_at_ms) VALUES (?1,'driver_lost',?2)",rusqlite::params![id,i64::try_from(super::now_ms()?).map_err(err)?]).map_err(err)?;
    tx.commit().map_err(err)?;
    inspect_one(db, root, id)
}

fn refusal(id: &str, code: &str) -> io::Error {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_recovery",
        &format!("{code}:{id}"),
        std::time::Instant::now(),
    );
    err(code)
}

fn validate_run(db: &Db, root: &std::path::Path, run: &super::EvaluationResult) -> io::Result<()> {
    use sha2::{Digest, Sha256};
    super::control::validate_owner(root, run)?;
    super::budget::validate_recovery_binding(db, root, run)?;
    let json: String = db
        .conn()
        .query_row(
            "SELECT task_json FROM evaluation_runs WHERE id=?1",
            [&run.id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let task: super::EvaluationTask = serde_json::from_str(&json)?;
    if task.id != run.task_id
        || format!("v1:{:x}", Sha256::digest(serde_json::to_vec(&task)?)) != run.task_fingerprint
    {
        return Err(err("evaluation task binding mismatch"));
    }
    Ok(())
}

fn open_attention(db: &Db, id: &str) -> io::Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT id FROM evaluation_human_intervals WHERE run_id=?1 AND ended_at_ms IS NULL",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)
}
fn reconcile_attention(db: &Db, root: &std::path::Path, id: &str) -> io::Result<()> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    if let Some(attention) = open_attention(db, id)? {
        let _lease = match probe_lease(db, root, "attention", &attention)? {
            LeaseProbe::Available(file) => file,
            _ => return Err(refusal(id, "attention_lease_unverifiable")),
        };
        db.conn().execute("UPDATE evaluation_human_intervals SET ended_at_ms=?2,duration_ms=NULL,end_reason='owner_handle_lost' WHERE id=?1 AND ended_at_ms IS NULL",rusqlite::params![attention,i64::try_from(super::now_ms()?).map_err(err)?]).map_err(err)?;
        db.conn()
            .execute(
                "UPDATE evaluation_cursors SET unmeasured_changes=1 WHERE run_id=?1",
                [id],
            )
            .map_err(err)?;
        db.conn().execute("INSERT OR IGNORE INTO evaluation_reconciliations(run_id,kind,observed_at_ms) VALUES (?1,'attention_lost',?2)",rusqlite::params![id,i64::try_from(super::now_ms()?).map_err(err)?]).map_err(err)?;
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_recovery",
            &format!("owner_handle_lost:{id}"),
            std::time::Instant::now(),
        );
    }
    tx.commit().map_err(err)?;
    Ok(())
}

// D10/D12: rejecting recovery costs manual inspection; falsely promoting a
// crashed run fabricates success. Recovery can only lower a started outcome.
fn recovery_outcome(original: &str) -> Option<&'static str> {
    (original == "started").then_some("incomplete")
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrashPoint {
    Claimed,
    Reserved,
    Returned,
    Persisted,
    ResultPersisted,
}
#[cfg(test)]
thread_local! {static CRASH:std::cell::Cell<Option<CrashPoint>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
pub(crate) fn arm_crash(point: &str) -> io::Result<()> {
    let point = match point {
        "claimed" => CrashPoint::Claimed,
        "reserved" => CrashPoint::Reserved,
        "returned" => CrashPoint::Returned,
        "persisted" => CrashPoint::Persisted,
        "result" => CrashPoint::ResultPersisted,
        _ => return Err(err("unknown fixture crash boundary")),
    };
    CRASH.set(Some(point));
    Ok(())
}
#[cfg(test)]
pub(crate) fn crash_at(point: CrashPoint) {
    if CRASH.get() == Some(point) {
        std::process::exit(86);
    }
}

// Activities have no acceptance result or task coverage. Reconciliation can
// only close an abandoned activity as interrupted, never manufacture a receipt.
enum Activity {
    Preflight(super::PreflightReceipt),
    Generation(super::PolicyGeneration),
}
impl Activity {
    fn state(&self) -> &str {
        match self {
            Self::Preflight(r) => &r.state,
            Self::Generation(r) => &r.state,
        }
    }
    fn workspace(&self) -> &str {
        match self {
            Self::Preflight(r) => &r.workspace,
            Self::Generation(r) => &r.workspace,
        }
    }
}
fn activity(db: &Db, id: &str) -> io::Result<Option<Activity>> {
    let preflight: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_preflights WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if preflight {
        return Ok(Some(Activity::Preflight(super::live::read(db, id)?)));
    }
    let context: Option<String> = db
        .conn()
        .query_row(
            "SELECT context_id FROM evaluation_policy_generations WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    context
        .map(|c| super::generation::read(db, &c).map(Activity::Generation))
        .transpose()
}
fn inspect_activity(
    db: &Db,
    root: &std::path::Path,
    id: &str,
    activity: &Activity,
) -> io::Result<RecoveryEntry> {
    let state = if matches!(
        activity.state(),
        "passed" | "failed" | "completed" | "interrupted"
    ) {
        RecoveryState::Ended
    } else {
        match probe_lease(db, root, "activity", id)? {
            LeaseProbe::Live => RecoveryState::Running,
            LeaseProbe::Available(_) => RecoveryState::NeedsReconciliation,
            _ => RecoveryState::Unverified,
        }
    };
    Ok(RecoveryEntry {
        run_id: id.into(),
        state,
        reasons: if state == RecoveryState::NeedsReconciliation {
            vec!["activity_owner_lost".into()]
        } else {
            vec![]
        },
    })
}
fn reconcile_activity(
    db: &Db,
    root: &std::path::Path,
    id: &str,
    mut activity: Activity,
) -> io::Result<RecoveryEntry> {
    let _owner = match probe_lease(db, root, "activity", id)? {
        LeaseProbe::Available(file) => file,
        _ => return Err(refusal(id, "activity_lease_unverifiable")),
    };
    let workspace = std::path::Path::new(activity.workspace());
    let expected = root.canonicalize()?.join(match activity {
        Activity::Preflight(_) => ".hexagon/evaluation-preflights",
        Activity::Generation(_) => ".hexagon/evaluation-generators",
    });
    if workspace.canonicalize()?.parent() != Some(expected.as_path())
        || workspace.symlink_metadata()?.is_symlink()
    {
        return Err(refusal(id, "activity_workspace_invalid"));
    }
    // D10/17: these fixed synchronous model calls expose no task tools or
    // detached work. Their activity OS lease is the execution boundary.
    // Task evaluation-work.lock does not exist in a probe/generation worker.
    let worker = Db::open_current(workspace.join(".hexagon/state.db")).map_err(err)?;
    crate::usage::recover_requests(&worker, workspace, crate::PROJECT_ID).map_err(err)?;
    let paid = match &activity {
        Activity::Preflight(_) => true,
        Activity::Generation(op) => op.source_kind != "scripted_generation",
    };
    super::budget::live::recover_activity(db, root, workspace, id, paid)?;
    // Completed source and authority close are in separate databases. Repair
    // that crash window without downgrading or recreating completed evidence.
    if matches!(
        activity.state(),
        "passed" | "failed" | "completed" | "interrupted"
    ) {
        return inspect_activity(db, root, id, &activity);
    }
    match &mut activity {
        Activity::Preflight(receipt) => {
            receipt.state = "interrupted".into();
            receipt.reason = Some("activity_owner_lost".into());
            super::live::save(db, receipt)?;
        }
        Activity::Generation(op) => {
            op.state = "interrupted".into();
            op.reason = Some("activity_owner_lost".into());
            super::generation::save(db, op, None)?;
        }
    }
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_activity_recovery",
        "activity_owner_lost",
        std::time::Instant::now(),
    );
    inspect_activity(db, root, id, &activity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn lease_identity_cannot_cross_owners(kind in "[a-z]{1,12}", owner in "[a-z0-9]{1,24}", run in "[a-z0-9]{1,24}") {
            let bytes=serde_json::to_vec(&(&kind,&owner,&run)).unwrap();
            prop_assert!(lease_identity_matches(&bytes,&kind,&owner,&run));
            prop_assert!(!lease_identity_matches(&bytes,&format!("{}x",kind),&owner,&run), "kind mismatch");
            prop_assert!(!lease_identity_matches(&bytes,&kind,&format!("{}x",owner),&run), "owner mismatch");
            prop_assert!(!lease_identity_matches(&bytes,&kind,&owner,&format!("{}x",run)), "run mismatch");
        }

        #[test]
        fn recovery_never_creates_success_or_rewrites_a_terminal(original in prop_oneof![Just("started".to_string()), ".{0,80}"]) {
            if let Some(next)=recovery_outcome(&original) {
                prop_assert_eq!(&original,"started");
                prop_assert_eq!(next,"incomplete");
                prop_assert!(recovery_outcome(next).is_none());
            }
            for terminal in ["completed","failed","incomplete","waiting_human"] {
                prop_assert!(recovery_outcome(terminal).is_none());
            }
        }
    }
}
