//! SessionHost: the durable owner of one project's current task chain.
//!
//! This module owns lifecycle validation and persistence. It deliberately does
//! not run tools or grant permissions; callers re-enter those existing seams.

use crate::db::Db;
use crate::trace::EventKind;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum SessionStatus {
    Created,
    Running,
    WaitingUser,
    Paused,
    Reconnecting,
    Completed,
    Failed,
}

impl SessionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::WaitingUser => "waiting_user",
            Self::Paused => "paused",
            Self::Reconnecting => "reconnecting",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum CapabilityHealth {
    Healthy,
    Degraded,
    Unavailable,
    Reconnecting,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct WorktreeAttachment {
    pub path: String,
    pub baseline: String,
    /// The baseline commit observed when the physical worktree was attached.
    /// Optional for rows written before P1-1; physical callers must populate it.
    #[serde(default)]
    pub baseline_commit: Option<String>,
    pub created_by_session: bool,
    pub cleanup_requested: bool,
}

/// Physical worktree operation approved by one `worktree` confirmation card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum WorktreeOperation {
    Create,
    Cleanup,
}

impl WorktreeOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Cleanup => "cleanup",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "cleanup" => Some(Self::Cleanup),
            _ => None,
        }
    }

    /// The proof operation the git seam accepts for this card.
    pub(crate) fn proof_operation(self) -> &'static str {
        match self {
            Self::Create => "worktree_create",
            Self::Cleanup => "worktree_cleanup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum WorktreeIntentState {
    /// Written immediately before the git call. Seen by anyone other than the
    /// confirming call itself, it means the outcome is unknown.
    Executing,
    Failed,
}

/// Durable intent for one approved physical worktree operation (Wayfinder-07
/// Addendum A). The card id is the action identity; the intent survives a
/// crash between git and metadata so a read-only verify can settle it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct WorktreeIntent {
    pub operation: WorktreeOperation,
    pub question_id: String,
    pub path: String,
    pub baseline_commit: String,
    pub state: WorktreeIntentState,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RuntimeAttachment {
    pub kind: String,
    pub binding_hint: String,
    pub health: CapabilityHealth,
    pub last_error: Option<String>,
    pub last_checked_at: Option<String>,
    #[ts(type = "number")]
    pub restart_attempts: u64,
    /// Read-only adapter facts observed by the owner beat. Older rows omit
    /// this field and therefore keep the read lane's `unknown` default.
    #[serde(default)]
    pub capabilities: Vec<crate::capabilities::CapabilityStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SessionRecord {
    pub session_id: String,
    pub project_id: String,
    pub project_root: String,
    pub baseline: String,
    pub status: SessionStatus,
    pub current_turn_id: Option<String>,
    pub worktree: Option<WorktreeAttachment>,
    #[serde(default)]
    pub worktree_intent: Option<WorktreeIntent>,
    pub runtime: Option<RuntimeAttachment>,
    #[ts(type = "number")]
    pub reconnect_attempts: u64,
    #[ts(type = "number")]
    pub reconnect_budget: u64,
    pub is_current: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_branch: Option<String>,
    pub last_reason_code: Option<String>,
    #[ts(type = "number")]
    pub last_elapsed_ms: u64,
}

/// Read-only recovery projection assembled by the shell from the existing
/// cards, diagnostics and capability probes. The constructor intentionally
/// performs no I/O or permission decisions.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SessionSummary {
    pub session: Option<SessionRecord>,
    pub recovery_run_id: Option<String>,
    #[ts(type = "number")]
    pub pending_count: u64,
    pub highest_pending_severity: Option<String>,
    pub recent_diagnostics: Vec<crate::diag::DiagRecord>,
    pub capabilities: Vec<crate::capabilities::CapabilityStatus>,
    pub triggers: Vec<OneTimeTrigger>,
    pub next_action: Option<String>,
}

/// A one-shot, user-visible continuation request. It is metadata only: consuming
/// it never invokes a runtime adapter; callers re-enter the normal turn/grant path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum OneTimeTriggerState {
    Scheduled,
    Expired,
    Consumed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct OneTimeTrigger {
    pub trigger_id: String,
    pub session_id: String,
    pub project_root: String,
    pub baseline: String,
    pub model_slot: String,
    pub permission_package: Vec<String>,
    #[ts(type = "number")]
    pub fire_at: u64,
    pub trace_id: String,
    pub state: OneTimeTriggerState,
    pub created_at: String,
    pub consumed_at: Option<String>,
}

/// Proof produced by the Workbench after its existing approval/grant seams
/// have been checked. Tauri cannot manufacture this token from IPC input.
#[derive(Debug, Clone)]
pub struct PermissionRecheck {
    operation: &'static str,
    binding: Option<WorktreeProofBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeProofBinding {
    question_id: String,
    session_id: String,
    project_root: String,
    worktree_path: String,
    baseline_commit: String,
}

impl PermissionRecheck {
    #[cfg(test)]
    pub(crate) fn confirmed() -> Self {
        Self {
            operation: "generic",
            binding: None,
        }
    }

    pub(crate) fn confirmed_for(operation: &'static str) -> Self {
        Self {
            operation,
            binding: None,
        }
    }

    fn permits(&self, operation: &str) -> bool {
        self.operation == "generic" || self.operation == operation
    }

    /// Minted only by the `worktree` card confirmation after it rechecked
    /// every bound fact and answered the card. `question_id` is the action
    /// identity the trace and the git seam carry.
    pub(crate) fn confirmed_for_worktree(
        operation: &'static str,
        question_id: &str,
        session_id: &str,
        project_root: &str,
        worktree_path: &str,
        baseline_commit: &str,
    ) -> Self {
        Self {
            operation,
            binding: Some(WorktreeProofBinding {
                question_id: question_id.to_owned(),
                session_id: session_id.to_owned(),
                project_root: project_root.to_owned(),
                worktree_path: worktree_path.to_owned(),
                baseline_commit: baseline_commit.to_owned(),
            }),
        }
    }

