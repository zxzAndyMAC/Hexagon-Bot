//! Managed-followups ticket 03 (spec T4): the owner's per-item "stop
//! retrying" reaches a role turn's model request — on the first network wait
//! and during an explicit retry — without pausing the project or putting the
//! role to sleep. Nothing automatic revives it; an explicit retry continues.
//! A file-backed project lets a second connection press stop mid-wait, the
//! way the shell's control lane does.
use super::*;
use crate::provider::{ChatRequest, ChatResponse, ModelProvider, ProviderError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Network down for every call; on call `stop_at` it presses the owner's
/// stop on the role's open item through its own connection.
struct DownThenStop {
    db_path: PathBuf,
    calls: AtomicUsize,
    stop_at: usize,
}

impl ModelProvider for DownThenStop {
    fn is_scripted(&self) -> bool {
        true
    }
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.stream(req, &mut |_| true)
    }
    fn stream(
        &self,
        _req: &ChatRequest,
        _sink: &mut crate::provider::StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.stop_at {
            let db = Db::open(&self.db_path).unwrap();
            let item = open_item(&db).expect("the waiting request is visible as an item");
            stop_model_request(&db, crate::PROJECT_ID, &item).unwrap();
        }
        Err(ProviderError::Transport("network down".into()))
    }
}

/// Network down for `down` calls, then one reply.
struct DownThenUp {
    calls: AtomicUsize,
    down: usize,
}

impl ModelProvider for DownThenUp {
    fn is_scripted(&self) -> bool {
        true
    }
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.stream(req, &mut |_| true)
    }
    fn stream(
        &self,
        _req: &ChatRequest,
        _sink: &mut crate::provider::StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.down {
            return Err(ProviderError::Transport("network down".into()));
        }
        Ok(crate::turn::text_response("finished"))
    }
}

fn open_item(db: &Db) -> Option<String> {
    crate::cards::queued(db, crate::PROJECT_ID)
        .unwrap()
        .into_iter()
        .find(|card| card.payload["sub"] == "model_request" && card.payload["purpose"] == "turn")
        .map(|card| card.id)
}

/// Fast retries (4 sends) happen before the network wait; call 5 is the
/// first probe inside it.
const FIRST_PROBE: usize = 5;

fn project(dir: &Path) -> Workbench {
    let mut wb =
        Workbench::open_scoped(dir, "stop", &[("a0".into(), "worker".into())], None, false)
            .unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.wait_policy = crate::turn::WaitPolicy {
        probe_interval: Duration::from_millis(5),
        budget: Duration::from_secs(60),
        tick: Duration::from_millis(1),
    };
    wb
}

fn stopping(dir: &Path, stop_at: usize) -> Arc<DownThenStop> {
    Arc::new(DownThenStop {
        db_path: dir.join(".hexagon/state.db"),
        calls: AtomicUsize::new(0),
        stop_at,
    })
}

/// A stop is the owner's choice, not a failure: the turn ends Interrupted.
fn stopped_turn(wb: &Workbench) {
    let outcome = wb.run_instance("a0", "Render the scene.");
    assert!(
        matches!(outcome, Ok(TurnOutcome::Interrupted)),
        "{outcome:?}"
    );
}

fn sleeping(wb: &Workbench) -> bool {
    wb.db
        .conn()
        .query_row(
            "SELECT status='sleeping' FROM agents WHERE id='a0'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn stopping_the_first_wait_leaves_one_stopped_item_and_the_team_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = project(dir.path());
    let down = stopping(dir.path(), FIRST_PROBE);
    wb.register_provider("default", down.clone());
    stopped_turn(&wb);
    assert_eq!(
        down.calls.load(Ordering::SeqCst),
        FIRST_PROBE,
        "nothing is sent after the stop"
    );
    let item = open_item(&wb.db).unwrap();
    let card = crate::cards::get(&wb.db, &item).unwrap();
    assert_eq!(card.payload["state"], "stopped");
    assert_eq!(card.payload["stop_requested"], true);
    assert!(
        card.payload["checkpoint_event_id"].is_i64(),
        "position kept"
    );
    assert!(card.payload["task_key"].is_string());
    assert!(!sleeping(&wb), "the role does not sleep");
    assert!(!orchestra::is_paused(&wb.db, &wb.project_id).unwrap());
    // Neither the stall rescue nor delegation revives it. (Autonomy levels
    // were removed — set_autonomy answers GearsRemoved — so the remaining
    // automatic paths are these two.)
    let _ = wb.stall_tick();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("end", "owner_stopped_retries")
    );
    assert_eq!(down.calls.load(Ordering::SeqCst), FIRST_PROBE);
}

