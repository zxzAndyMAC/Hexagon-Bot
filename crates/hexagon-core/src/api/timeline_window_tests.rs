//! Issue 15 regressions through the real in-process facade, not a parallel mock store.
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
fn request(wb: &Workbench, cursor: TimelineWindowCursor, limit: u32) -> TimelineWindowRequest {
    TimelineWindowRequest {
        expected_project_root: wb
            .repo_root
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        filter: TimelineFilter::All,
        agent_id: None,
        cursor,
        limit,
    }
}
fn page(wb: &Workbench, req: &TimelineWindowRequest) -> TimelineWindowPage {
    timeline_window(&wb.db, &wb.repo_root, req).unwrap()
}
fn facts_request(wb: &Workbench) -> TimelineFactsRequest {
    TimelineFactsRequest {
        expected_project_root: wb
            .repo_root
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        watched_tool_streams: Vec::new(),
        watched_message_ids: Vec::new(),
    }
}
fn ids(page: &TimelineWindowPage) -> Vec<i64> {
    page.items.iter().map(|it| it.event.id).collect()
}

#[test]
fn timeline_window_reads_latest_without_crawling_oldest_batches() {
    let (_dir, wb) = fixture();
    for _ in 0..5201 {
        event(&wb, EventKind::Paused, None, json!({}));
    }
    let latest = page(&wb, &request(&wb, TimelineWindowCursor::Latest, 100));
    assert_eq!(latest.items.len(), 100);
    assert_eq!(latest.items.last().unwrap().event.id, latest.watermark);
    assert!(latest.has_before);
    assert!(!latest.has_after);
    let before = page(
        &wb,
        &request(
            &wb,
            TimelineWindowCursor::Before {
                event_id: latest.items[0].event.id,
            },
            100,
        ),
    );
    assert!(before.items.last().unwrap().event.id < latest.items[0].event.id);
    let after = page(
        &wb,
        &request(
            &wb,
            TimelineWindowCursor::After {
                event_id: before.items.last().unwrap().event.id,
            },
            100,
        ),
    );
    assert_eq!(ids(&after), ids(&latest));
    assert_eq!(
        page(&wb, &request(&wb, TimelineWindowCursor::Latest, 0))
            .items
            .len(),
        1
    );
    assert_eq!(
        page(&wb, &request(&wb, TimelineWindowCursor::Latest, u32::MAX))
            .items
            .len(),
        500
    );
}

#[test]
fn timeline_window_filters_query_the_full_history_and_around_handles_edges() {
    let (_dir, wb) = fixture();
    wb.db
        .append_message(
            &wb.project_id,
            "a0",
            "old but latest message",
            &[],
            &[],
            Some("a0"),
            None,
        )
        .unwrap();
    let message_id = wb.db.timeline(&wb.project_id, None, 10, None).unwrap()[0]
        .event
        .id;
    let decision = event(&wb, EventKind::PermissionAsked, Some("a0"), json!({}));
    let failure = event(&wb, EventKind::TurnFailed, Some("a0"), json!({}));
    for _ in 0..1200 {
        event(
            &wb,
            EventKind::ToolCalled,
            Some("a0"),
            json!({"tool":"fs_read"}),
        );
    }
    let mut req = request(&wb, TimelineWindowCursor::Latest, 10);
    req.filter = TimelineFilter::Messages;
    assert_eq!(ids(&page(&wb, &req)), vec![message_id]);
    req.filter = TimelineFilter::Decisions;
    assert_eq!(ids(&page(&wb, &req)), vec![decision]);
    req.filter = TimelineFilter::Story;
    assert_eq!(ids(&page(&wb, &req)), vec![message_id, decision, failure]);
    req.filter = TimelineFilter::All;
    req.cursor = TimelineWindowCursor::Around {
        event_id: message_id,
    };
    let around = page(&wb, &req);
    assert_eq!(around.items.len(), 10);
    assert_eq!(around.target_event_id, Some(message_id));
    assert!(!around.has_before);
    assert!(around.has_after);
    req.cursor = TimelineWindowCursor::Around {
        event_id: around.watermark,
    };
    let end = page(&wb, &req);
    assert_eq!(end.items.len(), 10);
    assert!(!end.has_after);
    req.cursor = TimelineWindowCursor::Around {
        event_id: around.watermark + 1,
    };
    assert!(timeline_window(&wb.db, &wb.repo_root, &req).is_err());
    req.filter = TimelineFilter::Messages;
    req.cursor = TimelineWindowCursor::Around { event_id: decision };
    assert!(timeline_window(&wb.db, &wb.repo_root, &req).is_err());
}

