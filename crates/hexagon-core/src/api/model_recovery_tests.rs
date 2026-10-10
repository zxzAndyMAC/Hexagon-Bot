//! MR-02/MR-03 regressions through the real Workbench facade: role model
//! failures continue the failed call position; PM failures classify and
//! budget on the injectable clock (test3 tickets 05–07 and their review).
use super::*;
use crate::provider::{ChatRequest, ChatResponse, ModelProvider, ProviderError, StreamDelta};
use crate::turn::{text_response, tool_response};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

enum Step {
    Reply(ChatResponse),
    /// Stream this text, then fail: the unfinished-output case.
    Partial(&'static str, ProviderError),
    Fail(ProviderError),
}

/// Scripted provider that records every request it actually received and can
/// advance the workbench clock on each call (budget tests never sleep).
#[derive(Default)]
struct Faulty {
    steps: Mutex<VecDeque<Step>>,
    sent: Mutex<Vec<ChatRequest>>,
    decides: Mutex<u32>,
    decision_api: bool,
    clock: Option<(Arc<TestClock>, Duration)>,
}

impl Faulty {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            ..Default::default()
        }
    }
    fn deciding(steps: Vec<Step>) -> Self {
        Self {
            decision_api: true,
            ..Self::new(steps)
        }
    }
    fn sent(&self) -> Vec<ChatRequest> {
        self.sent.lock().unwrap().clone()
    }
    fn decides(&self) -> u32 {
        *self.decides.lock().unwrap()
    }
    fn next(&self) -> Option<Step> {
        if let Some((clock, step)) = &self.clock {
            clock.advance(*step);
        }
        self.steps.lock().unwrap().pop_front()
    }
}

impl ModelProvider for Faulty {
    fn is_scripted(&self) -> bool {
        true
    }
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.stream(req, &mut |_| true)
    }
    fn stream(
        &self,
        req: &ChatRequest,
        sink: &mut crate::provider::StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        self.sent.lock().unwrap().push(req.clone());
        match self.next() {
            Some(Step::Reply(response)) => Ok(response),
            Some(Step::Partial(text, error)) => {
                sink(&StreamDelta::Text(text.into()));
                Err(error)
            }
            Some(Step::Fail(error)) => Err(error),
            None => Err(ProviderError::ScriptExhausted),
        }
    }
    fn uses_decision_api(&self) -> bool {
        self.decision_api
    }
    fn decide(
        &self,
        _state: &str,
        _options: &[(&str, &str)],
    ) -> Result<ChatResponse, ProviderError> {
        *self.decides.lock().unwrap() += 1;
        match self.next() {
            Some(Step::Reply(response)) => Ok(response),
            Some(Step::Fail(error)) | Some(Step::Partial(_, error)) => Err(error),
            None => Err(ProviderError::ScriptExhausted),
        }
    }
}

struct TestClock(Mutex<Instant>);

impl TestClock {
    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl crate::stallwatch::StallClock for TestClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

fn http(status: u16) -> ProviderError {
    let cause = if status == 429 || status >= 500 {
        ProviderError::Transport(format!("HTTP {status}"))
    } else {
        ProviderError::Refused(format!("HTTP {status}"))
    };
    ProviderError::HttpStatus(Box::new(cause), status, None)
}

fn fast_policy(budget: Duration) -> crate::turn::WaitPolicy {
    crate::turn::WaitPolicy {
        probe_interval: Duration::from_millis(1),
        budget,
        tick: Duration::from_millis(1),
    }
}

fn worker_wb(dir: &Path) -> Workbench {
    let wb = Workbench::for_test(dir, &["worker"], None).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb
}

fn turn_card(wb: &Workbench) -> crate::cards::QueuedCard {
    crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .into_iter()
        .find(|card| card.payload["sub"] == "model_request" && card.payload["purpose"] == "turn")
        .expect("role model-request item")
}

fn view(wb: &Workbench, qid: &str) -> ModelRequestRecovery {
    model_request_recovery(&wb.db, &wb.project_id, qid).unwrap()
}

fn request_text(req: &ChatRequest) -> String {
    serde_json::to_string(&req.messages).unwrap()
}

#[test]
fn first_token_auth_failure_is_immediate_and_retry_resends_the_exact_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let first = Arc::new(Faulty::new(vec![Step::Fail(http(401))]));
    wb.register_provider("default", first.clone());
    assert!(wb.run_instance("a0", "Summarize the plan.").is_err());
    assert_eq!(
        first.sent().len(),
        1,
        "authentication never waits for the network budget"
    );
    let card = turn_card(&wb);
    let failed = view(&wb, &card.id);
    assert_eq!(failed.history[0].code, "authentication");
    assert!(
        failed.history[0].request_id.is_some(),
        "each send keeps its ledger id"
    );
    assert!(!failed.unfinished, "no streamed text, nothing unfinished");

