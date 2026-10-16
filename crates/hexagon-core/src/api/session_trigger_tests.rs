//! P1-2 trigger continuation through the real Workbench facade.
//!
//! These fixtures keep the trigger metadata path separate from the activation
//! path: the continuation must recover the existing TurnStarted identity and
//! use the provider only after the one-time row is consumed.

use super::*;
use crate::provider::ScriptedProvider;
use crate::session_host::{
    self, OneTimeTriggerState, PermissionRecheck, SessionStatus, Transition,
};
use crate::trace::EventKind;
use crate::turn::text_response;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn live_trigger_workbench(dir: &Path) -> (Workbench, String, i64) {
    let wb = Workbench::for_test(dir, &["worker"], None).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    let session =
        session_host::ensure_current(&wb.db, "p1", &dir.to_string_lossy(), "main", 3).unwrap();
    session_host::transition(&wb.db, &session.session_id, Transition::Start, 0).unwrap();
    let turn = wb
        .db
        .append_event(
            "p1",
            EventKind::TurnStarted,
            serde_json::json!({"instruction":"continue the original task","subagent":false}),
            Some("a0"),
            None,
        )
        .unwrap();
    session_host::bind_turn(&wb.db, &session.session_id, &turn.to_string()).unwrap();
    (wb, session.session_id, turn)
}

#[test]
fn live_due_trigger_continues_the_existing_session_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, original_turn) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("continued")]));
    wb.register_provider("default", provider.clone());
    let trigger = session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "due-1",
        "default",
        vec!["mcp:docs".into()],
        0,
        "trace-due-1",
    )
    .unwrap();
    assert_eq!(trigger.state, OneTimeTriggerState::Scheduled);

    wb.session_trigger_beat().unwrap();

    let consumed = session_host::list_triggers(&wb.db, &session_id)
        .unwrap()
        .into_iter()
        .find(|item| item.trigger_id == "due-1")
        .unwrap();
    assert_eq!(consumed.state, OneTimeTriggerState::Consumed);
    assert_eq!(provider.recorded().len(), 1);
    let current = session_host::current(&wb.db, "p1").unwrap().unwrap();
    assert_eq!(current.session_id, session_id);
    assert_eq!(current.status, SessionStatus::Completed);
    let original_turn_id = original_turn.to_string();
    assert_ne!(
        current.current_turn_id.as_deref(),
        Some(original_turn_id.as_str())
    );
    let continuation = current.current_turn_id.unwrap().parse::<i64>().unwrap();
    let payload: serde_json::Value = wb
        .db
        .conn()
        .query_row(
            "SELECT payload FROM events WHERE id=?1",
            [continuation],
            |row| row.get(0),
        )
        .map(|raw: String| serde_json::from_str(&raw).unwrap())
        .unwrap();
    assert_eq!(payload["resume_from_turn_id"].as_i64(), Some(original_turn));
    assert_eq!(
        payload["activation_root_turn_id"].as_i64(),
        Some(original_turn)
    );

    // A duplicate explicit Continue sees the consumed row and cannot make a
    // second provider request or create another continuation turn.
    let duplicate = wb.continue_session_trigger_once("due-1").unwrap();
    assert_eq!(duplicate.state, OneTimeTriggerState::Consumed);
    assert_eq!(provider.recorded().len(), 1);
    assert_eq!(
        session_host::list_triggers(&wb.db, &session_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn reopening_only_expires_due_trigger_until_explicit_continue() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![
        text_response("continued"),
        text_response("continued after reopen"),
    ]));
    wb.register_provider("default", provider.clone());
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "reopen-1",
        "default",
        vec![],
        0,
        "trace-reopen-1",
    )
    .unwrap();

    let expired = session_host::expire_due_now(&wb.db, "p1").unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].state, OneTimeTriggerState::Expired);
    assert!(provider.recorded().is_empty());
    assert_eq!(
        session_host::transition(&wb.db, &session_id, Transition::ProcessExit, 0)
            .unwrap()
            .status,
        SessionStatus::Paused
    );
    let consumed = wb.continue_session_trigger_once("reopen-1").unwrap();
    assert_eq!(consumed.state, OneTimeTriggerState::Consumed);
    assert_eq!(provider.recorded().len(), 1);
    assert_eq!(
        session_host::current(&wb.db, "p1").unwrap().unwrap().status,
        SessionStatus::Completed
    );
    assert_eq!(
        session_host::list_triggers(&wb.db, &session_id)
            .unwrap()
            .into_iter()
            .find(|item| item.trigger_id == "reopen-1")
            .unwrap()
            .state,
        OneTimeTriggerState::Consumed,
        "explicit Continue consumes metadata exactly once"
    );
}