#[test]
fn timeline_window_keeps_actual_action_receipts_and_long_turn_metadata_across_pages() {
    let (_dir, wb) = fixture();
    let start = event(&wb, EventKind::TurnStarted, Some("a0"), json!({}));
    let called = event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"tool":"computer_observe","action_id":"observe-one","seq":"[\"r1\",\"tool1\"]"}),
    );
    for _ in 0..300 {
        event(&wb, EventKind::Paused, None, json!({}));
    }
    // Real tool_result shape has no `tool`; borrowing agent B's receipt is forbidden.
    event(
        &wb,
        EventKind::ToolResult,
        Some("a1"),
        json!({"action_id":"observe-one","ok":true,"result":{"output":{"ok":true}}}),
    );
    let receipt = event(
        &wb,
        EventKind::ToolResult,
        Some("a0"),
        json!({"action_id":"observe-one","ok":true,"result":{"output":{"ok":true}}}),
    );
    let end = event(&wb, EventKind::TurnFinished, Some("a0"), json!({}));
    let result_page = page(
        &wb,
        &request(&wb, TimelineWindowCursor::Before { event_id: end }, 1),
    );
    assert_eq!(ids(&result_page), vec![receipt]);
    assert_eq!(result_page.boundary_pairs.len(), 1);
    assert_eq!(result_page.boundary_pairs[0].event.id, called);
    assert_eq!(result_page.turn_windows.len(), 1);
    let turn = &result_page.turn_windows[0];
    assert_eq!(
        (turn.start_id, turn.end_id, turn.tool_call_count),
        (start, Some(end), 1)
    );
    let called_page = page(
        &wb,
        &request(&wb, TimelineWindowCursor::Around { event_id: called }, 1),
    );
    assert_eq!(called_page.boundary_pairs[0].event.id, receipt);
    assert_eq!(called_page.items.len(), 1);
}

#[test]
fn timeline_window_legacy_pairing_requires_actual_adjacent_unkeyed_result() {
    let (_dir, wb) = fixture();
    let called = event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"tool":"fs_read"}),
    );
    event(&wb, EventKind::Paused, None, json!({}));
    event(&wb, EventKind::ToolResult, Some("a0"), json!({"ok":true}));
    let req = request(&wb, TimelineWindowCursor::Around { event_id: called }, 1);
    assert!(page(&wb, &req).boundary_pairs.is_empty());
    let next = event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"tool":"fs_read"}),
    );
    let receipt = event(&wb, EventKind::ToolResult, Some("a0"), json!({"ok":true}));
    assert_eq!(
        page(
            &wb,
            &request(&wb, TimelineWindowCursor::Around { event_id: next }, 1)
        )
        .boundary_pairs[0]
            .event
            .id,
        receipt
    );
}

#[test]
fn timeline_facts_are_complete_without_loading_history_and_settle_only_exact_stream_keys() {
    let (_dir, wb) = fixture();
    event(&wb, EventKind::TurnStarted, Some("a0"), json!({}));
    event(
        &wb,
        EventKind::System,
        Some("a0"),
        json!({"kind":"request_envelope","context":{"used_tokens":12,"window_tokens":100,"compact_at_tokens":80,"model_slot":"pm"}}),
    );
    let msg = wb
        .db
        .append_message(&wb.project_id, "a0", "answer", &[], &[], Some("a0"), None)
        .unwrap();
    event(
        &wb,
        EventKind::System,
        Some("a0"),
        json!({"kind":"agent_plan","text":"read then implement"}),
    );
    event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"tool":"fs_read","action_id":"one","seq":"r0:i0"}),
    );
    event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"tool":"fs_read","action_id":"two","seq":"[\"r0\",\"i1\"]"}),
    );
    event(
        &wb,
        EventKind::ToolResult,
        Some("a0"),
        json!({"action_id":"two","ok":true}),
    );
    event(
        &wb,
        EventKind::System,
        None,
        json!({"kind":"steering_injected","msg_id":msg}),
    );
    let mut req = facts_request(&wb);
    req.watched_tool_streams = vec![
        TimelineToolStreamKey {
            agent_id: "a0".into(),
            seq: "r0:i0".into(),
        },
        TimelineToolStreamKey {
            agent_id: "a0".into(),
            seq: "[\"r0\",\"i1\"]".into(),
        },
    ];
    req.watched_message_ids = vec![msg];
    let facts = timeline_facts(&wb.db, &wb.repo_root, &req).unwrap();
    assert_eq!(facts.active_turns.len(), 1);
    assert_eq!(
        facts.latest_contexts[0]
            .context
            .as_ref()
            .unwrap()
            .used_tokens,
        12
    );
    assert_eq!(facts.latest_agent_messages[0].agent_id, "a0");
    assert_eq!(facts.latest_plans[0].text, "read then implement");
    assert_eq!(
        facts.settled_tool_streams,
        vec![req.watched_tool_streams[1].clone()]
    );
    assert_eq!(facts.steered_message_ids, vec![msg]);
    event(&wb, EventKind::TeamSlept, None, json!({}));
    event(
        &wb,
        EventKind::System,
        Some("a0"),
        json!({"kind":"request_envelope","context":{"used_tokens":12,"window_tokens":0}}),
    );
    let facts = timeline_facts(&wb.db, &wb.repo_root, &req).unwrap();
    assert!(facts.active_turns.is_empty());
    assert!(facts.latest_contexts[0].context.is_none());
}