    pub(crate) fn permits_worktree(
        &self,
        operation: &str,
        session_id: &str,
        project_root: &str,
        worktree_path: &str,
        baseline_commit: &str,
    ) -> bool {
        self.operation == operation
            && self.binding.as_ref().is_some_and(|binding| {
                !binding.question_id.is_empty()
                    && binding.session_id == session_id
                    && binding.project_root == project_root
                    && binding.worktree_path == worktree_path
                    && binding.baseline_commit == baseline_commit
            })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn summary(
    session: Option<SessionRecord>,
    recovery_run_id: Option<String>,
    pending_count: u64,
    highest_pending_severity: Option<String>,
    recent_diagnostics: Vec<crate::diag::DiagRecord>,
    capabilities: Vec<crate::capabilities::CapabilityStatus>,
    triggers: Vec<OneTimeTrigger>,
    next_action: Option<String>,
) -> SessionSummary {
    SessionSummary {
        session,
        recovery_run_id,
        pending_count,
        highest_pending_severity,
        recent_diagnostics,
        capabilities,
        triggers,
        next_action,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Start,
    WaitUser,
    Approve,
    Resume,
    Pause,
    ProcessExit,
    RuntimeFailed,
    Reconnected,
    ReconnectExhausted,
    Complete,
    Fail,
    Reconnect,
}

impl Transition {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::WaitUser => "wait_user",
            Self::Approve => "approve",
            Self::Resume => "resume",
            Self::Pause => "pause",
            Self::ProcessExit => "process_exit",
            Self::RuntimeFailed => "runtime_failed",
            Self::Reconnected => "reconnected",
            Self::ReconnectExhausted => "reconnect_exhausted",
            Self::Complete => "complete",
            Self::Fail => "fail",
            Self::Reconnect => "reconnect",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("trace: {0}")]
    Trace(String),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("invalid session transition {from:?} -> {action}: {code}")]
    InvalidTransition {
        from: SessionStatus,
        action: &'static str,
        code: &'static str,
    },
    #[error("session binding mismatch: {0}")]
    BindingMismatch(String),
    #[error("corrupt session state: {0}")]
    CorruptState(String),
    #[error("session value out of sqlite range: {0}")]
    InvalidValue(String),
    #[error("permission recheck is not bound to operation: {0}")]
    PermissionRecheckMismatch(String),
    #[error("one-time trigger conflict: {0}")]
    TriggerConflict(String),
    #[error("one-time trigger is not ready: {0}")]
    TriggerNotReady(String),
    #[error("one-time trigger is cancelled: {0}")]
    TriggerCancelled(String),
    /// Cancel arrived after the trigger came due (expired) or was consumed:
    /// the continuation now belongs to the beat / the owner's Continue, so
    /// cancel refuses instead of pretending to retract it (gap D1).
    #[error("one-time trigger already fired: {0}")]
    TriggerAlreadyFired(String),
    /// A permission or grant card is still waiting for the owner; resume and
    /// trigger continuation answer it first (code `decision_pending`).
    #[error("a permission or grant decision is still pending")]
    DecisionPending,
}

fn now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    secs.to_string()
}

fn new_id(db: &Db) -> Result<String, SessionError> {
    // All project databases use p1; a per-db counter would mix host diagnostics
    // from unrelated projects (SessionHost P0-1, 2026-10-12).
    Ok(db.conn().query_row(
        "SELECT 'session-' || lower(hex(randomblob(16)))",
        [],
        |row| row.get(0),
    )?)
}

fn sqlite_i64(value: u64, field: &str) -> Result<i64, SessionError> {
    i64::try_from(value)
        .map_err(|_| SessionError::InvalidValue(format!("{field} exceeds sqlite integer range")))
}

impl From<crate::db::DbError> for SessionError {
    fn from(value: crate::db::DbError) -> Self {
        Self::Sqlite(match value {
            crate::db::DbError::Sqlite(e) => e,
        })
    }
}

pub fn create(
    db: &Db,
    project_id: &str,
    project_root: &str,
    baseline: &str,
    reconnect_budget: u64,
) -> Result<SessionRecord, SessionError> {
    // Db owns a shared Connection; IMMEDIATE serializes current-session
    // selection with other project handles and rejects nested transactions.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let record = insert_session(db, project_id, project_root, baseline, reconnect_budget)?;
    tx.commit()?;
    Ok(record)
}

fn insert_session(
    db: &Db,
    project_id: &str,
    project_root: &str,
    baseline: &str,
    reconnect_budget: u64,
) -> Result<SessionRecord, SessionError> {
    let id = new_id(db)?;
    let timestamp = now();
    let reconnect_budget = sqlite_i64(reconnect_budget, "reconnect_budget")?;
    db.conn().execute(
        "UPDATE session_hosts SET is_current=0, updated_at=?2 WHERE project_id=?1 AND is_current=1",
        rusqlite::params![project_id, timestamp],
    )?;
    db.conn().execute(
        "INSERT INTO session_hosts(session_id,project_id,project_root,baseline,status,reconnect_budget,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'created',?5,?6,?6)",
        rusqlite::params![id, project_id, project_root, baseline, reconnect_budget, timestamp],
    )?;
    let record = get(db, &id)?;
    emit(
        db,
        project_id,
        &record,
        "session_created",
        "session_create",
        "created",
        0,
    )?;
    Ok(record)
}

/// Reuse the current session only when its project binding still matches.
pub fn ensure_current(
    db: &Db,
    project_id: &str,
    project_root: &str,
    baseline: &str,
    reconnect_budget: u64,
) -> Result<SessionRecord, SessionError> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    if let Some(existing) = current(db, project_id)? {
        if existing.project_root != project_root || existing.baseline != baseline {
            tx.commit()?;
            emit_rejection(db, &existing, "ensure_current", "binding_mismatch");
            return Err(SessionError::BindingMismatch(format!(
                "session {} is bound to a different project root or baseline",
                existing.session_id
            )));
        }
        // A completed chain is the only terminal state that admits a new task.
        // A failed chain remains the durable recovery boundary; silently
        // replacing it would let a later turn bypass the explicit reconnect /
        // permission gate (P0-1, 2026-10-12).
        if existing.status == SessionStatus::Completed {
            let next = insert_session(db, project_id, project_root, baseline, reconnect_budget)?;
            tx.commit()?;
            return Ok(next);
        }
        tx.commit()?;
        return Ok(existing);
    }
    let next = insert_session(db, project_id, project_root, baseline, reconnect_budget)?;
    tx.commit()?;
    Ok(next)
}

pub fn current(db: &Db, project_id: &str) -> Result<Option<SessionRecord>, SessionError> {
    let mut stmt = db.conn().prepare(
        "SELECT session_id FROM session_hosts WHERE project_id=?1 AND is_current=1 LIMIT 1",
    )?;
    let id = stmt
        .query_row([project_id], |row| row.get::<_, String>(0))
        .optional()?;
    id.map(|id| get(db, &id)).transpose()
}

pub fn get(db: &Db, session_id: &str) -> Result<SessionRecord, SessionError> {
    let row = db
        .conn()
        .query_row(
            "SELECT session_id,project_id,project_root,baseline,status,current_turn_id,worktree_json,runtime_json,
                    reconnect_attempts,reconnect_budget,is_current,created_at,updated_at,last_branch,last_reason_code,last_elapsed_ms,
                    worktree_intent_json
             FROM session_hosts WHERE session_id=?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, Option<String>>(16)?,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SessionError::NotFound(session_id.into()),
            other => SessionError::Sqlite(other),
        })?;
    let status = parse_status(&row.4)
        .map_err(|_| SessionError::CorruptState(format!("unknown status for {session_id}")))?;
    let worktree = row
        .6
        .map(|value| {
            serde_json::from_str(&value).map_err(|e| SessionError::CorruptState(e.to_string()))
        })
        .transpose()?;
    let runtime = row
        .7
        .map(|value| {
            serde_json::from_str(&value).map_err(|e| SessionError::CorruptState(e.to_string()))
        })
        .transpose()?;
    let worktree_intent = row
        .16
        .map(|value| {
            serde_json::from_str(&value).map_err(|e| SessionError::CorruptState(e.to_string()))
        })
        .transpose()?;
    Ok(SessionRecord {
        session_id: row.0,
        project_id: row.1,
        project_root: row.2,
        baseline: row.3,
        status,
        current_turn_id: row.5,
        worktree,
        worktree_intent,
        runtime,
        reconnect_attempts: row.8.max(0) as u64,
        reconnect_budget: row.9.max(0) as u64,
        is_current: row.10 != 0,
        created_at: row.11,
        updated_at: row.12,
        last_branch: row.13,
        last_reason_code: row.14,
        last_elapsed_ms: row.15.max(0) as u64,
    })
}