    let next = Arc::new(Faulty::new(vec![Step::Reply(text_response(
        "Plan summarized.",
    ))]));
    wb.register_provider("default", next.clone());
    wb.retry_model_request(&card.id).unwrap();
    assert_eq!(
        request_text(&next.sent()[0]),
        request_text(&first.sent()[0])
    );
    assert_eq!(view(&wb, &card.id).state, "recovered");
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
}

#[test]
fn planning_failure_resumes_the_saved_planning_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let first = Arc::new(Faulty::new(vec![Step::Fail(http(403))]));
    wb.register_provider("default", first.clone());
    assert!(wb.dispatch("worker", "Write the spec.", &[]).is_err());
    let card = turn_card(&wb);
    let next = Arc::new(Faulty::new(vec![
        Step::Reply(text_response("Plan: outline then write.")),
        Step::Reply(text_response("Spec written.")),
    ]));
    wb.register_provider("default", next.clone());
    wb.retry_model_request(&card.id).unwrap();
    let sent = next.sent();
    assert!(
        sent[0].tools.is_empty(),
        "the planning call is resent, not the tool loop"
    );
    assert_eq!(request_text(&sent[0]), request_text(&first.sent()[0]));
    assert_eq!(view(&wb, &card.id).state, "recovered");
}

#[test]
fn partial_reply_is_unfinished_and_never_joins_the_new_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let cut = ProviderError::Refused("stream cut".into());
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![Step::Partial("half an answ", cut)])),
    );
    assert!(wb.run_instance("a0", "Explain the design.").is_err());
    let card = turn_card(&wb);
    let failed = view(&wb, &card.id);
    assert!(failed.unfinished);
    assert_eq!(failed.partial_text.as_deref(), Some("half an answ"));
    let leaked: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE body LIKE '%half an answ%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0, "unfinished text is not a formal reply");

    let next = Arc::new(Faulty::new(vec![Step::Reply(text_response(
        "The complete answer.",
    ))]));
    wb.register_provider("default", next);
    wb.retry_model_request(&card.id).unwrap();
    let bodies: Vec<String> = {
        let mut stmt = wb.db.conn().prepare("SELECT body FROM messages").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert!(bodies.iter().any(|b| b == "The complete answer."));
    assert!(
        bodies.iter().all(|b| !b.contains("half an answ")),
        "no stitching of old text"
    );
    let recovered = view(&wb, &card.id);
    assert_eq!(
        recovered.history.len(),
        1,
        "the failed attempt stays in history"
    );
}

