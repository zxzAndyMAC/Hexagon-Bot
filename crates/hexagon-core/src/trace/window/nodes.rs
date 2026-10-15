//! Node rail host read (components C15, interaction §5).
//!
//! 功能缺口 A1（2026-10-15，负责人已批准扩展 `timeline_nodes`）：票 06 只把里程碑 kind 交给宿主读，
//! 负责人发言 / 托管决定 / 失败三类节点只能从已加载窗口派生，长历史里看不到——轨道与时间线对不上。
//! 现在四类都由这里给出，节点轨覆盖全历史。
//!
//! 失败节点是「未恢复」的：`turn_failed` 之后同一 Agent 有 `turn_finished` 即恢复；模型请求失败
//! （`system.model_request_failed`）之后同一 `recovery_id` 有任何恢复项事件即不再算失败（一个 recovery_id
//! 只看最后状态）。主动停止（outcome = Interrupted）不算失败。增量读（after_event_id）里，之前已经交出的
//! 失败节点若在本段被恢复，通过 `retired` 告诉 UI 撤掉——否则增量合并会留下过期的红刻度。

use super::{watermark, EventKind, TraceError};
use crate::db::Db;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const MILESTONE_KINDS: &[&str] = &[
    "stage_started",
    "stamped",
    "artifact_delivered",
    "flag_submitted",
    "flag_adjudicated",
    "escalated",
    "check_overridden",
    "install_completed",
];
const MODEL_RECOVERY_KINDS: &[&str] = &[
    "model_request_failed",
    "model_request_retrying",
    "model_request_recovered",
    "model_request_superseded",
];
const LABEL_CHARS: usize = 80;
const SUMMARY_CHARS: usize = 240;

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
    /// 早于 after_event_id、此前作为失败节点交出、在本段内被恢复的事件 id。全量读恒为空。
    #[ts(type = "Array<number>")]
    pub retired: Vec<i64>,
}
/// 节点类别：里程碑（阶段 / 盖章 / 交付…）、负责人发言、托管决定、未恢复的失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum TimelineNodeClass {
    Milestone,
    Owner,
    Managed,
    Failed,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TimelineNode {
    #[ts(type = "number")]
    pub event_id: i64,
    pub kind: EventKind,
    pub class: TimelineNodeClass,
    pub label: String,
    /// 预览卡两行摘要（原话 / 理由 / 错误）；截断到 240 字符。
    pub summary: Option<String>,
    pub agent_id: Option<String>,
    pub created_at: String,
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

struct Row {
    id: i64,
    kind: String,
    agent_id: Option<String>,
    created_at: String,
    payload: Value,
    body: Option<String>,
}

struct Failure {
    node: TimelineNode,
    recovered_at: Option<i64>,
}

fn nodes_inner(
    db: &Db,
    project: &str,
    request: &TimelineNodesRequest,
) -> Result<TimelineNodesPage, TraceError> {
    let mark = watermark(db, project)?;
    let after = request.after_event_id.unwrap_or(0);
    let rows = candidate_rows(db, project, after, mark)?;
    let mut nodes = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut open_turns: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    let mut open_models: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for row in rows {
        let sub = row
            .payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("");
        match (row.kind.as_str(), sub) {
            ("turn_finished", _) => {
                for index in open_turns.remove(&row.agent_id).unwrap_or_default() {
                    failures[index].recovered_at = Some(row.id);
                }
            }
            ("system", sub) if MODEL_RECOVERY_KINDS.contains(&sub) => {
                let recovery = recovery_id(&row.payload);
                for index in open_models.remove(&recovery).unwrap_or_default() {
                    failures[index].recovered_at = Some(row.id);
                }
                if sub == "model_request_failed" {
                    open_models
                        .entry(recovery)
                        .or_default()
                        .push(failures.len());
                    failures.push(Failure {
                        node: failure_node(&row)?,
                        recovered_at: None,
                    });
                }
            }
            ("turn_failed", _) if !interrupted(&row.payload) => {
                open_turns
                    .entry(row.agent_id.clone())
                    .or_default()
                    .push(failures.len());
                failures.push(Failure {
                    node: failure_node(&row)?,
                    recovered_at: None,
                });
            }
            ("turn_failed", _) => {}
            _ if row.id > after => nodes.push(plain_node(&row)?),
            _ => {}
        }
    }
    let mut retired = Vec::new();
    for failure in failures {
        match failure.recovered_at {
            None if failure.node.event_id > after => nodes.push(failure.node),
            Some(at) if failure.node.event_id <= after && at > after => {
                retired.push(failure.node.event_id)
            }
            _ => {}
        }
    }
    nodes.sort_by_key(|node| node.event_id);
    Ok(TimelineNodesPage {
        project_root: request.expected_project_root.clone(),
        watermark: mark,
        nodes,
        retired,
    })
}

/// 里程碑 / 负责人发言 / 托管决定只要本段（id > after）；失败判定要回看全历史的失败与恢复事件。
fn candidate_rows(db: &Db, project: &str, after: i64, mark: i64) -> Result<Vec<Row>, TraceError> {
    let milestones = MILESTONE_KINDS
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(",");
    let models = MODEL_RECOVERY_KINDS
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT e.id,e.kind,e.agent_id,e.created_at,e.payload,m.body
         FROM events e LEFT JOIN messages m ON m.id=json_extract(e.payload,'$.message_id') AND m.author='owner'
         WHERE e.project_id=?1 AND e.id<=?3 AND (
           (e.id>?2 AND (e.kind IN ({milestones},'owner_message')
             OR (e.kind='system' AND json_extract(e.payload,'$.kind')='managed_decision')))
           OR e.kind IN ('turn_failed','turn_finished')
           OR (e.kind='system' AND json_extract(e.payload,'$.kind') IN ({models})))
         ORDER BY e.id"
    );
    let mut statement = db.conn().prepare(&sql)?;
    let rows = statement.query_map(params![project, after, mark], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, agent_id, created_at, payload, body) = row?;
        out.push(Row {
            id,
            kind,
            agent_id,
            created_at,
            payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            body,
        });
    }
    Ok(out)
}