pub fn bind_turn(db: &Db, session_id: &str, turn_id: &str) -> Result<SessionRecord, SessionError> {
    let record = get(db, session_id)?;
    if record.status.terminal() {
        emit_rejection(db, &record, "bind_turn", "terminal_session");
        return Err(SessionError::InvalidTransition {
            from: record.status,
            action: "bind_turn",
            code: "terminal_session",
        });
    }
    db.conn().execute(
        "UPDATE session_hosts SET current_turn_id=?2,updated_at=?3 WHERE session_id=?1",
        rusqlite::params![session_id, turn_id, now()],
    )?;
    get(db, session_id)
}

/// Persist one observer beat from the existing host/watch owner. A beat only
/// records observed health; it never retries a provider or external side effect.
pub fn runtime_beat(
    db: &Db,
    session_id: &str,
    kind: &str,
    health: CapabilityHealth,
    last_error: Option<&str>,
    restart_attempts: u64,
) -> Result<SessionRecord, SessionError> {
    runtime_beat_with_capabilities(
        db,
        session_id,
        kind,
        health,
        last_error,
        restart_attempts,
        Vec::new(),
    )
}

pub fn runtime_beat_with_capabilities(
    db: &Db,
    session_id: &str,
    kind: &str,
    health: CapabilityHealth,
    last_error: Option<&str>,
    restart_attempts: u64,
    capabilities: Vec<crate::capabilities::CapabilityStatus>,
) -> Result<SessionRecord, SessionError> {
    let record = get(db, session_id)?;
    let runtime = RuntimeAttachment {
        kind: kind.to_owned(),
        binding_hint: format!("session:{}", record.session_id),
        health,
        last_error: last_error.map(str::to_owned),
        last_checked_at: Some(now()),
        restart_attempts,
        capabilities,
    };
    let encoded = serde_json::to_string(&runtime)
        .map_err(|error| SessionError::CorruptState(error.to_string()))?;
    // Re-audit 2026-10-13: the beat used to overwrite the session's own
    // last_reason_code (hiding e.g. reconnect_budget_exhausted) and append a
    // trace event on every periodic beat. Health lives in runtime_json; only
    // a change of observed facts is a state change worth a record.
    let changed = record.runtime.as_ref().is_none_or(|previous| {
        previous.health != runtime.health
            || previous.last_error != runtime.last_error
            || previous.capabilities != runtime.capabilities
    });
    db.conn().execute(
        "UPDATE session_hosts SET runtime_json=?2,updated_at=?3 WHERE session_id=?1",
        rusqlite::params![session_id, encoded, now()],
    )?;
    let updated = get(db, session_id)?;
    if changed {
        emit(
            db,
            &updated.project_id,
            &updated,
            "runtime_health",
            "runtime_health",
            last_error.unwrap_or("runtime_healthy"),
            0,
        )?;
    }
    Ok(updated)
}

pub fn resume(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    elapsed_ms: u64,
    permission_recheck: PermissionRecheck,
) -> Result<SessionRecord, SessionError> {
    if !permission_recheck.permits("resume") {
        return Err(SessionError::PermissionRecheckMismatch("resume".into()));
    }
    let record = validate_binding(db, session_id, project_root, baseline, "resume")?;
    match record.status {
        SessionStatus::Running => Ok(record),
        SessionStatus::Paused => transition(db, session_id, Transition::Resume, elapsed_ms),
        status => {
            let code = if status.terminal() {
                "terminal_session"
            } else {
                "transition_not_allowed"
            };
            emit_rejection(db, &record, "resume", code);
            Err(SessionError::InvalidTransition {
                from: status,
                action: "resume",
                code,
            })
        }
    }
}

pub fn validate_binding(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    action: &str,
) -> Result<SessionRecord, SessionError> {
    let record = get(db, session_id)?;
    if record.project_root != project_root || record.baseline != baseline {
        emit_rejection(db, &record, action, "binding_mismatch");
        return Err(SessionError::BindingMismatch(format!(
            "session {session_id} binding changed"
        )));
    }
    Ok(record)
}

pub fn reconnect(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    elapsed_ms: u64,
) -> Result<SessionRecord, SessionError> {
    validate_binding(db, session_id, project_root, baseline, "reconnect")?;
    transition(db, session_id, Transition::Reconnect, elapsed_ms)
}

fn parse_status(status: &str) -> Result<SessionStatus, ()> {
    Ok(match status {
        "created" => SessionStatus::Created,
        "running" => SessionStatus::Running,
        "waiting_user" => SessionStatus::WaitingUser,
        "paused" => SessionStatus::Paused,
        "reconnecting" => SessionStatus::Reconnecting,
        "completed" => SessionStatus::Completed,
        "failed" => SessionStatus::Failed,
        _ => return Err(()),
    })
}