#[test]
fn retry_of_retry_continues_from_the_newest_position_without_replaying_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let write = |id: &str, path: &str| {
        Step::Reply(tool_response(vec![(
            id,
            "fs_write",
            json!({"path":path,"content":id}),
        )]))
    };
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![
            write("w1", "one.txt"),
            Step::Fail(http(401)),
        ])),
    );
    assert!(wb
        .run_instance("a0", "Write one.txt and two.txt, then report.")
        .is_err());
    let card = turn_card(&wb);
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![
            write("w2", "two.txt"),
            Step::Fail(http(401)),
        ])),
    );
    assert!(wb.retry_model_request(&card.id).is_err());
    assert_eq!(
        view(&wb, &card.id).history.len(),
        2,
        "same item, second attempt"
    );
    let last = Arc::new(Faulty::new(vec![Step::Reply(text_response(
        "Both written.",
    ))]));
    wb.register_provider("default", last.clone());
    wb.retry_model_request(&card.id).unwrap();
    let sent = request_text(&last.sent()[0]);
    assert!(
        sent.contains("w1") && sent.contains("w2"),
        "both confirmed results continue"
    );
    let writes: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_actions WHERE tool='fs_write' AND state='succeeded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(writes, 2, "each successful write happened exactly once");
    assert_eq!(view(&wb, &card.id).state, "recovered");
}

#[test]
fn exhausted_transient_role_failure_keeps_its_item_not_stage_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(
        json!({"name":"t","version":1,"stages":[{"name":"build","roles":["worker"],"due":[]}]}),
    )
    .unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    wb.wait_policy = fast_policy(Duration::from_millis(20));
    let down = (0..64).map(|_| Step::Fail(ProviderError::Transport("unexpected EOF".into())));
    wb.register_provider("default", Arc::new(Faulty::new(down.collect())));
    assert!(wb.run_instance("a0", "Build it.").is_err());
    let card = turn_card(&wb);
    assert_eq!(view(&wb, &card.id).history[0].code, "transport");
    assert_eq!(view(&wb, &card.id).history[0].message, "unexpected EOF");
    assert!(wb.active_run().unwrap().is_some(), "run stays active");
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .all(|c| c.payload["reason"] != "network_timeout"));
}

fn failed_turn(wb: &mut Workbench) -> crate::cards::QueuedCard {
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![Step::Fail(http(401))])),
    );
    assert!(wb.run_instance("a0", "Do the task.").is_err());
    turn_card(wb)
}

fn refused(wb: &mut Workbench, qid: &str) -> (String, usize) {
    let probe = Arc::new(Faulty::new(vec![Step::Reply(text_response("done"))]));
    wb.register_provider("default", probe.clone());
    let error = wb.retry_model_request(qid).unwrap_err().to_string();
    (error, probe.sent().len())
}

#[test]
fn corrupt_or_missing_material_refuses_without_a_generic_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let card = failed_turn(&mut wb);
    let checkpoint = card.payload["checkpoint_event_id"].as_i64().unwrap();
    wb.db
        .conn()
        .execute(
            "UPDATE events SET payload='{\"kind\":\"model_request_checkpoint\"}' WHERE id=?1",
            [checkpoint],
        )
        .unwrap();
    let (error, sends) = refused(&mut wb, &card.id);
    assert!(error.contains("native_material_unavailable"), "{error}");
    assert_eq!(sends, 0);
    crate::cards::annotate(&wb.db, &card.id, &[("checkpoint_event_id", Value::Null)]).unwrap();
    let (error, sends) = refused(&mut wb, &card.id);
    assert!(error.contains("checkpoint_missing"), "{error}");
    assert_eq!(sends, 0);
    assert_eq!(turn_card(&wb).id, card.id, "the item stays for the owner");
}

#[test]
fn unknown_tool_outcome_blocks_model_continuation() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let card = failed_turn(&mut wb);
    crate::cards::enqueue(
        &wb.db,
        &wb.project_id,
        Some("a0"),
        crate::cards::CardKind::Recovery,
        json!({"sub":"tool_outcome_unknown","action_id":"x1"}),
        None,
    )
    .unwrap();
    let (error, sends) = refused(&mut wb, &card.id);
    assert!(error.contains("tool_outcome_unknown"), "{error}");
    assert_eq!(sends, 0);
}

