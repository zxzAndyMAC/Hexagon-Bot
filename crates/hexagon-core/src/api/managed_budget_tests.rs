//! Managed-followups ticket 04 (spec T5): delegation budgets model-request
//! items by task. Before, the key was the item id, so every new turn's item
//! for the same task opened three fresh rounds — no real upper bound.
use super::*;
use crate::provider::{ChatRequest, ChatResponse, ModelProvider, ProviderError};
use proptest::prelude::*;

/// Every request fails with an authentication error (no waiting, no reply).
struct Down;
impl ModelProvider for Down {
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
        Err(ProviderError::HttpStatus(
            Box::new(ProviderError::Refused("HTTP 401".into())),
            401,
            None,
        ))
    }
}

fn staged(dir: &Path) -> Workbench {
    let pack: PackDef = serde_json::from_value(
        json!({"name":"t","version":1,"stages":[{"name":"build","roles":["worker"],"due":[]}]}),
    )
    .unwrap();
    let mut wb = Workbench::for_test(dir, &["worker"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    wb.register_provider("default", Arc::new(Down));
    wb
}

fn open_turn_items(wb: &Workbench) -> Vec<crate::cards::QueuedCard> {
    crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .into_iter()
        .filter(|card| card.payload["sub"] == "model_request" && card.payload["purpose"] == "turn")
        .collect()
}

fn fail_a_turn(wb: &Workbench) -> crate::cards::QueuedCard {
    assert!(wb.run_instance("a0", "Build it.").is_err());
    let items = open_turn_items(wb);
    assert_eq!(items.len(), 1, "one open item per failed turn");
    items.into_iter().next().unwrap()
}

fn rounds(wb: &Workbench, key: &str) -> i64 {
    crate::delegation::rounds(&wb.db, &wb.project_id, key)
}

#[test]
fn a_stage_task_shares_three_rounds_across_items_and_then_ends_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = staged(dir.path());
    let run = wb.active_run().unwrap().unwrap().id;
    let first = fail_a_turn(&wb);
    assert_eq!(first.payload["task_key"], format!("run:{run}"));
    let key = format!("model_request:run:{run}");
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    for spent in 1..=3 {
        let done = wb.managed_tick().unwrap();
        assert_eq!(done[0].action, "retry", "{done:?}");
        assert_eq!(rounds(&wb, &key), spent);
    }
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("end", "budget_exhausted")
    );
    let history = managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap();
    assert_eq!(history[0].result, "incomplete", "reported as not done");
    assert!(open_turn_items(&wb).is_empty());
    // A new turn of the same task opens a new item, not a new budget —
    // not even after delegation is turned off and on again.
    let second = fail_a_turn(&wb);
    assert_ne!(second.id, first.id);
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("end", "budget_exhausted")
    );
}

#[test]
fn other_tasks_and_legacy_items_keep_their_own_budget() {
    let dir = tempfile::tempdir().unwrap();
    let wb = staged(dir.path());
    let run = wb.active_run().unwrap().unwrap().id;
    let item = fail_a_turn(&wb);
    for _ in 0..3 {
        crate::delegation::spend_round(&wb.db, &wb.project_id, &format!("model_request:run:{run}"))
            .unwrap();
    }
    // Same role, different task; and an item written before task keys.
    let packet = item.payload["packet"].clone();
    let other = crate::cards::enqueue(
        &wb.db,
        &wb.project_id,
        Some("a0"),
        crate::cards::CardKind::Recovery,
        json!({"sub":"model_request","purpose":"turn","state":"failed","history":[],
            "packet":packet,"task_key":"activation:9999"}),
        None,
    )
    .unwrap();
    let legacy = crate::cards::enqueue(
        &wb.db,
        &wb.project_id,
        Some("a0"),
        crate::cards::CardKind::Recovery,
        json!({"sub":"model_request","purpose":"turn","state":"failed","history":[],"packet":packet}),
        None,
    )
    .unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    let reason = |id: &str| {
        let candidate = plan.iter().find(|c| c.card_id == id).unwrap();
        (
            candidate.decision_key.clone(),
            candidate.forced.clone().unwrap().1,
        )
    };
    assert_eq!(reason(&item.id).1, "budget_exhausted");
    assert_eq!(
        reason(&other),
        (
            "model_request:activation:9999".into(),
            "bounded_model_retry".into()
        )
    );
    assert_eq!(
        reason(&legacy),
        (
            format!("model_request:{legacy}"),
            "bounded_model_retry".into()
        )
    );
}

#[test]
fn the_owner_explicit_retry_spends_no_delegation_round() {
    let dir = tempfile::tempdir().unwrap();
    let wb = staged(dir.path());
    let run = wb.active_run().unwrap().unwrap().id;
    let item = fail_a_turn(&wb);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(wb.retry_model_request(&item.id).is_err(), "still down");
    assert_eq!(rounds(&wb, &format!("model_request:run:{run}")), 0);
}

#[test]
fn a_role_outside_any_stage_budgets_by_its_activation() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.register_provider("default", Arc::new(Down));
    let first = fail_a_turn(&wb);
    let key = first.payload["task_key"].as_str().unwrap().to_string();
    assert!(key.starts_with("activation:"), "{key}");
    wb.retire_model_request_incomplete(&crate::cards::get(&wb.db, &first.id).unwrap())
        .unwrap();
    // A new dispatch is a new activation: a new task with its own budget.
    let second = fail_a_turn(&wb);
    assert_ne!(second.payload["task_key"], key.as_str());
}

#[test]
fn a_pm_dispatch_is_one_task_per_source_trace() {
    let card = |task: Option<&str>, id: &str| crate::cards::QueuedCard {
        id: id.into(),
        kind: "recovery".into(),
        agent_id: Some("a0".into()),
        payload: json!({"sub":"model_request","purpose":"pm_route","task_key":task}),
        state: "queued".into(),
    };
    assert_eq!(
        decision_key(&card(Some("dispatch:7"), "q1")),
        decision_key(&card(Some("dispatch:7"), "q2"))
    );
    assert_eq!(
        decision_key(&card(None, "q3")).as_deref(),
        Some("model_request:q3")
    );
}

proptest! {
    // Ticket 04: the budget key of a keyed item depends on its task only —
    // never on the item id — and distinct tasks never share a budget.
    #[test]
    fn model_request_budget_follows_the_task(a in "[a-z0-9:]{1,16}", b in "[a-z0-9:]{1,16}", ida in "q[0-9]{1,4}", idb in "q[0-9]{1,4}") {
        let key = |task: &str, id: &str| decision_key(&crate::cards::QueuedCard {
            id: id.into(), kind: "recovery".into(), agent_id: None,
            payload: json!({"sub":"model_request","task_key":task}), state: "queued".into(),
        });
        prop_assert_eq!(key(&a, &ida), key(&a, &idb));
        prop_assert_eq!(key(&a, &ida) == key(&b, &ida), a == b);
    }
}