pub fn transition(
    db: &Db,
    session_id: &str,
    action: Transition,
    elapsed_ms: u64,
) -> Result<SessionRecord, SessionError> {
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let before = get(db, session_id)?;
    let (next, code) = match (before.status, action) {
        (SessionStatus::Created, Transition::Start) => (SessionStatus::Running, "session_started"),
        (SessionStatus::Running, Transition::WaitUser)
        // A model request that failed while the network was suspended waits
        // for its recovery item like any other owner decision.
        | (SessionStatus::Reconnecting, Transition::WaitUser) => {
            (SessionStatus::WaitingUser, "needs_user")
        }
        (SessionStatus::WaitingUser, Transition::Approve) => {
            (SessionStatus::Running, "user_approved")
        }
        // A denied permission may still be a valid task observation. The
        // continuation re-enters the same activation after the card is
        // answered; its eventual TurnOutcome decides whether the task fails.
        (SessionStatus::WaitingUser, Transition::Resume) => {
            (SessionStatus::Running, "user_decision_recorded")
        }
        (SessionStatus::WaitingUser, Transition::Fail) => {
            (SessionStatus::Failed, "approval_rejected")
        }
        (SessionStatus::Running, Transition::Pause) => (SessionStatus::Paused, "owner_paused"),
        // Process exit is durable and distinct from a user closing a window:
        // the host is paused for explicit reopen, while the owned workers are
        // stopped by the shell exit boundary.
        (SessionStatus::Running, Transition::ProcessExit)
        | (SessionStatus::WaitingUser, Transition::ProcessExit)
        | (SessionStatus::Reconnecting, Transition::ProcessExit) => {
            (SessionStatus::Paused, "process_exit")
        }
        (SessionStatus::WaitingUser, Transition::Pause)
        | (SessionStatus::Reconnecting, Transition::Pause) => {
            (SessionStatus::Paused, "owner_paused")
        }
        (SessionStatus::Paused, Transition::Resume) => (SessionStatus::Running, "owner_resumed"),
        (SessionStatus::Paused, Transition::Fail) => (SessionStatus::Failed, "binding_invalid"),
        (SessionStatus::Running, Transition::RuntimeFailed) => {
            let attempts = before.reconnect_attempts.saturating_add(1);
            if attempts >= before.reconnect_budget {
                (SessionStatus::Failed, "reconnect_budget_exhausted")
            } else {
                (SessionStatus::Reconnecting, "runtime_unhealthy")
            }
        }
        (SessionStatus::Reconnecting, Transition::Reconnected) => {
            (SessionStatus::Running, "health_restored")
        }
        // A retry may fail while the host is already reconnecting. Keep the
        // same identity and consume one budget slot; the API promotes the
        // final slot to ReconnectExhausted so that action is observable.
        (SessionStatus::Reconnecting, Transition::RuntimeFailed) => {
            let attempts = before.reconnect_attempts.saturating_add(1);
            if attempts >= before.reconnect_budget {
                (SessionStatus::Failed, "reconnect_budget_exhausted")
            } else {
                (SessionStatus::Reconnecting, "runtime_unhealthy")
            }
        }
        (SessionStatus::Reconnecting, Transition::ReconnectExhausted)
            if before.reconnect_attempts.saturating_add(1) >= before.reconnect_budget =>
        {
            (SessionStatus::Failed, "reconnect_budget_exhausted")
        }
        (SessionStatus::Running, Transition::Complete) => {
            (SessionStatus::Completed, "turn_completed")
        }
        (SessionStatus::Running, Transition::Fail) => (SessionStatus::Failed, "turn_failed"),
        (SessionStatus::Failed, Transition::Reconnect)
            if before.last_reason_code.as_deref() != Some("reconnect_budget_exhausted")
                && before.reconnect_attempts < before.reconnect_budget =>
        {
            (SessionStatus::Reconnecting, "user_requested_reconnect")
        }
        (from, action) => {
            let code = if from.terminal() {
                "terminal_session"
            } else if matches!(action, Transition::ReconnectExhausted) {
                "reconnect_budget_not_exhausted"
            } else {
                "transition_not_allowed"
            };
            tx.commit()?;
            emit_rejection(db, &before, action.label(), code);
            return Err(SessionError::InvalidTransition {
                from,
                action: action.label(),
                code,
            });
        }
    };
    let timestamp = now();
    let attempts = if matches!(
        action,
        Transition::RuntimeFailed | Transition::Reconnect | Transition::ReconnectExhausted
    ) {
        before.reconnect_attempts.saturating_add(1)
    } else {
        before.reconnect_attempts
    };
    let attempts = sqlite_i64(attempts, "reconnect_attempts")?;
    let persisted_elapsed_ms = sqlite_i64(elapsed_ms, "last_elapsed_ms")?;
    db.conn().execute(
        "UPDATE session_hosts SET status=?2,reconnect_attempts=?3,updated_at=?4,last_branch='session_transition',last_reason_code=?5,last_elapsed_ms=?6 WHERE session_id=?1",
        rusqlite::params![
            session_id,
            next.as_str(),
            attempts,
            timestamp,
            code,
            persisted_elapsed_ms
        ],
    )?;
    let after = get(db, session_id)?;
    emit(
        db,
        &after.project_id,
        &after,
        "session_transition",
        "session_transition",
        code,
        elapsed_ms,
    )?;
    tx.commit()?;
    Ok(after)
}

fn emit(
    db: &Db,
    project_id: &str,
    record: &SessionRecord,
    kind: &str,
    branch: &str,
    code: &str,
    elapsed_ms: u64,
) -> Result<(), SessionError> {
    let started = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(elapsed_ms))
        .unwrap_or_else(std::time::Instant::now);
    crate::diag::note(
        crate::diag::CLASS_HOST,
        false,
        Some(project_id),
        None,
        record.current_turn_id.as_deref(),
        Some(&record.session_id),
        branch,
        code,
        started,
    );
    db.append_event(
        project_id,
        EventKind::System,
        json!({"kind": kind, "session_id": record.session_id, "status": record.status, "branch": branch, "reason_code": code, "elapsed_ms": elapsed_ms}),
        None,
        None,
    )
    .map(|_| ())
    .map_err(|e| SessionError::Trace(e.to_string()))
}

fn emit_rejection(db: &Db, record: &SessionRecord, action: &str, code: &str) {
    let started = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(record.last_elapsed_ms))
        .unwrap_or_else(std::time::Instant::now);
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&record.project_id),
        None,
        record.current_turn_id.as_deref(),
        Some(&record.session_id),
        "session_transition_rejected",
        code,
        started,
    );
    let _ = db.append_event(
        &record.project_id,
        EventKind::System,
        json!({"kind":"session_transition_rejected","session_id":record.session_id,"status":record.status,"action":action,"reason_code":code}),
        None,
        None,
    );
}

fn emit_trigger_diag(
    project_id: &str,
    session_id: &str,
    trace_id: Option<&str>,
    branch: &str,
    code: &str,
) {
    crate::diag::note(
        crate::diag::CLASS_HOST,
        false,
        Some(project_id),
        None,
        trace_id,
        Some(session_id),
        branch,
        code,
        std::time::Instant::now(),
    );
}

fn parse_trigger_state(state: &str) -> Result<OneTimeTriggerState, SessionError> {
    match state {
        "scheduled" => Ok(OneTimeTriggerState::Scheduled),
        "expired" => Ok(OneTimeTriggerState::Expired),
        "consumed" => Ok(OneTimeTriggerState::Consumed),
        "cancelled" => Ok(OneTimeTriggerState::Cancelled),
        _ => Err(SessionError::CorruptState(format!(
            "unknown one-time trigger state: {state}"
        ))),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[allow(clippy::type_complexity)]
fn trigger_from_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        Option<String>,
    ),
) -> Result<OneTimeTrigger, SessionError> {
    let fire_at = u64::try_from(row.7)
        .map_err(|_| SessionError::CorruptState("negative trigger fire_at".into()))?;
    let permission_package = serde_json::from_str(&row.6)
        .map_err(|e| SessionError::CorruptState(format!("invalid trigger permissions: {e}")))?;
    Ok(OneTimeTrigger {
        trigger_id: row.0,
        session_id: row.1,
        project_root: row.2,
        baseline: row.3,
        model_slot: row.4,
        permission_package,
        fire_at,
        trace_id: row.8,
        state: parse_trigger_state(&row.9)?,
        created_at: row.5,
        consumed_at: row.10,
    })
}

fn trigger_row(
    db: &Db,
    session_id: &str,
    trigger_id: &str,
) -> Result<OneTimeTrigger, SessionError> {
    let row = db
        .conn()
        .query_row(
            "SELECT trigger_id,session_id,project_root,baseline,model_slot,created_at,
                    permission_package_json,fire_at,trace_id,state,consumed_at
             FROM session_triggers WHERE session_id=?1 AND trigger_id=?2",
            rusqlite::params![session_id, trigger_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                SessionError::NotFound(format!("trigger {trigger_id}"))
            }
            other => SessionError::Sqlite(other),
        })?;
    trigger_from_row(row)
}

pub fn list_triggers(db: &Db, session_id: &str) -> Result<Vec<OneTimeTrigger>, SessionError> {
    let mut stmt = db.conn().prepare(
        "SELECT trigger_id,session_id,project_root,baseline,model_slot,created_at,
                permission_package_json,fire_at,trace_id,state,consumed_at
         FROM session_triggers WHERE session_id=?1 ORDER BY created_at DESC, trigger_id",
    )?;
    let rows = stmt.query_map([session_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, Option<String>>(10)?,
        ))
    })?;
    rows.map(|row| trigger_from_row(row.map_err(SessionError::Sqlite)?))
        .collect()
}

