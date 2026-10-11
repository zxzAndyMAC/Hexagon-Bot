//! Test3/17: managed permission decisions. The model only picks inside the
//! closed set; every other path ends in deny, never back with the owner and
//! never an implicit allow. A decision prepared under an old grant never lands.
use super::*;
use crate::provider::{ModelProvider, ScriptedProvider};
use crate::tools::CallOutcome;
use crate::turn::text_response;
use serde_json::json;

fn wb_with(reply: Option<&str>) -> (tempfile::TempDir, Workbench, Option<Arc<ScriptedProvider>>) {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let chat = reply.map(|text| Arc::new(ScriptedProvider::new(vec![text_response(text)])));
    if let Some(chat) = &chat {
        wb.register_provider("default", chat.clone() as Arc<dyn ModelProvider>);
    }
    (dir, wb, chat)
}

fn ask(wb: &Workbench, tool: &str, input: serde_json::Value) -> String {
    let ctx = wb.ctx_for("a0", None);
    match wb.registry.call(&wb.db, &ctx, tool, input).unwrap() {
        CallOutcome::Asked(qid) => qid,
        other => panic!("expected a permission card, got {other:?}"),
    }
}

fn managed_events(wb: &Workbench) -> Vec<serde_json::Value> {
    wb.db
        .events(crate::PROJECT_ID, Some(&[EventKind::System]))
        .unwrap()
        .into_iter()
        .map(|event| event.payload)
        .filter(|payload| payload["kind"] == "managed_decision")
        .collect()
}

#[test]
fn delegation_off_leaves_every_card_with_the_owner() {
    let (_dir, mut wb, chat) = wb_with(Some("allow"));
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    assert!(wb.managed_tick().unwrap().is_empty());
    let card = crate::cards::get(&wb.db, &qid).unwrap();
    assert_eq!(card.state, crate::cards::CardState::Queued);
    assert!(
        chat.unwrap().recorded().is_empty(),
        "no model call while off"
    );
}

#[test]
fn a_model_allow_runs_the_real_action_and_continues_the_original_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_scoped(
        dir.path(),
        "managed",
        &[("a0".into(), "worker".into())],
        None,
        false,
    )
    .unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![crate::turn::tool_response(
            vec![("c1", "bash", json!({"cmd":"printf managed > managed.txt"}))],
        )])),
    );
    let TurnOutcome::AwaitingPermission(qid) = wb.run_instance("a0", "write the marker").unwrap()
    else {
        panic!("the permission must come from the real task")
    };
    // One script for both sends: the managed choice, then the resumed task.
    let chat = Arc::new(ScriptedProvider::new(vec![
        text_response("allow"),
        text_response("original task complete"),
    ]));
    wb.register_provider("default", chat.clone());
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let decisions = wb.managed_tick().unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].action, "allow", "{decisions:?}");
    assert_eq!(decisions[0].via, "model");
    let card = crate::cards::get(&wb.db, &qid).unwrap();
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(
        card.answered_by.as_deref(),
        Some(actor.as_str()),
        "never `owner`"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("managed.txt")).unwrap(),
        "managed",
        "the real action ran"
    );
    assert_eq!(
        chat.recorded().len(),
        2,
        "decision + exactly one continuation"
    );
    let events = managed_events(&wb);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["actor"], actor);
    let history = managed_history(&wb.db, crate::PROJECT_ID, 10).unwrap();
    assert_eq!(history[0].card_id, qid);
    assert_eq!(history[0].outcome, "allowed_and_continued");
    // A second beat finds nothing left to decide.
    assert!(wb.managed_tick().unwrap().is_empty());
}

#[test]
fn an_out_of_set_answer_or_no_model_denies() {
    for reply in [Some("sure, go ahead"), None] {
        let (_dir, mut wb, _chat) = wb_with(reply);
        let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
        crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
        let decisions = wb.managed_tick().unwrap();
        assert_eq!(decisions[0].action, "deny", "{reply:?}");
        assert_eq!(decisions[0].via, "fallback");
        assert!(decisions[0].reason.starts_with("managed_route_failed:"));
        let card = crate::cards::get(&wb.db, &qid).unwrap();
        assert_eq!(
            card.state,
            crate::cards::CardState::Answered,
            "not back to the owner"
        );
    }
}

#[test]
fn a_decision_prepared_under_an_old_grant_never_lands() {
    let (_dir, mut wb, _chat) = wb_with(Some("allow"));
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    let decision = ManagedDecision {
        action: "allow".into(),
        reason: "model_choice".into(),
        via: "model".into(),
    };
    // Off and on again between decide and commit: a new generation.
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(wb.managed_commit(&plan[0], decision).unwrap().is_none());
    let card = crate::cards::get(&wb.db, &qid).unwrap();
    assert_eq!(card.state, crate::cards::CardState::Queued);
    assert!(managed_events(&wb).is_empty());
}