#[test]
fn timeline_nodes_remain_complete_and_foreign_workspace_reads_are_rejected() {
    let (_dir, wb) = fixture();
    let mark = event(
        &wb,
        EventKind::ArtifactDelivered,
        Some("a0"),
        json!({"path":"report.md"}),
    );
    for _ in 0..501 {
        event(&wb, EventKind::Paused, None, json!({}));
    }
    let req = TimelineNodesRequest {
        expected_project_root: wb
            .repo_root
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        after_event_id: None,
    };
    let nodes = timeline_nodes(&wb.db, &wb.repo_root, &req).unwrap();
    assert_eq!(nodes.nodes.len(), 1);
    assert_eq!(nodes.nodes[0].event_id, mark);
    assert_eq!(nodes.nodes[0].label, "report.md");
    let delta = timeline_nodes(
        &wb.db,
        &wb.repo_root,
        &TimelineNodesRequest {
            after_event_id: Some(nodes.watermark),
            ..req
        },
    )
    .unwrap();
    assert!(delta.nodes.is_empty());
    assert_eq!(delta.watermark, nodes.watermark);
    let other = tempfile::tempdir().unwrap();
    let mut req = request(&wb, TimelineWindowCursor::Latest, 10);
    req.expected_project_root = other.path().to_string_lossy().into_owned();
    assert!(timeline_window(&wb.db, &wb.repo_root, &req).is_err());
    let mut req = facts_request(&wb);
    req.expected_project_root = other.path().to_string_lossy().into_owned();
    assert!(timeline_facts(&wb.db, &wb.repo_root, &req).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn timeline_window_before_partition_is_ordered_complete_and_bounded(
        decisions in prop::collection::vec(any::<bool>(),0..100), size in 1u32..20, filter_decisions in any::<bool>()
    ) {
        let (_dir,wb)=fixture();
        let mut expected=Vec::new();
        for decision in decisions {
            let id=event(&wb,if decision {EventKind::Stamped} else {EventKind::Paused},None,json!({}));
            if !filter_decisions || decision { expected.push(id); }
        }
        let mut req=request(&wb,TimelineWindowCursor::Latest,size);
        if filter_decisions { req.filter=TimelineFilter::Decisions; }
        let mut gathered=Vec::new();
        loop {
            let chunk=page(&wb,&req);
            prop_assert!(chunk.items.len()<=size as usize);
            let mut current=ids(&chunk);
            prop_assert!(current.windows(2).all(|pair| pair[0]<pair[1]));
            let first=current.first().copied();
            current.extend(gathered);
            gathered=current;
            if !chunk.has_before { break; }
            req.cursor=TimelineWindowCursor::Before {event_id:first.unwrap()};
        }
        prop_assert_eq!(gathered,expected);
    }
    #[test]
    fn timeline_boundary_receipts_never_cross_agent_or_action(
        matching_agent in any::<bool>(), matching_action in any::<bool>(), gap in 0..10usize
    ) {
        let (_dir,wb)=fixture();
        let call=event(&wb,EventKind::ToolCalled,Some("a0"),json!({"action_id":"one","tool":"fs_read"}));
        for _ in 0..gap { event(&wb,EventKind::Paused,None,json!({})); }
        event(&wb,EventKind::ToolResult,Some(if matching_agent {"a0"} else {"a1"}),json!({"action_id":if matching_action {"one"} else {"two"},"ok":true}));
        let result=page(&wb,&request(&wb,TimelineWindowCursor::Around {event_id:call},1));
        prop_assert_eq!(!result.boundary_pairs.is_empty(),matching_agent&&matching_action);
    }
}

#[test]
fn timeline_story_chapter_numbers_agent_histories_and_empty_windows_are_independent() {
    let (_dir, wb) = fixture();
    let empty = page(&wb, &request(&wb, TimelineWindowCursor::Latest, 30));
    assert!(empty.items.is_empty());
    assert_eq!(empty.watermark, 0);
    assert!(!empty.has_before && !empty.has_after);
    for _ in 0..5 {
        event(&wb, EventKind::TurnStarted, Some("a0"), json!({}));
        event(&wb, EventKind::TurnFinished, Some("a0"), json!({}));
        event(&wb, EventKind::TurnStarted, Some("a1"), json!({}));
        event(&wb, EventKind::TurnFinished, Some("a1"), json!({}));
    }
    let mut req = request(&wb, TimelineWindowCursor::Latest, 2);
    req.filter = TimelineFilter::Story;
    let all = page(&wb, &req);
    assert_eq!(all.chapter_base, 8);
    assert_eq!(all.items.len(), 2);
    req.agent_id = Some("a0".into());
    let a = page(&wb, &req);
    assert_eq!(a.chapter_base, 3);
    assert!(a
        .items
        .iter()
        .all(|it| it.event.agent_id.as_deref() == Some("a0")));
    req.cursor = TimelineWindowCursor::Before {
        event_id: a.items[0].event.id,
    };
    let before = page(&wb, &req);
    assert_eq!(before.chapter_base, 1);
}

#[test]
fn timeline_receipt_indexes_are_present_for_bounded_old_page_pairing() {
    let (_dir, wb) = fixture();
    // A schema assertion protects the measured old-history lookup regression.
    // Do not assert SQLite's human-readable planner output, which varies by version.
    let indexes: i64=wb.db.conn().query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN ('idx_events_action_receipt','idx_events_tool_stream')",[],|r|r.get(0)).unwrap();
    assert_eq!(indexes, 2);
}

#[test]
fn timeline_old_result_page_keeps_latest_resolution_and_null_agent_identity() {
    let (_dir, wb) = fixture();
    let call = event(
        &wb,
        EventKind::ToolCalled,
        None,
        json!({"action_id":"same","tool":"fs_read"}),
    );
    let unknown = event(
        &wb,
        EventKind::ToolResult,
        None,
        json!({"action_id":"same","ok":null,"state":"unknown"}),
    );
    event(
        &wb,
        EventKind::ToolResult,
        Some("a0"),
        json!({"action_id":"same","ok":false}),
    );
    let resolved = event(
        &wb,
        EventKind::ToolResult,
        None,
        json!({"action_id":"same","ok":true}),
    );
    let chunk = page(
        &wb,
        &request(&wb, TimelineWindowCursor::Around { event_id: unknown }, 1),
    );
    assert_eq!(ids(&chunk), vec![unknown]);
    assert_eq!(
        chunk
            .boundary_pairs
            .iter()
            .map(|it| it.event.id)
            .collect::<Vec<_>>(),
        vec![call, resolved]
    );
}

#[test]
fn timeline_window_metadata_retires_historical_live_cards_without_returning_tail_bodies() {
    let (_dir, wb) = fixture();
    let start = event(&wb, EventKind::TurnStarted, Some("a0"), json!({}));
    let call = event(
        &wb,
        EventKind::ToolCalled,
        Some("a0"),
        json!({"action_id":"later","tool":"fs_read"}),
    );
    let initial = page(
        &wb,
        &request(&wb, TimelineWindowCursor::Around { event_id: call }, 1),
    );
    assert!(initial.boundary_pairs.is_empty());
    assert_eq!(initial.turn_windows[0].end_id, None);
    let receipt = event(
        &wb,
        EventKind::ToolResult,
        Some("a0"),
        json!({"action_id":"later","ok":true}),
    );
    let end = event(&wb, EventKind::TurnFinished, Some("a0"), json!({}));
    wb.db
        .append_message(
            &wb.project_id,
            "a0",
            "unrelated distant tail body",
            &[],
            &[],
            Some("a0"),
            None,
        )
        .unwrap();
    let metadata = timeline_window_metadata(
        &wb.db,
        &wb.repo_root,
        &TimelineWindowMetadataRequest {
            expected_project_root: wb
                .repo_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            event_ids: vec![start, call],
        },
    )
    .unwrap();
    assert_eq!(
        metadata
            .boundary_pairs
            .iter()
            .map(|it| it.event.id)
            .collect::<Vec<_>>(),
        vec![receipt]
    );
    assert_eq!(metadata.turn_windows[0].end_id, Some(end));
    assert_eq!(metadata.turn_windows[0].start_id, start);
    assert!(metadata.watermark > end);
    assert!(metadata
        .boundary_pairs
        .iter()
        .all(|it| it.message.is_none()));
    let too_many = TimelineWindowMetadataRequest {
        expected_project_root: metadata.project_root,
        event_ids: vec![call; 501],
    };
    assert!(timeline_window_metadata(&wb.db, &wb.repo_root, &too_many).is_err());
}