/// Start a new task chain for an explicit owner instruction. A failed chain or
/// one bound to another root/baseline cannot be continued by a new task, so it
/// is superseded (kept as history, `is_current=0`); its old recovery entries
/// stay stale per ADR 0083. Continuations never call this: they must go
/// through explicit reconnect/resume on the existing chain.
pub fn start_task_chain(
    db: &Db,
    project_id: &str,
    project_root: &str,
    baseline: &str,
    reconnect_budget: u64,
) -> Result<SessionRecord, SessionError> {
    if let Some(existing) = current(db, project_id)? {
        let rebound = existing.project_root != project_root || existing.baseline != baseline;
        if existing.status == SessionStatus::Failed || rebound {
            let tx = rusqlite::Transaction::new_unchecked(
                db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )?;
            let next = insert_session(db, project_id, project_root, baseline, reconnect_budget)?;
            tx.commit()?;
            let old = get(db, &existing.session_id)?;
            emit(
                db,
                project_id,
                &old,
                "session_superseded",
                "session_create",
                if rebound {
                    "binding_changed_new_task"
                } else {
                    "failed_chain_new_task"
                },
                0,
            )?;
            return Ok(next);
        }
    }
    ensure_current(db, project_id, project_root, baseline, reconnect_budget)
}

/// Attach only the binding metadata. The host never creates or deletes a git
/// worktree here; cleanup remains an explicit, permission-checked action.
pub fn attach_worktree(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    attachment: WorktreeAttachment,
) -> Result<SessionRecord, SessionError> {
    let record = validate_binding(db, session_id, project_root, baseline, "worktree_attach")?;
    if attachment.path.trim().is_empty()
        || attachment.baseline != baseline
        || attachment
            .baseline_commit
            .as_deref()
            .is_some_and(str::is_empty)
    {
        emit_rejection(db, &record, "worktree_attach", "worktree_binding_mismatch");
        return Err(SessionError::BindingMismatch(
            "worktree path and baseline must match the session binding".into(),
        ));
    }
    if let Some(existing) = record.worktree.as_ref() {
        if existing.path != attachment.path || existing.baseline != attachment.baseline {
            emit_rejection(db, &record, "worktree_attach", "worktree_already_bound");
            return Err(SessionError::BindingMismatch(
                "session already has a different worktree binding".into(),
            ));
        }
    }
    let encoded = serde_json::to_string(&attachment)
        .map_err(|e| SessionError::CorruptState(e.to_string()))?;
    db.conn().execute(
        "UPDATE session_hosts SET worktree_json=?2,updated_at=?3 WHERE session_id=?1",
        rusqlite::params![session_id, encoded, now()],
    )?;
    let updated = get(db, session_id)?;
    emit(
        db,
        &updated.project_id,
        &updated,
        "worktree_attached",
        "worktree_attach",
        "worktree_attached",
        0,
    )?;
    Ok(updated)
}

/// Verify persisted worktree metadata before re-entry. This intentionally does
/// not inspect the filesystem; a caller that has current git facts can compare
/// them with `WorktreeAttachment` and route drift to user action.
pub fn validate_worktree(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
) -> Result<SessionRecord, SessionError> {
    let record = validate_binding(db, session_id, project_root, baseline, "worktree_validate")?;
    if let Some(worktree) = &record.worktree {
        if worktree.path.trim().is_empty()
            || worktree.baseline != record.baseline
            || worktree
                .baseline_commit
                .as_deref()
                .is_some_and(str::is_empty)
        {
            emit_rejection(
                db,
                &record,
                "worktree_validate",
                "worktree_binding_mismatch",
            );
            return Err(SessionError::BindingMismatch(
                "persisted worktree metadata does not match the session".into(),
            ));
        }
    }
    Ok(record)
}

/// Mark cleanup requested after the existing approval/grant path has yielded a
/// proof. It records intent only; it never removes user files.
pub fn request_worktree_cleanup(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    permission_recheck: PermissionRecheck,
) -> Result<SessionRecord, SessionError> {
    if !permission_recheck.permits("worktree_cleanup") {
        return Err(SessionError::PermissionRecheckMismatch(
            "worktree_cleanup".into(),
        ));
    }
    let record = validate_worktree(db, session_id, project_root, baseline)?;
    if record.worktree.is_none() {
        return Err(SessionError::NotFound(format!(
            "worktree for session {session_id}"
        )));
    }
    let mut worktree = record.worktree.clone().expect("checked above");
    worktree.cleanup_requested = true;
    let encoded =
        serde_json::to_string(&worktree).map_err(|e| SessionError::CorruptState(e.to_string()))?;
    db.conn().execute(
        "UPDATE session_hosts SET worktree_json=?2,updated_at=?3 WHERE session_id=?1",
        rusqlite::params![session_id, encoded, now()],
    )?;
    let updated = get(db, session_id)?;
    emit(
        db,
        &updated.project_id,
        &updated,
        "worktree_cleanup_requested",
        "worktree_cleanup",
        "owner_requested_cleanup",
        0,
    )?;
    Ok(updated)
}

/// Clear the binding only after the core has verified that the physical git
/// worktree was removed. Failed cleanup keeps the durable intent for retry.
pub fn clear_worktree(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
) -> Result<SessionRecord, SessionError> {
    let record = validate_worktree(db, session_id, project_root, baseline)?;
    db.conn().execute(
        "UPDATE session_hosts SET worktree_json=NULL,updated_at=?2 WHERE session_id=?1",
        rusqlite::params![session_id, now()],
    )?;
    let updated = get(db, session_id)?;
    emit(
        db,
        &record.project_id,
        &updated,
        "worktree_binding_cleared",
        "worktree_delete",
        "worktree_deleted",
        0,
    )?;
    Ok(updated)
}

/// Record or replace the physical worktree intent. Only the `worktree` card
/// confirmation and the read-only verify call this.
pub fn set_worktree_intent(
    db: &Db,
    session_id: &str,
    intent: Option<&WorktreeIntent>,
) -> Result<SessionRecord, SessionError> {
    let encoded = intent
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| SessionError::CorruptState(e.to_string()))?;
    db.conn().execute(
        "UPDATE session_hosts SET worktree_intent_json=?2,updated_at=?3 WHERE session_id=?1",
        rusqlite::params![session_id, encoded, now()],
    )?;
    get(db, session_id)
}