#[test]
fn paused_project_and_concurrent_retry_never_send() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let card = failed_turn(&mut wb);
    let origin = card.payload["packet"]["origin_turn_id"].as_i64().unwrap();
    let held = wb
        .lock_request(&format!("turn-{origin}"), "a0", None)
        .unwrap();
    let (error, sends) = refused(&mut wb, &card.id);
    assert!(error.contains("already_running"), "{error}");
    assert_eq!(sends, 0);
    drop(held);
    orchestra::pause(&wb.db, &wb.project_id).unwrap();
    let (error, sends) = refused(&mut wb, &card.id);
    assert!(error.contains("paused"), "{error}");
    assert_eq!(sends, 0);
}

fn pm_wb(dir: &Path) -> Workbench {
    let pack: PackDef = serde_json::from_value(json!({"name":"t","version":1,
        "stages":[{"name":"spec","roles":["产品策划"],"due":["规格"],"stamp_point":true}]}))
    .unwrap();
    let wb = Workbench::for_test(dir, &["项目经理", "产品策划"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    wb
}

fn send(wb: &Workbench, body: &str) -> Result<UnnamedRoute, ApiError> {
    let (id, _) = crate::commands::send_via_control(&wb.db, &wb.project_id, body, &[]).unwrap();
    wb.route_queued_owner(id)
}

fn pm_card(wb: &Workbench) -> crate::cards::QueuedCard {
    crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .into_iter()
        .find(|card| card.payload["purpose"] == "pm_route")
        .expect("PM model-request item")
}

#[test]
fn pm_authentication_failure_points_to_settings_without_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    wb.wait_policy = fast_policy(Duration::from_secs(300));
    let decision = Arc::new(Faulty::deciding(vec![Step::Fail(http(401))]));
    wb.register_provider("decision", decision.clone());
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    assert!(send(&wb, "Start").is_err());
    assert_eq!(decision.decides(), 1);
    let item = view(&wb, &pm_card(&wb).id);
    assert_eq!(item.state, "failed");
    assert_eq!(item.history[0].code, "authentication");
}

#[test]
fn pm_server_error_is_provider_unavailable_and_retried_in_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    wb.wait_policy = fast_policy(Duration::from_secs(300));
    let decision = Arc::new(Faulty::deciding(vec![
        Step::Fail(http(503)),
        Step::Reply(text_response("HOLD")),
    ]));
    wb.register_provider("decision", decision.clone());
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    send(&wb, "Hold for now").unwrap();
    let qid = events_of(&wb, "model_request_failed")[0]["recovery_id"]
        .as_str()
        .unwrap()
        .to_string();
    let item = view(&wb, &qid);
    assert_eq!(item.history[0].code, "provider_unavailable");
    assert_eq!(item.state, "recovered");
}

fn events_of(wb: &Workbench, kind: &str) -> Vec<Value> {
    let mut stmt = wb
        .db
        .conn()
        .prepare("SELECT payload FROM events WHERE kind='system' AND json_extract(payload,'$.kind')=?1 ORDER BY id")
        .unwrap();
    stmt.query_map([kind], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|p| serde_json::from_str(&p.unwrap()).unwrap())
        .collect()
}

#[test]
fn pm_budget_runs_on_the_workbench_clock_and_manual_retry_gets_a_fresh_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    wb.wait_policy = fast_policy(Duration::from_secs(300));
    let clock = Arc::new(TestClock(Mutex::new(Instant::now())));
    wb.stall_clock = clock.clone();
    let eof = || Step::Fail(ProviderError::Transport("unexpected EOF".into()));
    let decision = Arc::new(Faulty {
        clock: Some((clock.clone(), Duration::from_secs(40))),
        ..Faulty::deciding(vec![
            eof(),
            eof(),
            eof(),
            Step::Reply(text_response("HOLD")),
        ])
    });
    wb.register_provider("decision", decision.clone());
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    // Owner window is 60 s: 40 s, then 80 s exhausts it — two sends, no third.
    assert!(send(&wb, "Hold for now").is_err());
    assert_eq!(decision.decides(), 2);
    let card = pm_card(&wb);
    assert_eq!(view(&wb, &card.id).state, "failed");
    assert!(
        events_of(&wb, "model_request_retrying")
            .iter()
            .all(|e| e["retry_deadline_ms"].as_i64().is_some()),
        "retrying shows its window"
    );
    // Manual retry gets a fresh 60 s window: a failure at 40 s may retry once
    // more, and that second send succeeds inside the new window.
    wb.retry_model_request(&card.id).unwrap();
    assert_eq!(decision.decides(), 4, "fresh window allowed two more sends");
    assert_eq!(view(&wb, &card.id).state, "recovered");
}

