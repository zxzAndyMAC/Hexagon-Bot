//! Issue 15: bounded history windows and independent current-state projections.
//! Owner Q1–Q6 (2026-10-02): a partial viewport is never evidence that a turn is active.

use super::{EventKind, TimelineItem, TraceError};
use crate::db::Db;
use crate::turn::context::ContextSnapshot;
use rusqlite::{params, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const SELECT: &str =
    "SELECT e.id,e.project_id,e.stage_run_id,e.agent_id,e.kind,e.payload,e.created_at,
 m.id,m.author,m.body,m.tokens,m.created_at,m.attachments,m.thinking,m.element_refs
 FROM events e LEFT JOIN messages m ON m.id=json_extract(e.payload,'$.message_id')";
const DECISIONS: &str = "e.kind IN ('permission_asked','permission_allowed','permission_denied','permission_shape_remembered',
 'stamped','stamp_rejected','flag_submitted','flag_adjudicated','escalated',
 'proposal_queued','proposal_reviewed','proposal_stamped','proposal_rejected','proposal_activated','proposal_rolled_back',
 'publish_requested','publish_confirmed','publish_rejected','publish_failed',
 'autonomy_changed','baseline_merged','stage_rewound','pm_routed')";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum TimelineFilter {
    #[default]
    All,
    Messages,
    Decisions,
    Story,
}
impl TimelineFilter {
    fn sql(self) -> String {
        match self {
            Self::All => "1".into(),
            Self::Messages => "m.id IS NOT NULL".into(),
            Self::Decisions => DECISIONS.into(),
            Self::Story => format!(
                "(m.id IS NOT NULL OR {DECISIONS} OR e.kind IN ('turn_started','turn_failed') OR (e.kind='system' AND json_extract(e.payload,'$.kind') IN ('invariant_violation','tool_breaker','context_denied')))"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum TimelineWindowCursor {
    Latest,
    Before {
        #[ts(type = "number")]
        event_id: i64,
    },
    After {
        #[ts(type = "number")]
        event_id: i64,
    },
    Around {
        #[ts(type = "number")]
        event_id: i64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineWindowRequest {
    pub expected_project_root: String,
    pub filter: TimelineFilter,
    pub agent_id: Option<String>,
    pub cursor: TimelineWindowCursor,
    pub limit: u32,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineWindowPage {
    pub project_root: String,
    pub items: Vec<TimelineItem>,
    pub boundary_pairs: Vec<TimelineItem>,
    pub turn_windows: Vec<TimelineTurnWindow>,
    #[ts(type = "number")]
    pub chapter_base: u64,
    pub has_before: bool,
    pub has_after: bool,
    #[ts(type = "number")]
    pub watermark: i64,
    #[ts(type = "number | null")]
    pub target_event_id: Option<i64>,
    #[ts(type = "number[]")]
    pub steered_message_ids: Vec<i64>,
}
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineWindowMetadataRequest {
    pub expected_project_root: String,
    #[ts(type = "number[]")]
    pub event_ids: Vec<i64>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineWindowMetadata {
    pub project_root: String,
    #[ts(type = "number")]
    pub watermark: i64,
    pub boundary_pairs: Vec<TimelineItem>,
    pub turn_windows: Vec<TimelineTurnWindow>,
    #[ts(type = "number[]")]
    pub steered_message_ids: Vec<i64>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineTurnWindow {
    #[ts(type = "number")]
    pub start_id: i64,
    #[ts(type = "number | null")]
    pub end_id: Option<i64>,
    pub agent_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub failed: bool,
    #[ts(type = "number")]
    pub tool_call_count: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineToolStreamKey {
    pub agent_id: String,
    pub seq: String,
}
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineFactsRequest {
    pub expected_project_root: String,
    pub watched_tool_streams: Vec<TimelineToolStreamKey>,
    #[ts(type = "number[]")]
    pub watched_message_ids: Vec<i64>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineFacts {
    pub project_root: String,
    #[ts(type = "number")]
    pub latest_event_id: i64,
    pub active_turns: Vec<TimelineActiveTurn>,
    pub latest_contexts: Vec<TimelineLatestContext>,
    pub latest_agent_messages: Vec<TimelineAgentMessageReceipt>,
    pub latest_plans: Vec<TimelineLatestPlan>,
    pub settled_tool_streams: Vec<TimelineToolStreamKey>,
    #[ts(type = "number")]
    pub approval_mode_revision: i64,
    #[ts(type = "number | null")]
    pub latest_turn_start_id: Option<i64>,
    #[ts(type = "number[]")]
    pub steered_message_ids: Vec<i64>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineActiveTurn {
    pub agent_id: Option<String>,
    #[ts(type = "number")]
    pub start_event_id: i64,
    pub started_at: String,
    pub current_tool: Option<String>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineLatestContext {
    pub agent_id: String,
    #[ts(type = "number")]
    pub event_id: i64,
    pub created_at: String,
    pub context: Option<ContextSnapshot>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineAgentMessageReceipt {
    pub agent_id: String,
    #[ts(type = "number")]
    pub event_id: i64,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineLatestPlan {
    pub agent_id: String,
    #[ts(type = "number")]
    pub event_id: i64,
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineNodesRequest {
    pub expected_project_root: String,
    #[ts(type = "number | null")]
    pub after_event_id: Option<i64>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineNodesPage {
    pub project_root: String,
    #[ts(type = "number")]
    pub watermark: i64,
    pub nodes: Vec<TimelineNode>,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineNode {
    #[ts(type = "number")]
    pub event_id: i64,
    pub kind: EventKind,
    pub label: String,
}

fn items(
    db: &Db,
    project: &str,
    condition: &str,
    args: &[&dyn ToSql],
) -> Result<Vec<TimelineItem>, TraceError> {
    let mut bound: Vec<&dyn ToSql> = vec![&project];
    bound.extend(args.iter().copied());
    db.query_timeline(
        &format!("{SELECT} WHERE e.project_id=?1 AND {condition}"),
        &bound,
    )
}
// SQLite COUNT is a signed integer; the wire counter is nonnegative u64.
fn read_count(row: &rusqlite::Row<'_>) -> rusqlite::Result<u64> {
    let value: i64 = row.get(0)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn watermark(db: &Db, project: &str) -> Result<i64, TraceError> {
    Ok(db.conn().query_row(
        "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1",
        [project],
        |r| r.get(0),
    )?)
}
fn has(db: &Db, project: &str, condition: &str, args: &[&dyn ToSql]) -> Result<bool, TraceError> {
    let mut bound: Vec<&dyn ToSql> = vec![&project];
    bound.extend(args.iter().copied());
    Ok(db.conn().query_row(&format!("SELECT EXISTS(SELECT 1 FROM events e LEFT JOIN messages m ON m.id=json_extract(e.payload,'$.message_id') WHERE e.project_id=?1 AND {condition})"), bound.as_slice(), |r| r.get(0))?)
}

pub fn window(
    db: &Db,
    project: &str,
    request: &TimelineWindowRequest,
) -> Result<TimelineWindowPage, String> {
    let transaction = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let result = window_inner(db, project, request).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(result)
}
fn window_inner(
    db: &Db,
    project: &str,
    request: &TimelineWindowRequest,
) -> Result<TimelineWindowPage, TraceError> {
    let mark = watermark(db, project)?;
    let limit = i64::from(request.limit.clamp(1, 500));
    let base = format!(
        "e.id<=?2 AND (?3 IS NULL OR e.agent_id=?3) AND ({})",
        request.filter.sql()
    );
    let query = |predicate: &str, pivot: i64, size: i64, order: &str| {
        items(
            db,
            project,
            &format!("{base} AND {predicate} ORDER BY e.id {order} LIMIT ?5"),
            &[&mark, &request.agent_id, &pivot, &size],
        )
    };
    let (mut page, target) = match request.cursor {
        TimelineWindowCursor::Latest => (query("e.id<=?4", mark, limit, "DESC")?, None),
        TimelineWindowCursor::Before { event_id } => {
            (query("e.id<?4", event_id, limit, "DESC")?, None)
        }
        TimelineWindowCursor::After { event_id } => {
            (query("e.id>?4", event_id, limit, "ASC")?, None)
        }
        TimelineWindowCursor::Around { event_id } => {
            if !has(
                db,
                project,
                &format!("{base} AND e.id=?4"),
                &[&mark, &request.agent_id, &event_id],
            )? {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
            let mut left = query("e.id<=?4", event_id, (limit + 1) / 2, "DESC")?;
            let right = query("e.id>?4", event_id, limit - left.len() as i64, "ASC")?;
            if (left.len() + right.len()) < limit as usize {
                left = query("e.id<=?4", event_id, limit - right.len() as i64, "DESC")?;
            }
            left.extend(right);
            (left, Some(event_id))
        }
    };
    page.sort_by_key(|it| it.event.id);
    let first = page.first().map(|it| it.event.id);
    let last = page.last().map(|it| it.event.id);
    let has_before = if let Some(first) = first {
        has(
            db,
            project,
            &format!("{base} AND e.id<?4"),
            &[&mark, &request.agent_id, &first],
        )?
    } else {
        false
    };
    let has_after = if let Some(last) = last {
        has(
            db,
            project,
            &format!("{base} AND e.id>?4"),
            &[&mark, &request.agent_id, &last],
        )?
    } else {
        false
    };
    let chapter_base = if let Some(first) = first {
        db.conn().query_row("SELECT COUNT(*) FROM events WHERE project_id=?1 AND id<?2 AND kind='turn_started' AND (?3 IS NULL OR agent_id=?3)", params![project, first, request.agent_id], read_count)?
    } else {
        0
    };
    let metadata = metadata_for_items(db, project, &request.expected_project_root, &page, mark)?;
    Ok(TimelineWindowPage {
        project_root: request.expected_project_root.clone(),
        boundary_pairs: metadata.boundary_pairs,
        turn_windows: metadata.turn_windows,
        chapter_base,
        has_before,
        has_after,
        watermark: mark,
        target_event_id: target,
        steered_message_ids: metadata.steered_message_ids,
        items: page,
    })
}

// Issue 15 review: a historical/around window must retire live cards when
// receipts arrive, without appending disconnected tail events or reloading bodies.
pub fn metadata(
    db: &Db,
    project: &str,
    request: &TimelineWindowMetadataRequest,
) -> Result<TimelineWindowMetadata, String> {
    if request.event_ids.len() > 500 {
        return Err("timeline metadata exceeds 500 events".into());
    }
    let transaction = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let result = metadata_inner(db, project, request).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(result)
}
fn metadata_inner(
    db: &Db,
    project: &str,
    request: &TimelineWindowMetadataRequest,
) -> Result<TimelineWindowMetadata, TraceError> {
    let mark = watermark(db, project)?;
    let watched = serde_json::to_string(&request.event_ids)?;
    let page = items(
        db,
        project,
        "e.id<=?2 AND e.id IN (SELECT value FROM json_each(?3)) ORDER BY e.id",
        &[&mark, &watched],
    )?;
    metadata_for_items(db, project, &request.expected_project_root, &page, mark)
}
fn metadata_for_items(
    db: &Db,
    project: &str,
    root: &str,
    page: &[TimelineItem],
    mark: i64,
) -> Result<TimelineWindowMetadata, TraceError> {
    let mut support = BTreeMap::new();
    let ids: BTreeSet<_> = page.iter().map(|it| it.event.id).collect();
    for item in page {
        if let Some(other) = paired(db, project, item, mark)? {
            // A result-only historical page can precede a newer resolution of
            // the same action. Include its latest receipt too, never revive an
            // older unknown/failed outcome merely because the call is off-page.
            if item.event.kind == EventKind::ToolResult {
                if let Some(latest) = paired(db, project, &other, mark)? {
                    if !ids.contains(&latest.event.id) {
                        support.insert(latest.event.id, latest);
                    }
                }
            }
            if !ids.contains(&other.event.id) {
                support.insert(other.event.id, other);
            }
        }
    }
    let message_ids: Vec<_> = page
        .iter()
        .filter_map(|it| it.message.as_ref().map(|m| m.id))
        .collect();
    Ok(TimelineWindowMetadata {
        project_root: root.to_owned(),
        watermark: mark,
        boundary_pairs: support.into_values().collect(),
        turn_windows: turns(db, project, page, mark)?,
        steered_message_ids: steered(db, project, &message_ids, mark)?,
    })
}

// Issue 15: failure to find a receipt costs one pending card; borrowing another
// action's receipt falsely reports a side effect as reviewed/completed. Fail closed.
fn paired(
    db: &Db,
    project: &str,
    item: &TimelineItem,
    mark: i64,
) -> Result<Option<TimelineItem>, TraceError> {
    let ev = &item.event;
    let (other_kind, direction, order) = match ev.kind {
        EventKind::ToolCalled => ("tool_result", ">", "ASC"),
        EventKind::ToolResult => ("tool_called", "<", "DESC"),
        _ => return Ok(None),
    };
    if let Some(action) = ev.payload.get("action_id").and_then(|v| v.as_str()) {
        return Ok(items(db, project, "e.id<=?2 AND e.agent_id IS ?3 AND e.kind=?4 AND json_type(e.payload,'$.action_id')='text' AND json_extract(e.payload,'$.action_id')=?5 ORDER BY e.id DESC LIMIT 1", &[&mark, &ev.agent_id, &other_kind, &action])?.pop());
    }
    let adjacent = items(
        db,
        project,
        &format!("e.id<=?2 AND e.id{direction}?3 ORDER BY e.id {order} LIMIT 1"),
        &[&mark, &ev.id],
    )?
    .pop();
    Ok(adjacent.filter(|other| {
        other.event.agent_id == ev.agent_id
            && super::kind_str(other.event.kind) == other_kind
            && other
                .event
                .payload
                .get("action_id")
                .is_none_or(serde_json::Value::is_null)
    }))
}

fn turns(
    db: &Db,
    project: &str,
    page: &[TimelineItem],
    mark: i64,
) -> Result<Vec<TimelineTurnWindow>, TraceError> {
    if page.is_empty() {
        return Ok(Vec::new());
    }
    // Read only compact lifecycle boundaries; a 50,000-tool turn never expands the page.
    let mut stmt = db.conn().prepare("SELECT id,agent_id,kind,created_at FROM events WHERE project_id=?1 AND id<=?2 AND kind IN ('turn_started','turn_finished','turn_failed') ORDER BY id")?;
    let rows = stmt.query_map(params![project, mark], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut open: BTreeMap<Option<String>, (i64, String)> = BTreeMap::new();
    let mut spans = Vec::new();
    for row in rows {
        let (id, agent, kind, at) = row?;
        if kind == "turn_started" {
            open.insert(agent, (id, at));
        } else if let Some((start, started_at)) = open.remove(&agent) {
            spans.push(TimelineTurnWindow {
                start_id: start,
                end_id: Some(id),
                agent_id: agent,
                started_at,
                ended_at: Some(at),
                failed: kind == "turn_failed",
                tool_call_count: 0,
            });
        }
    }
    spans.extend(
        open.into_iter()
            .map(|(agent, (id, at))| TimelineTurnWindow {
                start_id: id,
                end_id: None,
                agent_id: agent,
                started_at: at,
                ended_at: None,
                failed: false,
                tool_call_count: 0,
            }),
    );
    spans.retain(|span| {
        page.iter().any(|it| {
            it.event.id >= span.start_id
                && it.event.id <= span.end_id.unwrap_or(mark)
                && (it.event.agent_id == span.agent_id || it.event.agent_id.is_none())
        })
    });
    for span in &mut spans {
        span.tool_call_count = db.conn().query_row("SELECT COUNT(*) FROM events WHERE project_id=?1 AND id>=?2 AND id<=?3 AND agent_id IS ?4 AND kind='tool_called'", params![project, span.start_id, span.end_id.unwrap_or(mark), span.agent_id], read_count)?;
    }
    spans.sort_by_key(|s| s.end_id.unwrap_or(i64::MAX));
    Ok(spans)
}

fn steered(db: &Db, project: &str, ids: &[i64], mark: i64) -> Result<Vec<i64>, TraceError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let watched = serde_json::to_string(ids)?;
    let mut stmt = db.conn().prepare("SELECT DISTINCT CAST(json_extract(payload,'$.msg_id') AS INTEGER) FROM events WHERE project_id=?1 AND id<=?2 AND kind='system' AND json_extract(payload,'$.kind')='steering_injected' AND json_extract(payload,'$.msg_id') IN (SELECT value FROM json_each(?3))")?;
    let values = stmt
        .query_map(params![project, mark, watched], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

pub fn facts(
    db: &Db,
    project: &str,
    request: &TimelineFactsRequest,
) -> Result<TimelineFacts, String> {
    if request.watched_tool_streams.len() > 500 || request.watched_message_ids.len() > 500 {
        return Err("timeline watched receipts exceed 500".into());
    }
    let transaction = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let result = facts_inner(db, project, request).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(result)
}
fn facts_inner(
    db: &Db,
    project: &str,
    request: &TimelineFactsRequest,
) -> Result<TimelineFacts, TraceError> {
    let mark = watermark(db, project)?;
    let reset: i64 = db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND id<=?2 AND kind IN ('team_slept','stage_started')", params![project,mark], |r| r.get(0))?;
    let mut active_turns = Vec::new();
    let mut statement = db.conn().prepare("SELECT id,agent_id,created_at,kind FROM events WHERE id IN (SELECT MAX(id) FROM events WHERE project_id=?1 AND id>?2 AND id<=?3 AND kind IN ('turn_started','turn_finished','turn_failed') GROUP BY agent_id) ORDER BY id")?;
    let lifecycle = statement.query_map(params![project, reset, mark], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in lifecycle {
        let (start, agent, at, kind) = row?;
        if kind != "turn_started" {
            continue;
        }
        let current_tool = db.conn().query_row("SELECT CASE WHEN kind='tool_called' THEN NULLIF(CAST(json_extract(payload,'$.tool') AS TEXT),'') ELSE NULL END FROM events WHERE project_id=?1 AND id>?2 AND id<=?3 AND agent_id IS ?4 AND kind IN ('tool_called','tool_result','permission_asked','permission_denied') ORDER BY id DESC LIMIT 1", params![project,start,mark,agent], |r| r.get::<_,Option<String>>(0)).optional()?.flatten();
        active_turns.push(TimelineActiveTurn {
            agent_id: agent,
            start_event_id: start,
            started_at: at,
            current_tool,
        });
    }
    let context_items = items(
        db,
        project,
        "e.id IN (SELECT MAX(id) FROM events WHERE project_id=?1 AND id<=?2 AND agent_id IS NOT NULL AND kind='system' AND json_extract(payload,'$.kind')='request_envelope' AND json_type(payload,'$.context') IS NOT NULL AND json_type(payload,'$.context')!='null' GROUP BY agent_id) ORDER BY e.id",
        &[&mark],
    )?;
    let latest_contexts = context_items
        .into_iter()
        .filter_map(|it| {
            let agent_id = it.event.agent_id?;
            let context = valid_context(&it.event.payload["context"]);
            Some(TimelineLatestContext {
                agent_id,
                event_id: it.event.id,
                created_at: it.event.created_at,
                context,
            })
        })
        .collect();
    let mut statement = db.conn().prepare("SELECT m.author,MAX(e.id) FROM events e JOIN messages m ON m.id=json_extract(e.payload,'$.message_id') WHERE e.project_id=?1 AND e.id<=?2 GROUP BY m.author")?;
    let latest_agent_messages = statement
        .query_map(params![project, mark], |r| {
            Ok(TimelineAgentMessageReceipt {
                agent_id: r.get(0)?,
                event_id: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let latest_plans = items(db, project, "e.id IN (SELECT MAX(id) FROM events WHERE project_id=?1 AND id<=?2 AND agent_id IS NOT NULL AND kind='system' AND json_extract(payload,'$.kind')='agent_plan' GROUP BY agent_id) ORDER BY e.id", &[&mark])?.into_iter().filter_map(|it| Some(TimelineLatestPlan {agent_id:it.event.agent_id?,event_id:it.event.id,text:it.event.payload.get("text")?.as_str()?.to_owned()})).collect();
    let mut settled_tool_streams = Vec::new();
    for key in &request.watched_tool_streams {
        let called = items(db, project, "e.id<=?2 AND e.agent_id=?3 AND e.kind='tool_called' AND CAST(json_extract(e.payload,'$.seq') AS TEXT)=?4 ORDER BY e.id DESC LIMIT 1", &[&mark,&key.agent_id,&key.seq])?.pop();
        if let Some(called) = called {
            if paired(db, project, &called, mark)?.is_some() {
                settled_tool_streams.push(key.clone());
            }
        }
    }
    let approval_mode_revision = db.conn().query_row("SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND id<=?2 AND kind='system' AND json_extract(payload,'$.kind')='approval_mode_changed'", params![project,mark], |r| r.get(0))?;
    let latest_turn_start_id = db.conn().query_row(
        "SELECT MAX(id) FROM events WHERE project_id=?1 AND id<=?2 AND kind='turn_started'",
        params![project, mark],
        |r| r.get(0),
    )?;
    Ok(TimelineFacts {
        project_root: request.expected_project_root.clone(),
        latest_event_id: mark,
        active_turns,
        latest_contexts,
        latest_agent_messages,
        latest_plans,
        settled_tool_streams,
        approval_mode_revision,
        latest_turn_start_id,
        steered_message_ids: steered(db, project, &request.watched_message_ids, mark)?,
    })
}

fn valid_context(value: &serde_json::Value) -> Option<ContextSnapshot> {
    let window_tokens = value.get("window_tokens")?.as_u64()?;
    if window_tokens == 0 {
        return None;
    }
    Some(ContextSnapshot {
        used_tokens: usize::try_from(value.get("used_tokens")?.as_u64()?).ok()?,
        window_tokens,
        compact_at_tokens: usize::try_from(value.get("compact_at_tokens")?.as_u64()?).ok()?,
        model_slot: value.get("model_slot")?.as_str()?.to_owned(),
    })
}

pub fn nodes(
    db: &Db,
    project: &str,
    request: &TimelineNodesRequest,
) -> Result<TimelineNodesPage, String> {
    let transaction = db
        .conn()
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let result = nodes_inner(db, project, request).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(result)
}
fn nodes_inner(
    db: &Db,
    project: &str,
    request: &TimelineNodesRequest,
) -> Result<TimelineNodesPage, TraceError> {
    let mark = watermark(db, project)?;
    let mut statement = db.conn().prepare("SELECT id,kind,CAST(COALESCE(json_extract(payload,'$.stage'),json_extract(payload,'$.path'),json_extract(payload,'$.flag_id'),kind) AS TEXT) FROM events WHERE project_id=?1 AND id>?2 AND id<=?3 AND kind IN ('stage_started','stamped','artifact_delivered','flag_submitted','flag_adjudicated','escalated','check_overridden','install_completed') ORDER BY id")?;
    let rows = statement.query_map(
        params![project, request.after_event_id.unwrap_or(0), mark],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut nodes = Vec::new();
    for row in rows {
        let (event_id, kind, label) = row?;
        nodes.push(TimelineNode {
            event_id,
            kind: serde_json::from_value(serde_json::Value::String(kind))?,
            label,
        });
    }
    Ok(TimelineNodesPage {
        project_root: request.expected_project_root.clone(),
        watermark: mark,
        nodes,
    })
}