/// Persist a one-time continuation request. Repeating the same `(session_id,
/// trigger_id)` with the same payload returns the original row.
#[allow(clippy::too_many_arguments)]
pub fn schedule_once(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    trigger_id: &str,
    model_slot: &str,
    permission_package: Vec<String>,
    fire_at: u64,
    trace_id: &str,
) -> Result<OneTimeTrigger, SessionError> {
    let session = validate_binding(db, session_id, project_root, baseline, "trigger_schedule")?;
    if session.status.terminal() {
        emit_rejection(db, &session, "trigger_schedule", "terminal_session");
        return Err(SessionError::InvalidTransition {
            from: session.status,
            action: "trigger_schedule",
            code: "terminal_session",
        });
    }
    if trigger_id.trim().is_empty() || model_slot.trim().is_empty() || trace_id.trim().is_empty() {
        return Err(SessionError::TriggerConflict(
            "trigger_id, model_slot and trace_id are required".into(),
        ));
    }
    let fire_at_i64 = sqlite_i64(fire_at, "fire_at")?;
    let package_json = serde_json::to_string(&permission_package)
        .map_err(|e| SessionError::CorruptState(e.to_string()))?;
    match trigger_row(db, session_id, trigger_id) {
        Ok(existing) => {
            if existing.project_root == project_root
                && existing.baseline == baseline
                && existing.model_slot == model_slot
                && existing.permission_package == permission_package
                && existing.fire_at == fire_at
                && existing.trace_id == trace_id
            {
                return Ok(existing);
            }
            return Err(SessionError::TriggerConflict(format!(
                "trigger {trigger_id} already exists with different metadata"
            )));
        }
        Err(SessionError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }
    db.conn().execute(
        "INSERT INTO session_triggers(trigger_id,session_id,project_id,project_root,baseline,model_slot,permission_package_json,fire_at,trace_id,state)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'scheduled')",
        rusqlite::params![
            trigger_id,
            session_id,
            session.project_id,
            project_root,
            baseline,
            model_slot,
            package_json,
            fire_at_i64,
            trace_id
        ],
    )?;
    let trigger = trigger_row(db, session_id, trigger_id)?;
    emit_trigger_diag(
        &session.project_id,
        session_id,
        Some(trace_id),
        "trigger_schedule",
        "trigger_scheduled",
    );
    db.append_event(
        &session.project_id,
        EventKind::System,
        json!({"kind":"trigger_scheduled","session_id":session_id,"trigger_id":trigger_id,"trace_id":trace_id,"fire_at":fire_at}),
        None,
        None,
    )
    .map_err(|e| SessionError::Trace(e.to_string()))?;
    Ok(trigger)
}

/// Owner retracts a "continue later" before it fires (ui-redesign gap D1,
/// 2026-10-16). Only `scheduled -> cancelled` moves; the UPDATE is guarded on
/// `state='scheduled'`, so a beat that already expired the row wins and cancel
/// reports `TriggerAlreadyFired` instead of racing the continuation. Repeating
/// a cancel returns the cancelled row (idempotent, no second event).
/// No binding or permission recheck: cancelling removes a future side effect
/// and never starts one, so a drifted worktree or a pending card must not
/// block it. Rejected alternative: deleting the row — it stays as history.
pub fn cancel_once(
    db: &Db,
    session_id: &str,
    trigger_id: &str,
) -> Result<OneTimeTrigger, SessionError> {
    let trigger = trigger_row(db, session_id, trigger_id)?;
    let project_id = get(db, session_id)?.project_id;
    let refuse = |trace_id: &str| {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&project_id),
            None,
            Some(trace_id),
            Some(session_id),
            "trigger_cancel",
            "trigger_already_fired",
            std::time::Instant::now(),
        );
        SessionError::TriggerAlreadyFired(trigger_id.into())
    };
    match trigger.state {
        OneTimeTriggerState::Cancelled => return Ok(trigger),
        OneTimeTriggerState::Expired | OneTimeTriggerState::Consumed => {
            return Err(refuse(&trigger.trace_id))
        }
        OneTimeTriggerState::Scheduled => {}
    }
    let changed = db.conn().execute(
        "UPDATE session_triggers SET state='cancelled' WHERE session_id=?1 AND trigger_id=?2 AND state='scheduled'",
        rusqlite::params![session_id, trigger_id],
    )?;
    let current = trigger_row(db, session_id, trigger_id)?;
    if changed == 0 {
        return match current.state {
            OneTimeTriggerState::Cancelled => Ok(current),
            _ => Err(refuse(&current.trace_id)),
        };
    }
    emit_trigger_diag(
        &project_id,
        session_id,
        Some(&current.trace_id),
        "trigger_cancel",
        "trigger_cancelled",
    );
    db.append_event(
        &project_id,
        EventKind::System,
        json!({"kind":"trigger_cancelled","session_id":session_id,"trigger_id":trigger_id,"trace_id":current.trace_id}),
        None,
        None,
    )
    .map_err(|e| SessionError::Trace(e.to_string()))?;
    Ok(current)
}

/// Mark due scheduled requests expired. Opening a project may call this; it
/// never starts a turn or invokes an adapter.
pub fn expire_due_now(db: &Db, project_id: &str) -> Result<Vec<OneTimeTrigger>, SessionError> {
    expire_due(db, project_id, unix_now())
}