#[test]
fn live_due_trigger_failure_waits_for_its_recovery_item_without_retrying_the_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    wb.register_provider("default", Arc::new(ScriptedProvider::new(vec![])));
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "failed-1",
        "default",
        vec![],
        0,
        "trace-failed-1",
    )
    .unwrap();

    wb.session_trigger_beat().unwrap();

    let trigger = session_host::list_triggers(&wb.db, &session_id)
        .unwrap()
        .into_iter()
        .find(|item| item.trigger_id == "failed-1")
        .unwrap();
    assert_eq!(trigger.state, OneTimeTriggerState::Consumed);
    // Behaviour change 2026-10-13: a failed model request on the continuation
    // now waits for the owner through its ADR 0083 recovery item instead of
    // spending the runtime reconnect budget (see api::tests::
    // model_request_failures_wait_for_the_owner_without_spending_runtime_budget).
    let current = session_host::current(&wb.db, "p1").unwrap().unwrap();
    assert_eq!(current.status, SessionStatus::WaitingUser);
    assert_eq!(current.reconnect_attempts, 0);
    assert!(crate::cards::queued(&wb.db, "p1")
        .unwrap()
        .iter()
        .any(|card| card.payload["sub"] == "model_request"));
    // A continuation error is recoverable host state, never a second trigger
    // consumption or a fresh session identity.
    assert_eq!(
        session_host::list_triggers(&wb.db, &session_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        session_host::current(&wb.db, "p1")
            .unwrap()
            .unwrap()
            .session_id,
        session_id
    );
}

#[test]
fn explicit_continue_rejects_a_trigger_before_its_due_time() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id, _) = live_trigger_workbench(dir.path());
    let future = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "early-1",
        "default",
        vec![],
        future,
        "trace-early-1",
    )
    .unwrap();
    let error = session_host::consume_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "early-1",
        PermissionRecheck::confirmed_for("trigger_consume"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        session_host::SessionError::TriggerNotReady(trigger) if trigger == "early-1"
    ));
    assert!(
        wb.session_trigger_beat().is_ok(),
        "a before-due row does not run continuation"
    );
}

#[test]
fn due_trigger_waits_for_permission_recheck_without_consuming_or_running() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("must not run")]));
    wb.register_provider("default", provider.clone());
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "denied-1",
        "default",
        vec![],
        0,
        "trace-denied-1",
    )
    .unwrap();
    crate::cards::enqueue(
        &wb.db,
        "p1",
        Some("a0"),
        crate::cards::CardKind::Grant,
        serde_json::json!({"grant_id":"grant-1","scope":"mcp:docs"}),
        None,
    )
    .unwrap();

    wb.session_trigger_beat().unwrap();

    let trigger = session_host::list_triggers(&wb.db, &session_id)
        .unwrap()
        .into_iter()
        .find(|item| item.trigger_id == "denied-1")
        .unwrap();
    assert_eq!(trigger.state, OneTimeTriggerState::Expired);
    assert!(provider.recorded().is_empty());
    assert_eq!(
        session_host::current(&wb.db, "p1").unwrap().unwrap().status,
        SessionStatus::Running
    );
}

#[test]
fn trigger_consume_proof_is_operation_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id, _) = live_trigger_workbench(dir.path());
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "proof-1",
        "default",
        vec![],
        0,
        "trace-proof-1",
    )
    .unwrap();
    session_host::expire_due_now(&wb.db, "p1").unwrap();
    let error = session_host::consume_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "proof-1",
        PermissionRecheck::confirmed_for("resume"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        session_host::SessionError::PermissionRecheckMismatch(operation)
            if operation == "trigger_consume"
    ));
}