#[test]
fn an_exhausted_budget_denies_and_survives_off_on() {
    let (_dir, mut wb, chat) = wb_with(Some("allow"));
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let key = wb.managed_plan().unwrap()[0].decision_key.clone();
    for _ in 0..crate::delegation::MAX_ROUNDS {
        crate::delegation::spend_round(&wb.db, crate::PROJECT_ID, &key).unwrap();
    }
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let decisions = wb.managed_tick().unwrap();
    assert_eq!(decisions[0].action, "deny");
    assert_eq!(decisions[0].reason, "budget_exhausted");
    assert!(chat.unwrap().recorded().is_empty());
    assert_eq!(
        crate::cards::get(&wb.db, &qid).unwrap().state,
        crate::cards::CardState::Answered
    );
}

#[test]
fn taken_over_cards_leave_the_owner_view_and_return_when_off() {
    let (_dir, wb, _chat) = wb_with(None);
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    let ids =
        |cards: Vec<crate::cards::QueuedCard>| cards.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(
        ids(owner_pending(&wb.db, crate::PROJECT_ID).unwrap()),
        vec![qid.clone()]
    );
    assert!(managed_processing(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    assert_eq!(
        ids(managed_processing(&wb.db, crate::PROJECT_ID).unwrap()),
        vec![qid.clone()]
    );
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    assert_eq!(
        ids(owner_pending(&wb.db, crate::PROJECT_ID).unwrap()),
        vec![qid]
    );
}

#[test]
fn the_owner_pause_stops_new_decisions_and_late_commits() {
    let (_dir, mut wb, chat) = wb_with(Some("allow"));
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    orchestra::pause(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(
        wb.managed_plan().unwrap().is_empty(),
        "no new decisions while paused"
    );
    // Paused cards return to the owner rather than sit under "processing".
    assert_eq!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().len(), 1);
    assert!(managed_processing(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
    let late = ManagedDecision {
        action: "allow".into(),
        reason: "model_choice".into(),
        via: "model".into(),
    };
    assert!(wb.managed_commit(&plan[0], late).unwrap().is_none());
    assert_eq!(
        crate::cards::get(&wb.db, &qid).unwrap().state,
        crate::cards::CardState::Queued
    );
    assert!(chat.unwrap().recorded().is_empty());
}

fn resolution_actor(wb: &Workbench, action: &str) -> String {
    wb.db
        .conn()
        .query_row(
            "SELECT actor FROM action_resolutions WHERE action_id=?1",
            [action],
            |r| r.get(0),
        )
        .unwrap()
}

fn queued_where(wb: &Workbench, f: impl Fn(&crate::cards::QueuedCard) -> bool) -> usize {
    crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .filter(|c| f(c))
        .count()
}

/// Test3/18: the model chooses one linked new attempt for an unknown write.
/// The attempt runs outside the original activation, so the edit guard
/// refuses it as an ordinary failure — feedback, not a blocker — and the
/// original task continues without any owner check.
#[cfg(unix)]
#[test]
fn an_unknown_action_gets_one_new_attempt_and_the_task_continues() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, action) = super::super::tests::unknown_write_in_a_turn(dir.path());
    let chat = Arc::new(ScriptedProvider::new(vec![
        text_response("new_attempt"),
        text_response("will re-read and edit"),
    ]));
    wb.register_provider("default", chat.clone());
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let first = wb.managed_tick().unwrap();
    assert_eq!(first[0].action, "new_attempt", "{first:?}");
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(resolution_actor(&wb, &action), actor, "never `owner`");
    let next = crate::actions::resolution(&wb.db, &action)
        .unwrap()
        .unwrap()
        .2
        .unwrap();
    assert_eq!(
        crate::actions::get(&wb.db, crate::PROJECT_ID, &next)
            .unwrap()
            .state,
        "failed"
    );
    assert_eq!(
        queued_where(&wb, |c| c.payload["sub"] == "tool_outcome_unknown"),
        0
    );
    // The continue entry is the processor's next decision, not an owner card.
    assert!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    let second = wb.managed_tick().unwrap();
    assert_eq!(second[0].action, "continue", "{second:?}");
    assert_eq!(second[0].via, "host");
    assert_eq!(chat.recorded().len(), 2, "one decision + one continuation");
    assert_eq!(queued_where(&wb, |_| true), 0);
}

/// Test3/18: the one new attempt is itself unknown. There is never a second
/// attempt: host rule abandons it, which also resolves the origin, and the
/// original task continues.
#[cfg(unix)]
#[test]
fn a_new_attempt_that_is_unknown_again_is_abandoned_without_a_second_try() {
    let dir = tempfile::tempdir().unwrap();
    let (mut wb, action) = super::super::tests::unknown_write_in_a_turn(dir.path());
    // The same state the product reaches when a dispatched attempt's effect
    // becomes uncertain: linked pending attempt -> unknown, origin card closed.
    let ctx = wb.ctx_for("a0", None);
    let next = crate::actions::new_owner_attempt(&wb.db, &ctx, &action, "retry", true, "owner")
        .unwrap()
        .id;
    wb.db
        .conn()
        .execute(
            "UPDATE tool_actions SET state='unknown' WHERE id=?1",
            [&next],
        )
        .unwrap();
    crate::actions::expose_unknown(
        &wb.db,
        &ctx,
        &next,
        "fs_write",
        None,
        std::time::Instant::now(),
    )
    .unwrap();
    crate::actions::close_resolved_card(&wb.db, &ctx, &action).unwrap();
    let chat = Arc::new(ScriptedProvider::new(vec![text_response(
        "continued without the write",
    )]));
    wb.register_provider("default", chat.clone());
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let first = wb.managed_tick().unwrap();
    assert_eq!(first[0].action, "abandon", "{first:?}");
    assert_eq!(first[0].reason, "new_attempt_already_used");
    assert!(
        chat.recorded().is_empty(),
        "the forced abandon asks no model"
    );
    // Abandoning the linked attempt resolves the origin: its task continues.
    let second = wb.managed_tick().unwrap();
    assert_eq!(second[0].action, "continue", "{second:?}");
    assert_eq!(chat.recorded().len(), 1);
    let attempts: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM action_resolutions WHERE decision='new_attempt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1, "never a second new attempt");
}

/// Test3/18: a saved permission that provably never ran (the project was
/// paused) is resumed by the host once the pause ends, then the task goes on.
#[test]
fn a_ready_action_is_resumed_after_the_pause_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_scoped(
        dir.path(),
        "managed",
        &[("a0".into(), "worker".into())],
        None,
        false,
    )
    .unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![crate::turn::tool_response(
            vec![("c1", "bash", json!({"cmd":"printf ready > ready.txt"}))],
        )])),
    );
    let TurnOutcome::AwaitingPermission(qid) = wb.run_instance("a0", "write the marker").unwrap()
    else {
        panic!("the permission must come from the real task")
    };
    orchestra::pause(&wb.db, crate::PROJECT_ID).unwrap();
    wb.permission_control()
        .save(&wb.db, &qid, true, None, "activation")
        .unwrap();
    assert!(
        wb.continue_saved_permission(&qid).is_err(),
        "paused: not executed"
    );
    assert_eq!(
        queued_where(&wb, |c| c.payload["sub"] == "tool_action_ready"),
        1
    );
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(
        wb.managed_tick().unwrap().is_empty(),
        "nothing while paused"
    );
    assert!(!dir.path().join("ready.txt").exists());
    orchestra::resume(&wb.db, crate::PROJECT_ID).unwrap();
    let chat = Arc::new(ScriptedProvider::new(vec![text_response("marker written")]));
    wb.register_provider("default", chat.clone());
    let decisions = wb.managed_tick().unwrap();
    assert_eq!(decisions[0].action, "resume", "{decisions:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ready.txt")).unwrap(),
        "ready"
    );
    assert_eq!(chat.recorded().len(), 1, "the original task continued once");
}