pub fn expire_due(db: &Db, project_id: &str, at: u64) -> Result<Vec<OneTimeTrigger>, SessionError> {
    let at_i64 = sqlite_i64(at, "expiry_time")?;
    let ids: Vec<(String, String)> = {
        let mut stmt = db.conn().prepare(
            "SELECT session_id,trigger_id FROM session_triggers
             WHERE project_id=?1 AND state='scheduled' AND fire_at<=?2
             ORDER BY fire_at,trigger_id",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_id, at_i64], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        rows.collect::<Result<_, _>>()?
    };
    let mut expired = Vec::with_capacity(ids.len());
    for (session_id, trigger_id) in ids {
        let changed = db.conn().execute(
            "UPDATE session_triggers SET state='expired' WHERE session_id=?1 AND trigger_id=?2 AND state='scheduled'",
            rusqlite::params![session_id, trigger_id],
        )?;
        if changed == 0 {
            continue;
        }
        let trigger = trigger_row(db, &session_id, &trigger_id)?;
        emit_trigger_diag(
            project_id,
            &session_id,
            Some(&trigger.trace_id),
            "trigger_expire",
            "trigger_expired",
        );
        db.append_event(
            project_id,
            EventKind::System,
            json!({"kind":"trigger_expired","session_id":session_id,"trigger_id":trigger_id,"trace_id":trigger.trace_id}),
            None,
            None,
        )
        .map_err(|e| SessionError::Trace(e.to_string()))?;
        expired.push(trigger);
    }
    Ok(expired)
}

/// Consume only after the Workbench has rechecked approval/grants. A duplicate
/// consume returns the consumed row and therefore cannot enqueue a second side effect.
pub fn consume_once(
    db: &Db,
    session_id: &str,
    project_root: &str,
    baseline: &str,
    trigger_id: &str,
    permission_recheck: PermissionRecheck,
) -> Result<OneTimeTrigger, SessionError> {
    if !permission_recheck.permits("trigger_consume") {
        return Err(SessionError::PermissionRecheckMismatch(
            "trigger_consume".into(),
        ));
    }
    let session = validate_binding(db, session_id, project_root, baseline, "trigger_consume")?;
    let trigger = trigger_row(db, session_id, trigger_id)?;
    if trigger.project_root != project_root || trigger.baseline != baseline {
        emit_rejection(db, &session, "trigger_consume", "binding_mismatch");
        return Err(SessionError::BindingMismatch(format!(
            "trigger {trigger_id} binding changed"
        )));
    }
    match trigger.state {
        OneTimeTriggerState::Consumed => return Ok(trigger),
        OneTimeTriggerState::Cancelled => {
            return Err(SessionError::TriggerCancelled(trigger_id.into()))
        }
        OneTimeTriggerState::Scheduled if trigger.fire_at > unix_now() => {
            return Err(SessionError::TriggerNotReady(trigger_id.into()))
        }
        OneTimeTriggerState::Scheduled => {
            let changed = db.conn().execute(
                "UPDATE session_triggers SET state='expired' WHERE session_id=?1 AND trigger_id=?2 AND state='scheduled'",
                rusqlite::params![session_id, trigger_id],
            )?;
            if changed > 0 {
                emit_trigger_diag(
                    &session.project_id,
                    session_id,
                    Some(&trigger.trace_id),
                    "trigger_expire",
                    "trigger_expired",
                );
                db.append_event(
                    &session.project_id,
                    EventKind::System,
                    json!({"kind":"trigger_expired","session_id":session_id,"trigger_id":trigger_id,"trace_id":trigger.trace_id}),
                    None,
                    None,
                )
                .map_err(|e| SessionError::Trace(e.to_string()))?;
            }
            return Err(SessionError::TriggerNotReady(trigger_id.into()));
        }
        OneTimeTriggerState::Expired => {}
    }
    db.conn().execute(
        "UPDATE session_triggers SET state='consumed',consumed_at=?3 WHERE session_id=?1 AND trigger_id=?2 AND state='expired'",
        rusqlite::params![session_id, trigger_id, now()],
    )?;
    let consumed = trigger_row(db, session_id, trigger_id)?;
    emit_trigger_diag(
        &session.project_id,
        session_id,
        Some(&consumed.trace_id),
        "trigger_consume",
        "trigger_consumed",
    );
    db.append_event(
        &session.project_id,
        EventKind::System,
        json!({"kind":"trigger_consumed","session_id":session_id,"trigger_id":trigger_id,"trace_id":consumed.trace_id}),
        None,
        None,
    )
    .map_err(|e| SessionError::Trace(e.to_string()))?;
    Ok(consumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects(id,dir,name,mode) VALUES ('p1','/tmp/p','p','pack')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn lifecycle_persists_and_reloads_stable_identity() {
        let db = db();
        let created = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        let id = created.session_id.clone();
        assert_eq!(created.status, SessionStatus::Created);
        assert_eq!(current(&db, "p1").unwrap().unwrap().session_id, id);
        assert_eq!(
            transition(&db, &id, Transition::Start, 1).unwrap().status,
            SessionStatus::Running
        );
        assert_eq!(
            transition(&db, &id, Transition::Pause, 2).unwrap().status,
            SessionStatus::Paused
        );
        assert_eq!(
            resume(
                &db,
                &id,
                "/tmp/p",
                "main",
                3,
                PermissionRecheck::confirmed(),
            )
            .unwrap()
            .status,
            SessionStatus::Running
        );
        assert_eq!(
            resume(
                &db,
                &id,
                "/tmp/p",
                "main",
                4,
                PermissionRecheck::confirmed(),
            )
            .unwrap()
            .session_id,
            id
        );
        assert_eq!(get(&db, &id).unwrap().session_id, id);
    }

    #[test]
    fn invalid_and_terminal_transitions_are_rejected() {
        let db = db();
        let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        let err = transition(&db, &s.session_id, Transition::Pause, 0).unwrap_err();
        assert!(matches!(
            err,
            SessionError::InvalidTransition {
                code: "transition_not_allowed",
                ..
            }
        ));
        transition(&db, &s.session_id, Transition::Start, 0).unwrap();
        transition(&db, &s.session_id, Transition::Complete, 0).unwrap();
        let err = transition(&db, &s.session_id, Transition::Reconnect, 0).unwrap_err();
        assert!(matches!(
            err,
            SessionError::InvalidTransition {
                code: "terminal_session",
                ..
            }
        ));
    }

    #[test]
    fn process_exit_pauses_active_host_with_distinct_reason() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &session.session_id, Transition::Start, 0).unwrap();
        let exited = transition(&db, &session.session_id, Transition::ProcessExit, 7).unwrap();
        assert_eq!(exited.status, SessionStatus::Paused);
        assert_eq!(exited.last_reason_code.as_deref(), Some("process_exit"));
    }

    #[test]
    fn runtime_beat_persists_health_without_changing_session_state() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &session.session_id, Transition::Start, 0).unwrap();
        let updated = runtime_beat(
            &db,
            &session.session_id,
            "session_host",
            CapabilityHealth::Healthy,
            None,
            0,
        )
        .unwrap();
        assert_eq!(updated.status, SessionStatus::Running);
        assert_eq!(updated.runtime.unwrap().health, CapabilityHealth::Healthy);
    }

    #[test]
    fn physical_operation_proof_cannot_be_reused_for_recovery() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &session.session_id, Transition::Start, 0).unwrap();
        transition(&db, &session.session_id, Transition::Pause, 0).unwrap();
        assert!(matches!(
            resume(
                &db,
                &session.session_id,
                "/tmp/p",
                "main",
                0,
                PermissionRecheck::confirmed_for("worktree_cleanup"),
            ),
            Err(SessionError::PermissionRecheckMismatch(ref operation)) if operation == "resume"
        ));
    }

    #[test]
    fn binds_turn_and_rejects_changed_project_binding() {
        let db = db();
        let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        assert_eq!(
            bind_turn(&db, &s.session_id, "turn-1")
                .unwrap()
                .current_turn_id
                .as_deref(),
            Some("turn-1")
        );
        let err = resume(
            &db,
            &s.session_id,
            "/tmp/other",
            "main",
            0,
            PermissionRecheck::confirmed(),
        )
        .unwrap_err();
        assert!(matches!(err, SessionError::BindingMismatch(_)));
    }

    #[test]
    fn reconnect_budget_is_enforced() {
        let db = db();
        let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &s.session_id, Transition::Start, 0).unwrap();
        assert_eq!(
            transition(&db, &s.session_id, Transition::RuntimeFailed, 0)
                .unwrap()
                .status,
            SessionStatus::Reconnecting
        );
        assert_eq!(
            transition(&db, &s.session_id, Transition::Reconnected, 0)
                .unwrap()
                .status,
            SessionStatus::Running
        );
        assert_eq!(
            transition(&db, &s.session_id, Transition::RuntimeFailed, 0)
                .unwrap()
                .status,
            SessionStatus::Failed
        );
        assert_eq!(
            get(&db, &s.session_id).unwrap().last_reason_code.as_deref(),
            Some("reconnect_budget_exhausted")
        );
        assert_eq!(get(&db, &s.session_id).unwrap().reconnect_attempts, 2);
        assert!(transition(&db, &s.session_id, Transition::Reconnect, 0).is_err());
    }

    #[test]
    fn completed_task_starts_a_new_identity_but_reopen_only_reads() {
        let db = db();
        let first = ensure_current(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &first.session_id, Transition::Start, 0).unwrap();
        transition(&db, &first.session_id, Transition::Complete, 0).unwrap();
        assert_eq!(
            current(&db, "p1").unwrap().unwrap().session_id,
            first.session_id
        );
        let next = ensure_current(&db, "p1", "/tmp/p", "main", 2).unwrap();
        assert_ne!(first.session_id, next.session_id);
        assert!(!get(&db, &first.session_id).unwrap().is_current);
        assert!(matches!(
            schedule_once(
                &db,
                &first.session_id,
                "/tmp/p",
                "main",
                "terminal-trigger",
                "default",
                vec![],
                10,
                "trace-terminal",
            ),
            Err(SessionError::InvalidTransition {
                code: "terminal_session",
                ..
            })
        ));
    }

    #[test]
    fn failed_task_keeps_identity_until_explicit_recovery() {
        let db = db();
        let session = ensure_current(&db, "p1", "/tmp/p", "main", 2).unwrap();
        transition(&db, &session.session_id, Transition::Start, 0).unwrap();
        transition(&db, &session.session_id, Transition::Fail, 0).unwrap();
        let current_session = ensure_current(&db, "p1", "/tmp/p", "main", 2).unwrap();
        assert_eq!(current_session.session_id, session.session_id);
        assert_eq!(current_session.status, SessionStatus::Failed);
        assert!(matches!(
            transition(&db, &current_session.session_id, Transition::Reconnect, 0),
            Ok(record) if record.status == SessionStatus::Reconnecting
        ));
    }

    #[test]
    fn oversized_values_cannot_replace_or_mutate_the_current_session() {
        let db = db();
        let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        assert!(matches!(
            create(&db, "p1", "/tmp/p", "main", u64::MAX),
            Err(SessionError::InvalidValue(_))
        ));
        assert_eq!(current(&db, "p1").unwrap().unwrap(), s);
        assert!(matches!(
            transition(&db, &s.session_id, Transition::Start, u64::MAX),
            Err(SessionError::InvalidValue(_))
        ));
        assert_eq!(get(&db, &s.session_id).unwrap(), s);
    }

    #[test]
    fn worktree_metadata_is_bound_and_cleanup_is_only_intent() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        let attachment = WorktreeAttachment {
            path: "/tmp/p-worktree".into(),
            baseline: "main".into(),
            baseline_commit: None,
            created_by_session: true,
            cleanup_requested: false,
        };
        let attached =
            attach_worktree(&db, &session.session_id, "/tmp/p", "main", attachment).unwrap();
        assert_eq!(attached.worktree.as_ref().unwrap().path, "/tmp/p-worktree");
        assert!(
            !validate_worktree(&db, &session.session_id, "/tmp/p", "main")
                .unwrap()
                .worktree
                .as_ref()
                .unwrap()
                .cleanup_requested
        );
        assert!(attach_worktree(
            &db,
            &session.session_id,
            "/tmp/p",
            "main",
            WorktreeAttachment {
                path: "/tmp/another-worktree".into(),
                baseline: "main".into(),
                baseline_commit: None,
                created_by_session: true,
                cleanup_requested: false,
            },
        )
        .is_err());
        let cleanup = request_worktree_cleanup(
            &db,
            &session.session_id,
            "/tmp/p",
            "main",
            PermissionRecheck::confirmed(),
        )
        .unwrap();
        assert!(cleanup.worktree.unwrap().cleanup_requested);
        assert!(!std::path::Path::new("/tmp/p-worktree").exists());
        assert!(attach_worktree(
            &db,
            &session.session_id,
            "/tmp/p",
            "other",
            WorktreeAttachment {
                path: "/tmp/p-worktree".into(),
                baseline: "other".into(),
                baseline_commit: None,
                created_by_session: true,
                cleanup_requested: false,
            },
        )
        .is_err());
    }

    #[test]
    fn one_time_trigger_expires_and_consumes_idempotently() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        let trigger = schedule_once(
            &db,
            &session.session_id,
            "/tmp/p",
            "main",
            "later-1",
            "default",
            vec!["mcp:docs".into()],
            10,
            "trace-1",
        )
        .unwrap();
        assert_eq!(trigger.state, OneTimeTriggerState::Scheduled);
        assert_eq!(
            schedule_once(
                &db,
                &session.session_id,
                "/tmp/p",
                "main",
                "later-1",
                "default",
                vec!["mcp:docs".into()],
                10,
                "trace-1",
            )
            .unwrap()
            .trigger_id,
            "later-1"
        );
        assert_eq!(expire_due(&db, "p1", 10).unwrap().len(), 1);
        assert_eq!(
            list_triggers(&db, &session.session_id)
                .unwrap()
                .first()
                .unwrap()
                .state,
            OneTimeTriggerState::Expired
        );
        let consumed = consume_once(
            &db,
            &session.session_id,
            "/tmp/p",
            "main",
            "later-1",
            PermissionRecheck::confirmed(),
        )
        .unwrap();
        assert_eq!(consumed.state, OneTimeTriggerState::Consumed);
        assert_eq!(
            consume_once(
                &db,
                &session.session_id,
                "/tmp/p",
                "main",
                "later-1",
                PermissionRecheck::confirmed(),
            )
            .unwrap()
            .state,
            OneTimeTriggerState::Consumed
        );
        assert!(matches!(
            schedule_once(
                &db,
                &session.session_id,
                "/tmp/other",
                "main",
                "later-2",
                "default",
                vec![],
                10,
                "trace-2",
            ),
            Err(SessionError::BindingMismatch(_))
        ));
    }

    #[test]
    fn one_time_trigger_before_due_stays_scheduled() {
        let db = db();
        let session = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        schedule_once(
            &db,
            &session.session_id,
            "/tmp/p",
            "main",
            "later-early",
            "default",
            vec![],
            unix_now().saturating_add(60),
            "trace-early",
        )
        .unwrap();
        assert!(matches!(
            consume_once(
                &db,
                &session.session_id,
                "/tmp/p",
                "main",
                "later-early",
                PermissionRecheck::confirmed(),
            ),
            Err(SessionError::TriggerNotReady(_))
        ));
        assert_eq!(
            trigger_row(&db, &session.session_id, "later-early")
                .unwrap()
                .state,
            OneTimeTriggerState::Scheduled
        );
    }

    #[test]
    fn malformed_attachment_fails_closed() {
        let db = db();
        let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
        db.conn()
            .execute(
                "UPDATE session_hosts SET worktree_json='{' WHERE session_id=?1",
                [&s.session_id],
            )
            .unwrap();
        assert!(matches!(
            get(&db, &s.session_id),
            Err(SessionError::CorruptState(_))
        ));
    }

    proptest! {
        #[test]
        fn terminal_state_never_moves_again(actions in prop::collection::vec(0u8..10, 0..30)) {
            let db = db();
            let s = create(&db, "p1", "/tmp/p", "main", 2).unwrap();
            let mut terminal = false;
            for action in actions {
                let requested = match action {
                    0 => Transition::Start,
                    1 => Transition::WaitUser,
                    2 => Transition::Approve,
                    3 => Transition::Pause,
                    4 => Transition::RuntimeFailed,
                    5 => Transition::Reconnected,
                    6 => Transition::ReconnectExhausted,
                    7 => Transition::Complete,
                    8 => Transition::Fail,
                    _ => Transition::Reconnect,
                };
                let before = get(&db, &s.session_id).unwrap();
                let result = super::transition(&db, &s.session_id, requested, 0);
                let after = get(&db, &s.session_id).unwrap();
                if terminal {
                    let explicit_reconnect = before.status == SessionStatus::Failed
                        && requested == Transition::Reconnect;
                    if before.status == SessionStatus::Completed || !explicit_reconnect {
                        prop_assert!(result.is_err());
                        prop_assert_eq!(before.status, after.status);
                    }
                }
                terminal = after.status.terminal();
            }
        }
    }
}