/// Wayfinder-07 Addendum A (2026-10-12): on macOS a window close pauses the
/// host but keeps the process alive. The owner's one-time "continue later" is
/// the explicit instruction to wake it, so the live beat must not skip a
/// paused host, and the continuation must lift the close-time orchestra pause
/// or the turn would stop at its first pause check.
#[test]
fn live_due_trigger_wakes_a_window_closed_paused_host() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("woke")]));
    wb.register_provider("default", provider.clone());
    // Same effects as the shell's window-close boundary.
    session_host::transition(&wb.db, &session_id, Transition::Pause, 0).unwrap();
    orchestra::pause(&wb.db, "p1").unwrap();
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "closed-1",
        "default",
        vec![],
        0,
        "trace-closed-1",
    )
    .unwrap();

    wb.session_trigger_beat().unwrap();

    let trigger = session_host::list_triggers(&wb.db, &session_id)
        .unwrap()
        .into_iter()
        .find(|item| item.trigger_id == "closed-1")
        .unwrap();
    assert_eq!(trigger.state, OneTimeTriggerState::Consumed);
    assert_eq!(provider.recorded().len(), 1);
    let current = session_host::current(&wb.db, "p1").unwrap().unwrap();
    assert_eq!(current.session_id, session_id);
    assert_eq!(current.status, SessionStatus::Completed);
    assert!(!orchestra::is_paused(&wb.db, "p1").unwrap());
}

#[test]
fn live_beat_leaves_a_paused_host_alone_before_the_trigger_is_due() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("too early")]));
    wb.register_provider("default", provider.clone());
    session_host::transition(&wb.db, &session_id, Transition::Pause, 0).unwrap();
    orchestra::pause(&wb.db, "p1").unwrap();
    let later = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "later-1",
        "default",
        vec![],
        later,
        "trace-later-1",
    )
    .unwrap();

    wb.session_trigger_beat().unwrap();

    assert!(provider.recorded().is_empty());
    assert_eq!(
        session_host::current(&wb.db, "p1").unwrap().unwrap().status,
        SessionStatus::Paused
    );
    assert!(orchestra::is_paused(&wb.db, "p1").unwrap());
}

/// Audit fields come from the session's current turn, not from IPC; the same
/// trigger id is idempotent, and a past fire time is refused.
#[test]
fn owner_schedule_derives_audit_fields_from_the_current_turn() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id, turn) = live_trigger_workbench(dir.path());
    let later = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    let first = wb.schedule_session_trigger("later-owner", later).unwrap();
    assert_eq!(first.session_id, session_id);
    assert_eq!(first.trace_id, format!("turn:{turn}"));
    assert_eq!(first.model_slot, "default");
    assert!(first.permission_package[0].starts_with("approval_mode:"));
    let again = wb.schedule_session_trigger("later-owner", later).unwrap();
    assert_eq!(again, first);
    assert_eq!(
        session_host::list_triggers(&wb.db, &session_id)
            .unwrap()
            .len(),
        1
    );
    assert!(wb.schedule_session_trigger("too-early", 1).is_err());
}

/// Found in the 2026-10-13 golden path: after a window-close pause the owner
/// could save decisions but nothing could run (`project or instance
/// stopped`), and the strip's resume needed an interrupted run that a close
/// never creates. An explicit owner resume must lift the host and the
/// close-time orchestra pause without one, after the same rechecks.
#[test]
fn owner_resume_lifts_a_window_closed_host_without_an_interrupted_run() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id, _) = live_trigger_workbench(dir.path());
    session_host::transition(&wb.db, &session_id, Transition::Pause, 0).unwrap();
    orchestra::pause(&wb.db, "p1").unwrap();

    // A queued permission decision must be answered first.
    let card = crate::cards::enqueue(
        &wb.db,
        "p1",
        Some("a0"),
        crate::cards::CardKind::Permission,
        serde_json::json!({}),
        None,
    )
    .unwrap();
    assert!(wb.resume_session(None).is_err());
    assert_eq!(
        session_host::current(&wb.db, "p1").unwrap().unwrap().status,
        SessionStatus::Paused
    );
    assert!(orchestra::is_paused(&wb.db, "p1").unwrap());

    crate::cards::answer(&wb.db, &card, "owner").unwrap();
    let resumed = wb.resume_session(None).unwrap();
    assert_eq!(resumed.session_id, session_id);
    assert_eq!(resumed.status, SessionStatus::Running);
    assert!(!orchestra::is_paused(&wb.db, "p1").unwrap());
}