#[test]
fn stall_investigation_failure_keeps_the_stall_contract() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    let decision = Arc::new(Faulty::deciding(vec![Step::Fail(http(503))]));
    wb.register_provider("decision", decision.clone());
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    let roster = wb.roster().unwrap();
    let origin = model_recovery::ChoiceOrigin {
        stall_investigation: true,
        ..Default::default()
    };
    let result = wb.closed_choice(
        crate::stallwatch::INVESTIGATION_SPEAKER,
        "Nothing moved.",
        &[],
        &roster,
        None,
        origin,
    );
    assert!(result.is_err());
    assert_eq!(
        decision.decides(),
        1,
        "one bounded attempt, no five-minute loop"
    );
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .all(|card| card.payload["sub"] != "model_request"));
}

// ---- MR-04 (test3 08): reopen and supersession -------------------------

fn open_worker(dir: &Path) -> Workbench {
    let wb = Workbench::open(dir, "t", &[("a0".into(), "worker".into())], None).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb
}

#[test]
fn reopen_restores_the_role_item_and_sends_nothing_until_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = open_worker(dir.path());
    let qid = failed_turn(&mut wb).id;
    drop(wb);
    let mut reopened = open_worker(dir.path());
    let probe = Arc::new(Faulty::new(vec![Step::Reply(text_response("done"))]));
    reopened.register_provider("default", probe.clone());
    assert_eq!(probe.sent().len(), 0, "opening a project sends nothing");
    let item = view(&reopened, &qid);
    assert_eq!(item.state, "failed");
    assert_eq!(item.instruction.as_deref(), Some("Do the task."));
    reopened.retry_model_request(&qid).unwrap();
    assert_eq!(probe.sent().len(), 1);
    assert_eq!(view(&reopened, &qid).state, "recovered");
}

#[test]
fn an_attempt_in_flight_at_exit_reopens_as_unknown_and_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = open_worker(dir.path());
    let qid = failed_turn(&mut wb).id;
    // Process ended mid-retry: the durable state still says `retrying`.
    crate::cards::annotate(&wb.db, &qid, &[("state", json!("retrying"))]).unwrap();
    drop(wb);
    let reopened = open_worker(dir.path());
    let item = view(&reopened, &qid);
    assert_eq!(
        item.state, "unknown",
        "not retrying, not failed-before-send"
    );
    assert!(item.can_retry);
    assert_eq!(item.history.len(), 1, "no attempt is invented or erased");
}

fn owner_says(wb: &Workbench, body: &str) -> Result<UnnamedRoute, ApiError> {
    send(wb, body)
}

#[test]
fn a_new_owner_instruction_to_the_failed_role_waits_for_an_explicit_choice() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let qid = failed_turn(&mut wb).id;
    // Following the new instruction is a planned dispatch: plan, then reply.
    let probe = Arc::new(Faulty::new(vec![
        Step::Reply(text_response("plan")),
        Step::Reply(text_response("new work done")),
    ]));
    wb.register_provider("default", probe.clone());
    owner_says(&wb, "@worker write the changelog instead").unwrap();
    assert_eq!(probe.sent().len(), 0, "neither task runs before the choice");
    let item = view(&wb, &qid);
    assert_eq!(item.state, "needs_choice");
    assert_eq!(
        item.held_instruction.as_deref(),
        Some("@worker write the changelog instead")
    );
    assert!(!item.can_retry, "plain retry cannot pick for the owner");

    wb.resolve_model_request_choice(&qid, true).unwrap();
    let sent = request_text(&probe.sent()[0]);
    assert!(
        sent.contains("write the changelog instead"),
        "the new instruction runs"
    );
    let retired = view(&wb, &qid);
    assert_eq!(retired.state, "superseded");
    assert_eq!(
        retired.superseded_reason.as_deref(),
        Some("owner_chose_new")
    );
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .all(|c| c.id != qid));
    assert!(
        wb.retry_model_request(&qid).is_err(),
        "a retired entry never sends again"
    );
}