#[test]
fn an_explicit_retry_clears_the_stop_and_continues_the_same_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = project(dir.path());
    wb.register_provider("default", stopping(dir.path(), FIRST_PROBE));
    stopped_turn(&wb);
    let item = open_item(&wb.db).unwrap();
    let up = Arc::new(DownThenUp {
        calls: AtomicUsize::new(0),
        down: 0,
    });
    wb.register_provider("default", up.clone());
    wb.retry_model_request(&item).unwrap();
    let card = crate::cards::get(&wb.db, &item).unwrap();
    assert_eq!(card.payload["state"], "recovered");
    assert_eq!(card.payload["stop_requested"], false);
    assert_eq!(up.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn stopping_a_running_retry_stops_its_sends() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = project(dir.path());
    wb.register_provider("default", stopping(dir.path(), FIRST_PROBE));
    stopped_turn(&wb);
    let item = open_item(&wb.db).unwrap();
    // The explicit retry clears the stop, waits again, and is stopped again.
    let again = stopping(dir.path(), FIRST_PROBE);
    wb.register_provider("default", again.clone());
    assert!(wb.retry_model_request(&item).is_err());
    assert_eq!(again.calls.load(Ordering::SeqCst), FIRST_PROBE);
    let card = crate::cards::get(&wb.db, &item).unwrap();
    assert_eq!(card.payload["state"], "stopped");
    assert!(!sleeping(&wb));
}

#[test]
fn a_wait_that_recovers_closes_its_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = project(dir.path());
    wb.register_provider(
        "default",
        Arc::new(DownThenUp {
            calls: AtomicUsize::new(0),
            down: FIRST_PROBE,
        }),
    );
    wb.run_instance("a0", "Render the scene.").unwrap();
    assert!(open_item(&wb.db).is_none(), "no item left open");
    let recovered = wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::System]))
        .unwrap()
        .into_iter()
        .filter(|event| event.payload["kind"] == "model_request_recovered")
        .count();
    assert_eq!(recovered, 1);
}

/// ADR0087 实施修订 5: with autonomy levels gone, "autonomy" is the
/// automatic follow-up after another role's turn. Handing work to the
/// stopped role starts its new turn; it never resends the stopped request.
#[test]
fn an_automatic_follow_up_to_the_stopped_role_never_revives_its_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_scoped(
        dir.path(),
        "stop",
        &[
            ("a0".into(), "worker".into()),
            ("a1".into(), "helper".into()),
        ],
        None,
        false,
    )
    .unwrap();
    for agent in ["a0", "a1"] {
        orchestra::write_agent_status(&wb.db, "p1", agent, false).unwrap();
    }
    wb.wait_policy = crate::turn::WaitPolicy {
        probe_interval: Duration::from_millis(5),
        budget: Duration::from_secs(60),
        tick: Duration::from_millis(1),
    };
    wb.register_provider("default", stopping(dir.path(), FIRST_PROBE));
    stopped_turn(&wb);
    let item = open_item(&wb.db).unwrap();
    let after_stop = |wb: &Workbench, kind: &str| {
        wb.db
            .events(crate::PROJECT_ID, Some(&[EventKind::System]))
            .unwrap()
            .into_iter()
            .filter(|event| {
                event.payload["kind"] == kind && event.payload["recovery_id"] == item.as_str()
            })
            .count()
    };
    let retrying_before = after_stop(&wb, "model_request_retrying");
    let replies = Arc::new(crate::provider::ScriptedProvider::new(vec![
        crate::turn::text_response("plan: hand the render back"),
        crate::turn::text_response("@worker[a0] please pick the render up again"),
        crate::turn::text_response("plan: start over"),
        crate::turn::text_response("picked up as a new task"),
    ]));
    wb.register_provider("default", replies.clone());
    let routed = wb.dispatch_instance("a1", "check the scene", &[]);
    assert!(routed.is_ok(), "{routed:?}");
    let worker_turns = wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::TurnStarted]))
        .unwrap()
        .into_iter()
        .filter(|event| event.agent_id.as_deref() == Some("a0"))
        .count();
    assert_eq!(
        worker_turns, 2,
        "the follow-up really reached the stopped role"
    );
    let card = crate::cards::get(&wb.db, &item).unwrap();
    assert_eq!(card.payload["state"], "stopped");
    assert_eq!(card.payload["stop_requested"], true);
    assert_eq!(after_stop(&wb, "model_request_retrying"), retrying_before);
    assert_eq!(after_stop(&wb, "model_request_recovered"), 0);
}
