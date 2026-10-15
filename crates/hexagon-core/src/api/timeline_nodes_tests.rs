//! 功能缺口 A1：节点轨宿主读覆盖负责人发言 / 托管决定 / 未恢复失败，经真实进程内门面。
use super::*;
use crate::trace::window::*;
use proptest::prelude::*;
use serde_json::{json, Value};

fn fixture() -> (tempfile::TempDir, Workbench) {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["pm", "builder"], None).unwrap();
    (dir, wb)
}
fn event(wb: &Workbench, kind: EventKind, agent: Option<&str>, payload: Value) -> i64 {
    wb.db
        .append_event(&wb.project_id, kind, payload, agent, None)
        .unwrap()
}
fn read(wb: &Workbench, after: Option<i64>) -> TimelineNodesPage {
    let root = wb.repo_root.canonicalize().unwrap();
    timeline_nodes(
        &wb.db,
        &wb.repo_root,
        &TimelineNodesRequest {
            expected_project_root: root.to_string_lossy().into_owned(),
            after_event_id: after,
        },
    )
    .unwrap()
}
fn classes(page: &TimelineNodesPage) -> Vec<(i64, TimelineNodeClass)> {
    page.nodes.iter().map(|n| (n.event_id, n.class)).collect()
}

#[test]
fn nodes_cover_owner_messages_managed_decisions_and_unrecovered_failures() {
    let (_dir, wb) = fixture();
    let stage = event(
        &wb,
        EventKind::StageStarted,
        None,
        json!({"stage":"design"}),
    );
    wb.db
        .append_message(
            &wb.project_id,
            "owner",
            "先做登录\n细节稍后",
            &[],
            &[],
            None,
            None,
        )
        .unwrap();
    let owner = read(&wb, None).watermark;
    let managed = event(
        &wb,
        EventKind::System,
        None,
        json!({"kind":"managed_decision","card_kind":"permission","action":"allow","reason":"范围内"}),
    );
    let recovered = event(
        &wb,
        EventKind::TurnFailed,
        Some("a1"),
        json!({"error":"boom"}),
    );
    event(&wb, EventKind::TurnFinished, Some("a1"), json!({}));
    event(
        &wb,
        EventKind::TurnFailed,
        Some("a0"),
        json!({"outcome":"Interrupted"}),
    );
    let failed = event(
        &wb,
        EventKind::TurnFailed,
        Some("a0"),
        json!({"error":"rate limited\nretry later"}),
    );
    let model = event(
        &wb,
        EventKind::System,
        Some("a0"),
        json!({"kind":"model_request_failed","recovery_id":"r1","error":"401"}),
    );
    let page = read(&wb, None);
    assert_eq!(
        classes(&page),
        vec![
            (stage, TimelineNodeClass::Milestone),
            (owner, TimelineNodeClass::Owner),
            (managed, TimelineNodeClass::Managed),
            (failed, TimelineNodeClass::Failed),
            (model, TimelineNodeClass::Failed),
        ]
    );
    assert!(!classes(&page).iter().any(|(id, _)| *id == recovered));
    let by = |id: i64| page.nodes.iter().find(|n| n.event_id == id).unwrap();
    assert_eq!(by(owner).label, "先做登录");
    assert_eq!(by(owner).summary.as_deref(), Some("先做登录\n细节稍后"));
    assert_eq!(by(managed).label, "permission · allow");
    assert_eq!(by(managed).summary.as_deref(), Some("范围内"));
    assert_eq!(by(failed).label, "rate limited");
    assert_eq!(by(failed).agent_id.as_deref(), Some("a0"));
    assert!(page.retired.is_empty());

    // 增量：之前交出的失败在本段被恢复 → retired。
    event(
        &wb,
        EventKind::System,
        Some("a0"),
        json!({"kind":"model_request_recovered","recovery_id":"r1"}),
    );
    event(&wb, EventKind::TurnFinished, Some("a0"), json!({}));
    let delta = read(&wb, Some(page.watermark));
    assert!(delta.nodes.is_empty());
    assert_eq!(delta.retired, vec![failed, model]);
    assert!(read(&wb, None)
        .nodes
        .iter()
        .all(|n| n.class != TimelineNodeClass::Failed));
}

#[derive(Debug, Clone)]
enum Step {
    Milestone,
    Fail(bool),
    Finish(bool),
    ModelFail(u8),
    ModelRecover(u8),
}
fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        Just(Step::Milestone),
        any::<bool>().prop_map(Step::Fail),
        any::<bool>().prop_map(Step::Finish),
        (0u8..3).prop_map(Step::ModelFail),
        (0u8..3).prop_map(Step::ModelRecover),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 不变量：任意切点的「全量读 + 增量读（并入 nodes、撤掉 retired）」与一次全量读相同。
    #[test]
    fn incremental_node_reads_converge_to_full_read(
        steps in prop::collection::vec(step(), 0..30), cut in 0usize..30
    ) {
        let (_dir, wb) = fixture();
        let apply = |s: &Step| {
            let agent = |b: bool| if b { "a0" } else { "a1" };
            match s {
                Step::Milestone => event(&wb, EventKind::Stamped, None, json!({"stage":"s"})),
                Step::Fail(b) => event(&wb, EventKind::TurnFailed, Some(agent(*b)), json!({"error":"x"})),
                Step::Finish(b) => event(&wb, EventKind::TurnFinished, Some(agent(*b)), json!({})),
                Step::ModelFail(r) => event(&wb, EventKind::System, None, json!({"kind":"model_request_failed","recovery_id":r.to_string()})),
                Step::ModelRecover(r) => event(&wb, EventKind::System, None, json!({"kind":"model_request_recovered","recovery_id":r.to_string()})),
            };
        };
        let cut = cut.min(steps.len());
        steps[..cut].iter().for_each(apply);
        let first = read(&wb, None);
        steps[cut..].iter().for_each(apply);
        let delta = read(&wb, Some(first.watermark));
        let mut merged: Vec<(i64, TimelineNodeClass)> = classes(&first)
            .into_iter()
            .filter(|(id, _)| !delta.retired.contains(id))
            .chain(classes(&delta))
            .collect();
        merged.sort_by_key(|(id, _)| *id);
        prop_assert_eq!(merged, classes(&read(&wb, None)));
    }
}