#[test]
fn choosing_the_original_continues_it_and_leaves_the_new_instruction_unexecuted() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let qid = failed_turn(&mut wb).id;
    let probe = Arc::new(Faulty::new(vec![Step::Reply(text_response(
        "original done",
    ))]));
    wb.register_provider("default", probe.clone());
    owner_says(&wb, "@worker also check the docs").unwrap();
    wb.resolve_model_request_choice(&qid, false).unwrap();
    assert_eq!(probe.sent().len(), 1);
    assert!(!request_text(&probe.sent()[0]).contains("also check the docs"));
    assert_eq!(view(&wb, &qid).state, "recovered");
}

fn two_stage_pm_wb(dir: &Path) -> Workbench {
    let pack: PackDef = serde_json::from_value(json!({"name":"t","version":1,"stages":[
        {"name":"spec","roles":["产品策划"],"due":[]},
        {"name":"build","roles":["产品策划"],"due":[]}]}))
    .unwrap();
    let wb = Workbench::for_test(dir, &["项目经理", "产品策划"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    let writer = wb.agent_by_role("产品策划").unwrap();
    orchestra::write_agent_status(&wb.db, "p1", &writer, false).unwrap();
    wb
}

fn failed_writer(wb: &mut Workbench) -> String {
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![Step::Fail(http(401))])),
    );
    let writer = wb.agent_by_role("产品策划").unwrap();
    assert!(wb.run_instance(&writer, "Draft the spec.").is_err());
    turn_card(wb).id
}

#[test]
fn an_ordinary_question_keeps_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = two_stage_pm_wb(dir.path());
    let qid = failed_writer(&mut wb);
    let decision = Arc::new(Faulty::deciding(vec![Step::Reply(text_response("HOLD"))]));
    wb.register_provider("decision", decision);
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    let probe = Arc::new(Faulty::new(vec![]));
    wb.register_provider("default", probe.clone());
    owner_says(&wb, "How is it going?").unwrap();
    assert_eq!(probe.sent().len(), 0);
    assert_eq!(
        view(&wb, &qid).state,
        "failed",
        "not cancelled by a question"
    );
}

#[test]
fn a_stage_change_retires_the_entry_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = two_stage_pm_wb(dir.path());
    let qid = failed_writer(&mut wb);
    wb.open_stage(1).unwrap();
    let (error, sends) = refused(&mut wb, &qid);
    assert!(error.contains("stage_changed"), "{error}");
    assert_eq!(sends, 0);
    let item = view(&wb, &qid);
    assert_eq!(item.state, "superseded");
    assert_eq!(item.superseded_reason.as_deref(), Some("stage_changed"));
}

#[test]
fn a_newer_task_for_the_role_supersedes_the_saved_position() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let qid = failed_turn(&mut wb).id;
    wb.register_provider(
        "default",
        Arc::new(Faulty::new(vec![
            Step::Reply(text_response("plan")),
            Step::Reply(text_response("other task done")),
        ])),
    );
    // A background dispatch (not an owner instruction) runs immediately.
    wb.dispatch("worker", "Run the nightly check.", &[])
        .unwrap();
    let (error, sends) = refused(&mut wb, &qid);
    assert!(error.contains("superseded_by_new_task"), "{error}");
    assert_eq!(sends, 0);
    assert_eq!(view(&wb, &qid).state, "superseded");
}