fn plain_node(row: &Row) -> Result<TimelineNode, TraceError> {
    let text = |key: &str| row.payload.get(key).and_then(Value::as_str);
    let (class, label, summary) = match row.kind.as_str() {
        "owner_message" => {
            let body = row.body.clone().unwrap_or_default();
            (
                TimelineNodeClass::Owner,
                first_line(&body),
                non_empty(clip(&body, SUMMARY_CHARS)),
            )
        }
        "system" => {
            let label = [text("card_kind"), text("action")]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            (
                TimelineNodeClass::Managed,
                if label.is_empty() {
                    "managed_decision".to_owned()
                } else {
                    label
                },
                text("reason").and_then(|r| non_empty(clip(r, SUMMARY_CHARS))),
            )
        }
        kind => {
            let label = ["stage", "path", "flag_id"]
                .into_iter()
                .find_map(|key| row.payload.get(key).filter(|v| !v.is_null()).map(scalar))
                .unwrap_or_else(|| kind.to_owned());
            let summary = ["summary", "note", "reason"]
                .into_iter()
                .find_map(text)
                .and_then(|s| non_empty(clip(s, SUMMARY_CHARS)));
            (TimelineNodeClass::Milestone, label, summary)
        }
    };
    node(row, class, label, summary)
}

fn failure_node(row: &Row) -> Result<TimelineNode, TraceError> {
    let error = ["error", "reason"]
        .into_iter()
        .find_map(|key| row.payload.get(key).filter(|v| !v.is_null()).map(scalar))
        .unwrap_or_default();
    let label = match first_line(&error) {
        line if line.is_empty() => row
            .payload
            .get("kind")
            .and_then(Value::as_str)
            .filter(|_| row.kind == "system")
            .unwrap_or(&row.kind)
            .to_owned(),
        line => line,
    };
    node(
        row,
        TimelineNodeClass::Failed,
        label,
        non_empty(clip(&error, SUMMARY_CHARS)),
    )
}

fn node(
    row: &Row,
    class: TimelineNodeClass,
    label: String,
    summary: Option<String>,
) -> Result<TimelineNode, TraceError> {
    Ok(TimelineNode {
        event_id: row.id,
        kind: serde_json::from_value(Value::String(row.kind.clone()))?,
        class,
        label,
        summary,
        agent_id: row.agent_id.clone(),
        created_at: row.created_at.clone(),
    })
}

/// 与 UI `isInterruptedTurn` 同口径：主动停止落成 turn_failed，但不是失败。
fn interrupted(payload: &Value) -> bool {
    let outcome = payload.get("outcome").or_else(|| payload.get("error"));
    matches!(
        outcome.and_then(Value::as_str),
        Some("Ok(Interrupted)" | "Interrupted")
    )
}

fn recovery_id(payload: &Value) -> String {
    payload.get("recovery_id").map(scalar).unwrap_or_default()
}

fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn first_line(text: &str) -> String {
    clip(text.trim().lines().next().unwrap_or(""), LABEL_CHARS)
}

fn clip(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