/// Test3/23: an unregistered card type is taken over and ended as
/// incomplete — never left with the owner and never approved by default.
#[test]
fn an_unregistered_card_type_is_ended_not_approved() {
    let (_dir, mut wb, chat) = wb_with(Some("allow"));
    crate::cards::enqueue(
        &wb.db,
        crate::PROJECT_ID,
        Some("a0"),
        crate::cards::CardKind::Recovery,
        json!({"sub":"from_a_future_version"}),
        None,
    )
    .unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("end", "unrouted_card_type")
    );
    assert!(chat.unwrap().recorded().is_empty());
    assert_eq!(
        managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap()[0].outcome,
        "incomplete"
    );
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
}

/// Test3/23: delegation survives a restart. A run interrupted by the crash is
/// resumed by host rule on reopen and its task continues — without replaying
/// a completed tool; a decision prepared before the restart cannot land.
#[test]
fn reopening_a_delegated_project_resumes_interrupted_work() {
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"resume","version":1,
        "stages":[{"name":"build","roles":["worker"],"due":["规格"],"stamp_point":true}]}))
    .unwrap();
    let roles = [("a0".to_string(), "worker".to_string())];
    let (run, stale) = {
        let mut wb =
            Workbench::open_scoped(dir.path(), "resume", &roles, Some(pack.clone()), false)
                .unwrap();
        orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
        wb.register_provider(
            "default",
            Arc::new(ScriptedProvider::new(vec![text_response(
                "started the build",
            )])),
        );
        wb.open_stage(0).unwrap();
        wb.run_instance("a0", "build it").unwrap();
        let run = wb.active_run().unwrap().unwrap().id;
        crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
        // The process dies mid-turn: a turn started and never finished.
        wb.db
            .append_event(
                crate::PROJECT_ID,
                EventKind::TurnStarted,
                json!({"instruction":"build it"}),
                Some("a0"),
                Some(&run),
            )
            .unwrap();
        let stale = wb.managed_plan().unwrap();
        (run, stale)
    };
    let mut wb = Workbench::open_scoped(dir.path(), "resume", &roles, Some(pack), false).unwrap();
    let chat = Arc::new(ScriptedProvider::new(vec![text_response(
        "continued the build",
    )]));
    wb.register_provider("default", chat.clone());
    assert!(
        crate::delegation::current(&wb.db, crate::PROJECT_ID).is_some(),
        "delegation persists"
    );
    // Turning it off and on again in between invalidates the old decision.
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let late = ManagedDecision {
        action: "resume_run".into(),
        reason: "stale".into(),
        via: "host".into(),
    };
    if let Some(old) = stale.first() {
        assert!(wb.managed_commit(old, late).unwrap().is_none());
    }
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "resume_run", "{done:?}");
    assert_eq!(wb.active_run().unwrap().unwrap().id, run);
    assert_eq!(chat.recorded().len(), 1, "the original task continued once");
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
}