#[test]
fn a_transient_refusal_keeps_the_entry_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = worker_wb(dir.path());
    let qid = failed_turn(&mut wb).id;
    orchestra::pause(&wb.db, &wb.project_id).unwrap();
    let (error, _) = refused(&mut wb, &qid);
    assert!(error.contains("paused"));
    let item = view(&wb, &qid);
    assert_eq!(item.state, "failed");
    assert_eq!(item.last_refusal.as_deref(), Some("paused"));
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .any(|c| c.id == qid));
}

// ---- MR-05 (test3 09): explicit one-off chat-model retry ----------------

fn decision_failed(wb: &mut Workbench) -> (Arc<Faulty>, String) {
    let decision = Arc::new(Faulty::deciding(vec![
        Step::Fail(http(401)),
        Step::Reply(text_response("HOLD")),
    ]));
    wb.register_provider("decision", decision.clone());
    wb.set_decision_slot("项目经理", Some("decision")).unwrap();
    assert!(send(wb, "Hold for now").is_err());
    (decision, pm_card(wb).id)
}

#[test]
fn chat_model_retry_completes_the_same_choice_once_and_changes_no_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    let (decision, qid) = decision_failed(&mut wb);
    assert_eq!(view(&wb, &qid).chat_model_slot.as_deref(), Some("default"));
    let chat = Arc::new(Faulty::new(vec![Step::Reply(text_response("HOLD"))]));
    wb.register_provider("default", chat.clone());
    assert!(matches!(
        wb.retry_model_request_with_chat_model(&qid).unwrap(),
        UnnamedRoute::Held { .. }
    ));
    assert_eq!(
        chat.sent().len(),
        1,
        "one chat send for the same closed choice"
    );
    assert_eq!(
        decision.decides(),
        1,
        "the decision model was not called again"
    );
    let item = view(&wb, &qid);
    assert_eq!(item.state, "recovered");
    assert_eq!(
        item.history[0].model_slot, "decision",
        "history keeps the failed attempt"
    );
    let slot: Option<String> = wb
        .db
        .conn()
        .query_row(
            "SELECT decision_slot FROM agents WHERE role='项目经理'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        slot.as_deref(),
        Some("decision"),
        "no slot configuration changed"
    );
    send(&wb, "Hold again").unwrap();
    assert_eq!(
        decision.decides(),
        2,
        "later tasks still use the decision model"
    );
    assert_eq!(chat.sent().len(), 1);
}

#[test]
fn chat_model_retry_refuses_a_decision_only_transport_and_keeps_the_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    let (_, qid) = decision_failed(&mut wb);
    let not_chat = Arc::new(Faulty::deciding(vec![Step::Reply(text_response("HOLD"))]));
    wb.register_provider("default", not_chat.clone());
    let error = wb
        .retry_model_request_with_chat_model(&qid)
        .unwrap_err()
        .to_string();
    assert!(error.contains("chat_provider_unavailable"), "{error}");
    assert_eq!(not_chat.decides(), 0);
    let item = view(&wb, &qid);
    assert_eq!(item.state, "failed");
    assert_eq!(
        item.last_refusal.as_deref(),
        Some("chat_provider_unavailable")
    );
}

#[test]
fn chat_model_retry_does_not_apply_to_a_chat_slot_choice() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = pm_wb(dir.path());
    let chat = Arc::new(Faulty::new(vec![Step::Fail(http(401))]));
    wb.register_provider("default", chat.clone());
    assert!(send(&wb, "Hold for now").is_err());
    let qid = pm_card(&wb).id;
    assert_eq!(view(&wb, &qid).chat_model_slot, None);
    let error = wb
        .retry_model_request_with_chat_model(&qid)
        .unwrap_err()
        .to_string();
    assert!(error.contains("chat_fallback_not_applicable"), "{error}");
    assert_eq!(chat.sent().len(), 1);
}