/// Golden path 2026-10-13: Continue on an expired trigger while a permission
/// card waited showed only "输入无效". The refusal carries its own code so
/// the owner is told to answer the pending card first.
#[test]
fn continue_refused_by_a_pending_decision_names_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, _, _) = live_trigger_workbench(dir.path());
    crate::cards::enqueue(
        &wb.db,
        "p1",
        Some("a0"),
        crate::cards::CardKind::Permission,
        serde_json::json!({}),
        None,
    )
    .unwrap();
    let error = wb
        .recheck_session_permissions_for("trigger_consume")
        .unwrap_err();
    assert_eq!(crate::errcode::ErrorCode::code(&error), "decision_pending");
}

fn trigger_state(wb: &Workbench, session_id: &str, trigger_id: &str) -> OneTimeTriggerState {
    session_host::list_triggers(&wb.db, session_id)
        .unwrap()
        .into_iter()
        .find(|item| item.trigger_id == trigger_id)
        .unwrap()
        .state
}

fn cancel_events(wb: &Workbench) -> i64 {
    wb.db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE project_id='p1' AND kind='system' AND json_extract(payload,'$.kind')='trigger_cancelled'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

/// Gap D1 (2026-10-16): the owner books a continue-later, then cancels it
/// before it fires. The row stays as history in `cancelled`, a second cancel
/// is a no-op, and the beat never continues it afterwards.
#[test]
fn owner_cancels_a_booked_trigger_once_and_the_beat_skips_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("never")]));
    wb.register_provider("default", provider.clone());
    let later = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    wb.schedule_session_trigger("later-cancel", later).unwrap();

    let cancelled = wb.cancel_session_trigger("later-cancel").unwrap();
    assert_eq!(cancelled.state, OneTimeTriggerState::Cancelled);
    assert_eq!(cancel_events(&wb), 1);

    let again = wb.cancel_session_trigger("later-cancel").unwrap();
    assert_eq!(again, cancelled);
    assert_eq!(
        cancel_events(&wb),
        1,
        "idempotent cancel emits no second event"
    );

    // Even once its time passes, a cancelled row is neither expired nor run.
    session_host::expire_due(&wb.db, "p1", later + 1).unwrap();
    wb.session_trigger_beat().unwrap();
    assert_eq!(
        trigger_state(&wb, &session_id, "later-cancel"),
        OneTimeTriggerState::Cancelled
    );
    assert!(provider.recorded().is_empty());
    assert!(wb.continue_session_trigger_once("later-cancel").is_err());
}

/// Cancel after the trigger fired is refused with its own code and the fired
/// row keeps its state; an unknown id reports not_found.
#[test]
fn cancel_after_the_trigger_fired_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, session_id, _) = live_trigger_workbench(dir.path());
    let provider = Arc::new(ScriptedProvider::new(vec![text_response("continued")]));
    wb.register_provider("default", provider);
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "fired-1",
        "default",
        vec![],
        0,
        "trace-fired-1",
    )
    .unwrap();
    wb.session_trigger_beat().unwrap();
    assert_eq!(
        trigger_state(&wb, &session_id, "fired-1"),
        OneTimeTriggerState::Consumed
    );

    let error = wb.cancel_session_trigger("fired-1").unwrap_err();
    assert_eq!(
        crate::errcode::ErrorCode::code(&error),
        "trigger_already_fired"
    );
    assert_eq!(
        trigger_state(&wb, &session_id, "fired-1"),
        OneTimeTriggerState::Consumed
    );
    assert_eq!(cancel_events(&wb), 0);

    let missing = wb.cancel_session_trigger("nope").unwrap_err();
    assert_eq!(crate::errcode::ErrorCode::code(&missing), "not_found");
}

/// A due-but-unconsumed (expired) trigger is already handed to Continue; the
/// strip shows it as expired, so cancel refuses rather than hiding it.
#[test]
fn cancel_of_an_expired_trigger_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id, _) = live_trigger_workbench(dir.path());
    session_host::schedule_once(
        &wb.db,
        &session_id,
        &dir.path().to_string_lossy(),
        "main",
        "expired-1",
        "default",
        vec![],
        5,
        "trace-expired-1",
    )
    .unwrap();
    session_host::expire_due(&wb.db, "p1", 10).unwrap();
    let error = wb.cancel_session_trigger("expired-1").unwrap_err();
    assert_eq!(
        crate::errcode::ErrorCode::code(&error),
        "trigger_already_fired"
    );
    assert_eq!(
        trigger_state(&wb, &session_id, "expired-1"),
        OneTimeTriggerState::Expired
    );
}
