//! D09: durable stop control; a stopped run is never a successful completion.
use super::{err, now_ms, EvaluationResult};
use crate::db::Db;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationControlState {
    Running,
    WaitingHuman,
    Paused,
    Stopping,
    Interrupted,
    Ended,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationControl {
    pub run_id: String,
    pub state: EvaluationControlState,
    pub reason: Option<String>,
    pub active_ms: u64,
    pub active_limit_ms: u64,
    pub pending_requests: u64,
    pub unknown_requests: u64,
    pub remote_cancellation_confirmed: bool,
    pub stopped_at_ms: Option<u64>,
    pub cleanup_elapsed_ms: Option<u64>,
    pub active_time_complete: bool,
    pub cleanup_confirmed: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    host: String,
    run_id: String,
    workspace: String,
}
fn num(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let n: i64 = row.get(index)?;
    u64::try_from(n).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, n))
}
fn optional_num(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    let n: Option<i64> = row.get(index)?;
    n.map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, v)))
        .transpose()
}
pub(crate) fn install(db: &Db, host: &Path, run: &EvaluationResult, limit: u64) -> io::Result<()> {
    let workspace = Path::new(&run.workspace).canonicalize()?;
    db.conn().execute("INSERT INTO evaluation_controls(run_id,state,active_limit_ms,phase_started_ms) VALUES (?1,'running',?2,?3)",params![run.id,i64::try_from(limit).map_err(err)?,i64::try_from(now_ms()?).map_err(err)?]).map_err(err)?;
    std::fs::create_dir_all(workspace.join(".hexagon"))?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(workspace.join(".hexagon/evaluation-work.lock"))?;
    std::fs::write(
        workspace.join(".hexagon/evaluation-control.json"),
        serde_json::to_vec(&Binding {
            host: host.canonicalize()?.to_string_lossy().into_owned(),
            run_id: run.id.clone(),
            workspace: workspace.to_string_lossy().into_owned(),
        })?,
    )?;
    Ok(())
}
pub(crate) fn sync(db: &Db, run: &EvaluationResult) -> io::Result<()> {
    let mut next = if run.state == "waiting_human" {
        "waiting_human"
    } else if run.state == "started" {
        "running"
    } else {
        "ended"
    };
    if next == "ended" {
        use rusqlite::OptionalExtension;
        let stopping: Option<bool> = db
            .conn()
            .query_row(
                "SELECT state='stopping' FROM evaluation_controls WHERE run_id=?1",
                [&run.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if stopping == Some(true) && read(db, &run.id)?.cleanup_confirmed {
            next = "interrupted";
        }
    }
    db.conn().execute("UPDATE evaluation_controls SET state=CASE WHEN ?2='interrupted' THEN ?2 WHEN state IN ('stopping','interrupted') THEN state ELSE ?2 END,automatic_ms=?3,phase_started_ms=CASE WHEN ?2='running' THEN CASE WHEN state='running' THEN COALESCE(phase_started_ms,?4) ELSE ?4 END ELSE NULL END WHERE run_id=?1",params![run.id,next,i64::try_from(run.elapsed_ms).map_err(err)?,i64::try_from(now_ms()?).map_err(err)?]).map_err(err)?;
    if next == "interrupted" {
        db.conn().execute("UPDATE evaluation_controls SET cleanup_ended_ms=COALESCE(cleanup_ended_ms,?2) WHERE run_id=?1",params![run.id,i64::try_from(now_ms()?).map_err(err)?]).map_err(err)?;
    }
    Ok(())
}
pub(crate) fn read(db: &Db, id: &str) -> io::Result<EvaluationControl> {
    let (state,reason,limit,automatic,since):(String,Option<String>,u64,u64,Option<i64>)=db.conn().query_row("SELECT state,reason,active_limit_ms,automatic_ms,phase_started_ms FROM evaluation_controls WHERE run_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,num(r,2)?,num(r,3)?,r.get(4)?))).map_err(err)?;
    let state: EvaluationControlState = serde_json::from_value(serde_json::Value::String(state))?;
    let now = now_ms()?;
    let (human,backwards):(u64,bool)=db.conn().query_row("SELECT COALESCE(SUM(CASE WHEN ended_at_ms IS NULL THEN MAX(0,?2-started_at_ms) ELSE COALESCE(duration_ms,0) END),0),COALESCE(MAX(ended_at_ms IS NULL AND started_at_ms>?2),0) FROM evaluation_human_intervals WHERE run_id=?1",params![id,i64::try_from(now).map_err(err)?],|r|Ok((num(r,0)?,r.get(1)?))).map_err(err)?;
    if backwards {
        return Err(err("attention clock moved backwards"));
    }
    let phase = if state == EvaluationControlState::Running {
        let start =
            u64::try_from(since.ok_or_else(|| err("active clock missing"))?).map_err(err)?;
        now.checked_sub(start)
            .ok_or_else(|| err("active clock moved backwards"))?
    } else {
        0
    };
    let measured_active = automatic
        .checked_add(human)
        .and_then(|n| n.checked_add(phase))
        .ok_or_else(|| err("active clock overflow"))?;
    let (frozen, stopped, cleaned): (Option<u64>, Option<u64>, Option<u64>) = db.conn().query_row(
        "SELECT active_frozen_ms,stopped_at_ms,cleanup_ended_ms FROM evaluation_controls WHERE run_id=?1",[id],|r|Ok((optional_num(r,0)?,optional_num(r,1)?,optional_num(r,2)?))).map_err(err)?;
    let active = frozen.unwrap_or(measured_active);
    let incomplete:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_human_intervals WHERE run_id=?1 AND ended_at_ms IS NOT NULL AND duration_ms IS NULL)",[id],|r|r.get(0)).map_err(err)?;
    let recovery_unknown: bool = db
        .conn()
        .query_row(
            "SELECT recovery_unknown FROM evaluation_controls WHERE run_id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let run = super::read(db, id)?;
    let worker = rusqlite::Connection::open_with_flags(
        Path::new(&run.workspace).join(".hexagon/state.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(err)?;
    let (pending,unknown):(u64,u64)=worker.query_row("SELECT COALESCE(SUM(request_state='pending'),0),COALESCE(SUM(request_state IN ('outcome_unknown','interrupted')),0) FROM usage WHERE record_kind='request'",[],|r|Ok((num(r,0)?,num(r,1)?))).map_err(err)?;
    Ok(EvaluationControl {
        run_id: id.into(),
        state,
        reason,
        active_ms: active,
        active_limit_ms: limit,
        pending_requests: pending,
        unknown_requests: unknown,
        remote_cancellation_confirmed: false,
        stopped_at_ms: stopped,
        cleanup_elapsed_ms: stopped.map(|start| cleaned.unwrap_or(now).saturating_sub(start)),
        active_time_complete: !incomplete && !recovery_unknown,
        cleanup_confirmed: !recovery_unknown
            && matches!(
                state,
                EvaluationControlState::Stopping
                    | EvaluationControlState::Interrupted
                    | EvaluationControlState::Ended
            )
            && pending == 0
            && quiescent(Path::new(&run.workspace), &worker),
    })
}
pub(crate) fn stop(db: &Db, id: &str) -> io::Result<EvaluationControl> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let mut run = super::read(db, id)?;
    halt(db, id, "owner_stopped")?;
    if run.state == "waiting_human" && read(db, id)?.cleanup_confirmed {
        run.state = "incomplete".into();
        run.flow_completed = false;
        run.error = Some("owner_stopped".into());
        super::update_started(db, &run)?;
        use rusqlite::OptionalExtension;
        let plan:Option<(String,i64)>=db.conn().query_row("SELECT plan_id,position FROM evaluation_plan_runs WHERE run_id=?1 AND state='started'",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?;
        if let Some((plan, position)) = plan {
            super::plan::finish(db, &plan, usize::try_from(position).map_err(err)?, id)?;
        }

        db.conn().execute("UPDATE evaluation_controls SET state='interrupted',phase_started_ms=NULL WHERE run_id=?1 AND state='stopping'",[id]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    read(db, id)
}

// D09: pause only at an idle owner boundary. Active work requires stop;
// pause is not a claim that an in-flight remote request was cancelled.
pub(crate) fn pause(db: &Db, id: &str, paused: bool) -> io::Result<EvaluationControl> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let current = read(db, id)?;
    let expected = if paused {
        EvaluationControlState::WaitingHuman
    } else {
        EvaluationControlState::Paused
    };
    let open:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_human_intervals WHERE run_id=?1 AND ended_at_ms IS NULL)",[id],|r|r.get(0)).map_err(err)?;
    if current.state != expected || current.pending_requests != 0 || open {
        return Err(err("pause/resume requires an idle owner boundary"));
    }
    let next = if paused { "paused" } else { "waiting_human" };
    db.conn()
        .execute(
            "UPDATE evaluation_controls SET state=?2 WHERE run_id=?1",
            params![id, next],
        )
        .map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_control",
        &format!("{next}:{id}"),
        std::time::Instant::now(),
    );
    tx.commit().map_err(err)?;
    read(db, id)
}

// D09: holding this shared OS lease proves live evaluation work even when
// no provider request is pending. A stop flag or kill signal is not exit proof.
pub(crate) fn work_lease(root: &Path) -> io::Result<Option<std::fs::File>> {
    if is_probe(root) || binding(root)?.is_none() {
        return Ok(None);
    }
    let path = root.join(".hexagon/evaluation-work.lock");
    if path.symlink_metadata()?.is_symlink() {
        return Err(err("work lease is an alias"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    file.try_lock_shared().map_err(err)?;
    checkpoint(root)?;
    Ok(Some(file))
}
fn quiescent(root: &Path, worker: &rusqlite::Connection) -> bool {
    let unresolved=worker.query_row("SELECT EXISTS(SELECT 1 FROM tool_actions WHERE state IN ('executing','unknown') AND NOT EXISTS(SELECT 1 FROM action_resolutions WHERE action_resolutions.action_id=tool_actions.id))",[],|r|r.get::<_,bool>(0));
    if !matches!(unresolved, Ok(false)) {
        return false;
    }
    idle_leases(root).is_ok()
}
/// D10: keep exclusion while reconciling. A momentary liveness check would
/// race a newly dispatched worker between inspection and the stop transition.
pub(crate) fn idle_leases(root: &Path) -> io::Result<Vec<std::fs::File>> {
    let mut locks = Vec::new();
    for (name, required) in [
        ("evaluation-work.lock", true),
        ("write-locks/repository.lock", false),
    ] {
        let path = root.join(".hexagon").join(name);
        match path.symlink_metadata() {
            Err(e) if e.kind() == io::ErrorKind::NotFound && !required => continue,
            Ok(meta) if !meta.is_symlink() => (),
            _ => return Err(err("execution lease missing or invalid")),
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        file.try_lock().map_err(err)?;
        locks.push(file);
    }
    Ok(locks)
}

// D05/D09: fixed host boundary probes are not task runs. Their authority is
// an in-process lease, never a worker-editable marker or a fake success record.
static PROBES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::time::Instant>>,
> = std::sync::OnceLock::new();
pub(crate) struct ProbeLease(std::path::PathBuf);
impl Drop for ProbeLease {
    fn drop(&mut self) {
        if let Some(probes) = PROBES.get() {
            probes.lock().unwrap().remove(&self.0);
        }
    }
}
pub(crate) fn probe(root: &Path) -> io::Result<ProbeLease> {
    let root = root.canonicalize()?;
    PROBES.get_or_init(Default::default).lock().unwrap().insert(
        root.clone(),
        std::time::Instant::now() + std::time::Duration::from_secs(180),
    );
    Ok(ProbeLease(root))
}
fn is_probe(root: &Path) -> bool {
    let Some(probes) = PROBES.get() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    probes
        .lock()
        .unwrap()
        .get(&root)
        .is_some_and(|until| std::time::Instant::now() < *until)
}

fn binding(root: &Path) -> io::Result<Option<Binding>> {
    if !root.join(".hexagon/evaluation-worker").exists() {
        return Ok(None);
    }
    let path = root.join(".hexagon/evaluation-control.json");
    if path.symlink_metadata()?.is_symlink() {
        return Err(err("evaluation control binding is a symlink"));
    }
    let binding: Binding = serde_json::from_slice(&std::fs::read(path)?)?;
    if root.canonicalize()?.to_string_lossy() != binding.workspace
        || Path::new(&binding.host).canonicalize()?.to_string_lossy() != binding.host
    {
        return Err(err("evaluation control identity mismatch"));
    }
    Ok(Some(binding))
}
pub(crate) fn validate_owner(host: &Path, run: &EvaluationResult) -> io::Result<()> {
    let worker = Path::new(&run.workspace);
    let b = binding(worker)?.ok_or_else(|| err("evaluation worker binding missing"))?;
    if b.run_id != run.id || Path::new(&b.host) != host.canonicalize()? {
        return Err(err("evaluation worker owner mismatch"));
    }
    if !worker
        .canonicalize()?
        .starts_with(host.canonicalize()?.join(".hexagon/evaluation-runs"))
    {
        return Err(err("evaluation workspace outside host"));
    }
    Ok(())
}
fn halt(db: &Db, id: &str, reason: &str) -> io::Result<()> {
    let current = read(db, id)?;
    let now = i64::try_from(now_ms()?).map_err(err)?;
    let changed=db.conn().execute("UPDATE evaluation_controls SET state='stopping',reason=?2,active_frozen_ms=?3,stopped_at_ms=?4,phase_started_ms=NULL WHERE run_id=?1 AND state IN ('running','waiting_human','paused')",params![id,reason,i64::try_from(current.active_ms).map_err(err)?,now]).map_err(err)?;
    if changed > 0 {
        // D09: the stop caller does not own the monotonic attention handle.
        // Close the lease, retain unknown human duration, never invent savings.
        db.conn().execute("UPDATE evaluation_human_intervals SET ended_at_ms=?2,end_reason='interrupted',duration_ms=NULL WHERE run_id=?1 AND ended_at_ms IS NULL",params![id,now]).map_err(err)?;
        db.conn().execute("UPDATE evaluation_cursors SET ended_at_ms=COALESCE(ended_at_ms,?2) WHERE run_id=?1",params![id,now]).map_err(err)?;

        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_stop",
            &format!("{reason}:{id}"),
            std::time::Instant::now(),
        );
    }
    Ok(())
}
pub(crate) fn checkpoint(root: &Path) -> io::Result<()> {
    let started = std::time::Instant::now();
    let result = checkpoint_inner(root);
    if result.is_err() {
        let id = binding(root)
            .ok()
            .flatten()
            .map(|b| b.run_id)
            .unwrap_or_else(|| "unbound".into());
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_execution_gate",
            &format!("control_refused:{id}"),
            started,
        );
    }
    result
}
fn checkpoint_inner(root: &Path) -> io::Result<()> {
    if is_probe(root) {
        return Ok(());
    }
    let Some(binding) = binding(root)? else {
        return Ok(());
    };
    let db = Db::open_current(Path::new(&binding.host).join(".hexagon/state.db")).map_err(err)?;
    let run = super::read(&db, &binding.run_id)?;
    if Path::new(&run.workspace).canonicalize()?.to_string_lossy() != binding.workspace {
        return Err(err("evaluation worker no longer current"));
    }
    let control = read(&db, &binding.run_id)?;
    if control.state != EvaluationControlState::Running {
        return Err(err(format!("evaluation is {:?}", control.state)));
    }
    let reason = if active_limit_reached(control.active_ms, control.active_limit_ms) {
        Some("active_time_limit")
    } else {
        super::budget::stop_reason(root)?
    };
    if let Some(reason) = reason {
        halt(&db, &binding.run_id, reason)?;
        return Err(err(reason));
    }
    Ok(())
}
pub(crate) fn budget_refused(root: &Path) -> io::Result<()> {
    if let Some(binding) = binding(root)? {
        let db =
            Db::open_current(Path::new(&binding.host).join(".hexagon/state.db")).map_err(err)?;
        halt(&db, &binding.run_id, "budget_admission_refused")?;
    }
    Ok(())
}
pub(crate) fn apply_stop(db: &Db, run: &mut EvaluationResult) -> io::Result<()> {
    use rusqlite::OptionalExtension;
    let reason:Option<Option<String>>=db.conn().query_row("SELECT reason FROM evaluation_controls WHERE run_id=?1 AND state IN ('stopping','interrupted')",[&run.id],|r|r.get(0)).optional().map_err(err)?;
    if let Some(reason) = reason {
        run.state = "incomplete".into();
        run.flow_completed = false;
        run.error = reason.or_else(|| Some("evaluation_stopped".into()));
    }
    Ok(())
}

/// Independent of provider deltas: silent calls must still stop owned children.
pub(crate) struct Watch {
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        // OS cleanup can outlive its caller. Never turn cancellation into an
        // unbounded join; durable pending/unknown facts continue to describe it.
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
pub(crate) fn watch(
    root: &Path,
    sessions: crate::sessions::SessionTable,
    tasks: crate::subagent::TaskBoard,
) -> io::Result<Option<Watch>> {
    if is_probe(root) || binding(root)?.is_none() {
        return Ok(None);
    }
    let root = root.to_path_buf();
    let (stop, receiver) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("evaluation-stop-watch".into())
        .spawn(move || {
            while matches!(
                receiver.recv_timeout(std::time::Duration::from_millis(100)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                if checkpoint_inner(&root).is_err() {
                    tasks.halt_all();
                    sessions.kill_all();
                }
            }
        })?;
    Ok(Some(Watch {
        stop,
        thread: Some(thread),
    }))
}

// D09/D12: a false positive delays a task; a false negative admits work beyond
// the owner's limit. Equality and missing/zero limits fail closed.
fn active_limit_reached(active: u64, limit: u64) -> bool {
    limit == 0 || active >= limit
}
pub(crate) fn abandon(db: &Db, run: &EvaluationResult) -> io::Result<()> {
    db.conn().execute("UPDATE evaluation_controls SET state='stopping',reason='execution_owner_lost',recovery_unknown=1,active_frozen_ms=?2,phase_started_ms=NULL,stopped_at_ms=COALESCE(stopped_at_ms,?3) WHERE run_id=?1 AND state IN ('running','stopping')",params![run.id,i64::try_from(run.elapsed_ms).map_err(err)?,i64::try_from(now_ms()?).map_err(err)?]).map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_recovery",
        &format!("execution_owner_lost:{}", run.id),
        std::time::Instant::now(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn reaching_active_limit_never_reopens_with_more_time(active in any::<u64>(),limit in any::<u64>(),extra in any::<u64>()) {
            if active_limit_reached(active,limit) {prop_assert!(active_limit_reached(active.saturating_add(extra),limit));}
            prop_assert!(active_limit_reached(limit,limit));
        }
    }
}