/// Review fix: one unreadable card ends incomplete on its own and does not
/// stall the decisions for every other card.
#[test]
fn an_unreadable_card_ends_incomplete_without_blocking_others() {
    let (_dir, mut wb, _chat) = wb_with(Some("allow"));
    let good = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    crate::cards::enqueue(
        &wb.db,
        crate::PROJECT_ID,
        Some("a0"),
        crate::cards::CardKind::Recovery,
        json!({"sub":"tool_outcome_unknown","action_id":"missing-action"}),
        None,
    )
    .unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert!(
        done.iter().any(|d| d.reason == "route_unavailable"),
        "{done:?}"
    );
    assert!(done.iter().any(|d| d.action == "allow"), "{done:?}");
    assert_ne!(
        crate::cards::get(&wb.db, &good).unwrap().state,
        crate::cards::CardState::Queued
    );
    let history = managed_history(&wb.db, crate::PROJECT_ID, 10).unwrap();
    assert!(history.iter().any(|r| r.result == "incomplete"));
    // The allowed bash leaves its own ready card (the test agent is asleep);
    // only the unreadable card must be gone.
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .iter()
        .all(|c| c.payload["action_id"] != "missing-action"));
}

/// 2026-10-11 UI alignment: pausing delegation keeps the grant, stops new
/// decisions and returns taken-over cards to the owner; resuming takes them
/// back under the same grant id with a new generation.
#[test]
fn pausing_delegation_returns_cards_and_resume_keeps_the_grant() {
    let (_dir, mut wb, chat) = wb_with(Some("allow"));
    let qid = ask(&wb, "bash", json!({"cmd":"echo hi"}));
    let on = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    let paused = crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, true).unwrap();
    assert!(paused.enabled && paused.paused);
    assert!(crate::delegation::current(&wb.db, crate::PROJECT_ID).is_none());
    assert_eq!(
        owner_pending(&wb.db, crate::PROJECT_ID).unwrap().len(),
        1,
        "back with the owner"
    );
    assert!(wb.managed_tick().unwrap().is_empty());
    let late = ManagedDecision {
        action: "allow".into(),
        reason: "model_choice".into(),
        via: "model".into(),
    };
    assert!(
        wb.managed_commit(&plan[0], late).unwrap().is_none(),
        "pre-pause decision never lands"
    );
    let resumed = crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, false).unwrap();
    assert_eq!(resumed.delegation_id, on.delegation_id, "same grant");
    assert!(resumed.generation > paused.generation);
    assert!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    assert_eq!(wb.managed_tick().unwrap()[0].action, "allow");
    assert_ne!(
        crate::cards::get(&wb.db, &qid).unwrap().state,
        crate::cards::CardState::Queued
    );
    assert_eq!(chat.unwrap().recorded().len(), 1);
}
