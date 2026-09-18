//! 轨迹事件：只追加不可变的事件流 + 时间线投影。
//!
//! 一切行为（激活/交付/复审/打回/权限/盖章/提案/系统…）落成 `events` 行；
//! 群聊时间线、归来摘要、审计回放都是它的投影——结构化事件是权威，
//! 聊天文本只是其中一类。

use crate::db::{Db, DbError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 事件类型枚举。新增类型 = 加变体；spec「轨迹」列出的事件面逐项在此。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    // 回合与生命周期
    TurnStarted,
    TurnFinished,
    TurnFailed,
    AgentActivated,
    AgentSlept,
    StageStarted,
    StageFinished,
    StageSkipped,
    StageRewound,
    Paused,
    Resumed,
    // 产物与复审
    ArtifactDelivered,
    ArtifactRejected,
    ReviewPassed,
    ReviewRejected,
    ReviewSkipped,
    FlagSubmitted,
    FlagAdjudicated,
    BackfillExecuted,
    ConsultWakeup,
    // 权限与盖章
    PermissionAsked,
    PermissionAllowed,
    PermissionDenied,
    PermissionShapeRemembered,
    Stamped,
    StampRejected,
    Escalated,
    // 检验与工具
    ToolCalled,
    ToolResult,
    TestRan,
    CheckOverridden,
    // 安装助手
    InstallRequested,
    InstallCompleted,
    InstallRejected,
    // 消息与控制
    OwnerMessage,
    AgentMessage,
    OwnerCommand,
    // 快速通道
    FastpathDispatched,
    PackUpgraded,
    // 自治与提案
    AutonomyChanged,
    ProposalQueued,
    ProposalReviewed,
    ProposalStamped,
    ProposalRejected,
    ProposalActivated,
    ProposalRolledBack,
    // 发布与系统
    PublishRequested,
    PublishConfirmed,
    PublishRejected,
    PublishFailed,
    BaselineMerged,
    UsageCapHit,
    TeamSlept,
    ReturnSummary,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub project_id: String,
    pub stage_run_id: Option<String>,
    pub agent_id: Option<String>,
    pub kind: EventKind,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub id: i64,
    pub author: String, // "owner" 或 agent_id
    pub body: String,
    pub tokens: Vec<MessageToken>,
    pub created_at: String,
}

/// composer 解析出的结构化 token：@点名 / #路径指针。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageToken {
    Mention { agent_role: String },
    PathRef { path: String },
}

/// 时间线条目：事件行，若是消息事件则带消息体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineItem {
    pub event: Event,
    pub message: Option<MessageRow>,
}

/// EventKind 在库里存裸 snake_case 字符串（不是 JSON 带引号形式）。
fn kind_str(k: EventKind) -> String {
    serde_json::to_string(&k)
        .unwrap()
        .trim_matches('"')
        .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// 轨迹导出过滤（US54）：阶段 / Agent / kind 三维，None = 不过滤。
#[derive(Debug, Default, Clone)]
pub struct ExportFilter {
    pub stage_run_id: Option<String>,
    pub agent_id: Option<String>,
    pub kinds: Option<Vec<EventKind>>,
}

impl Db {
    /// 追加事件。events.id 自增即全序。
    pub fn append_event(
        &self,
        project_id: &str,
        kind: EventKind,
        payload: Value,
        agent_id: Option<&str>,
        stage_run_id: Option<&str>,
    ) -> Result<i64, TraceError> {
        self.conn().execute(
            "INSERT INTO events (project_id, stage_run_id, agent_id, kind, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                project_id,
                stage_run_id,
                agent_id,
                kind_str(kind),
                serde_json::to_string(&payload)?,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// 写消息：messages 行 + 一个配对事件（时间线序因此只需 events.id）。
    pub fn append_message(
        &self,
        project_id: &str,
        author: &str,
        body: &str,
        tokens: &[MessageToken],
        agent_id: Option<&str>,
        stage_run_id: Option<&str>,
    ) -> Result<i64, TraceError> {
        let tx = self.conn().unchecked_transaction()?;
        tx.execute(
            "INSERT INTO messages (project_id, author, body, tokens) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![project_id, author, body, serde_json::to_string(&tokens)?],
        )?;
        let msg_id = tx.last_insert_rowid();
        let kind = if author == "owner" {
            EventKind::OwnerMessage
        } else {
            EventKind::AgentMessage
        };
        tx.execute(
            "INSERT INTO events (project_id, stage_run_id, agent_id, kind, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                project_id,
                stage_run_id,
                agent_id,
                kind_str(kind),
                serde_json::to_string(&serde_json::json!({ "message_id": msg_id }))?,
            ],
        )?;
        tx.commit()?;
        Ok(msg_id)
    }

    /// 时间线投影：按 events.id 归并事件与消息。`after_id` 分页，`kinds` 过滤。
    pub fn timeline(
        &self,
        project_id: &str,
        after_id: Option<i64>,
        limit: usize,
        kinds: Option<&[EventKind]>,
    ) -> Result<Vec<TimelineItem>, TraceError> {
        let kind_filter = kinds.map(|ks| {
            ks.iter()
                .map(|k| format!("'{}'", serde_json::to_string(k).unwrap().trim_matches('"')))
                .collect::<Vec<_>>()
                .join(",")
        });
        let sql = format!(
            "SELECT e.id, e.project_id, e.stage_run_id, e.agent_id, e.kind, e.payload, e.created_at,
                    m.id, m.author, m.body, m.tokens, m.created_at
             FROM events e
             LEFT JOIN messages m
               ON m.id = json_extract(e.payload, '$.message_id')
             WHERE e.project_id = ?1
               AND (?2 IS NULL OR e.id > ?2)
               {}
             ORDER BY e.id ASC LIMIT ?3",
            kind_filter
                .map(|f| format!("AND e.kind IN ({f})"))
                .unwrap_or_default()
        );
        let mut st = self.conn().prepare(&sql)?;
        let rows = st.query_map(rusqlite::params![project_id, after_id, limit as i64], |r| {
            let kind_str: String = r.get(4)?;
            let payload_str: String = r.get(5)?;
            let msg_tokens: Option<String> = r.get(10)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                kind_str,
                payload_str,
                r.get::<_, String>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                msg_tokens,
                r.get::<_, Option<String>>(11)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, pid, srid, aid, kind, payload, at, mid, author, body, tokens, mat) = row?;
            out.push(TimelineItem {
                event: Event {
                    id,
                    project_id: pid,
                    stage_run_id: srid,
                    agent_id: aid,
                    kind: serde_json::from_str(&format!("\"{kind}\""))?,
                    payload: serde_json::from_str(&payload)?,
                    created_at: at,
                },
                message: match (mid, author, body) {
                    (Some(mid), Some(author), Some(body)) => Some(MessageRow {
                        id: mid,
                        author,
                        body,
                        tokens: serde_json::from_str(&tokens.unwrap_or_else(|| "[]".into()))?,
                        created_at: mat.unwrap_or_default(),
                    }),
                    _ => None,
                },
            });
        }
        Ok(out)
    }

    /// 轨迹导出（US54）：过滤后事件集写 JSON 文件供回放核对。
    /// 提示词全文从不落库，导出天然不含提示词/凭据正文。
    /// 返回导出条数。
    pub fn export_events(
        &self,
        project_id: &str,
        dest: &std::path::Path,
        filter: &ExportFilter,
    ) -> Result<usize, TraceError> {
        let kind_filter = filter.kinds.as_ref().map(|ks| {
            ks.iter()
                .map(|k| format!("'{}'", serde_json::to_string(k).unwrap().trim_matches('"')))
                .collect::<Vec<_>>()
                .join(",")
        });
        let sql = format!(
            "SELECT e.id, e.stage_run_id, e.agent_id, e.kind, e.payload, e.created_at,
                    m.id, m.author, m.body, m.tokens
             FROM events e
             LEFT JOIN messages m
               ON m.id = json_extract(e.payload, '$.message_id')
             WHERE e.project_id = ?1
               AND (?2 IS NULL OR e.stage_run_id = ?2)
               AND (?3 IS NULL OR e.agent_id = ?3)
               {}
             ORDER BY e.id ASC",
            kind_filter
                .map(|f| format!("AND e.kind IN ({f})"))
                .unwrap_or_default()
        );
        let mut st = self.conn().prepare(&sql)?;
        let rows = st.query_map(
            rusqlite::params![project_id, filter.stage_run_id, filter.agent_id],
            |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "stage_run_id": r.get::<_, Option<String>>(1)?,
                    "agent_id": r.get::<_, Option<String>>(2)?,
                    "kind": r.get::<_, String>(3)?,
                    "payload": serde_json::from_str::<Value>(&r.get::<_, String>(4)?)
                        .unwrap_or(Value::Null),
                    "created_at": r.get::<_, String>(5)?,
                    "message": match (r.get::<_, Option<i64>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?) {
                        (Some(mid), Some(author), Some(body)) => serde_json::json!({
                            "id": mid, "author": author, "body": body,
                            "tokens": serde_json::from_str::<Value>(
                                &r.get::<_, Option<String>>(9)?.unwrap_or_else(|| "[]".into())
                            ).unwrap_or(Value::Null),
                        }),
                        _ => Value::Null,
                    },
                }))
            },
        )?;
        let events: Vec<Value> = rows.collect::<Result<_, _>>()?;
        let doc = serde_json::json!({
            "format": "hexagon-trace-export",
            "version": 1,
            "project_id": project_id,
            "exported_at": self.conn().query_row(
                "SELECT datetime('now')", [], |r| r.get::<_, String>(0)
            )?,
            "count": events.len(),
            "events": events,
        });
        std::fs::write(dest, serde_json::to_string_pretty(&doc)?)?;
        Ok(doc["count"].as_u64().unwrap() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_ids_are_monotonic() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        let a = db
            .append_event("p1", EventKind::AgentActivated, json!({}), None, None)
            .unwrap();
        let b = db
            .append_event("p1", EventKind::AgentSlept, json!({}), None, None)
            .unwrap();
        assert!(b > a);
    }

    #[test]
    fn timeline_merges_messages_in_order() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.append_event(
            "p1",
            EventKind::StageStarted,
            json!({"name":"规格"}),
            None,
            None,
        )
        .unwrap();
        db.append_message(
            "p1",
            "owner",
            "先写规格",
            &[MessageToken::Mention {
                agent_role: "产品策划".into(),
            }],
            None,
            None,
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::ArtifactDelivered,
            json!({"path":"specs/prd.md"}),
            None,
            None,
        )
        .unwrap();

        let items = db.timeline("p1", None, 50, None).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].event.kind, EventKind::StageStarted);
        let msg = items[1].message.as_ref().unwrap();
        assert_eq!(msg.body, "先写规格");
        assert_eq!(
            msg.tokens[0],
            MessageToken::Mention {
                agent_role: "产品策划".into()
            }
        );
        assert_eq!(items[2].event.kind, EventKind::ArtifactDelivered);
    }

    #[test]
    fn timeline_filters_by_kind_and_pages() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        for i in 0..5 {
            db.append_event("p1", EventKind::System, json!({"i": i}), None, None)
                .unwrap();
        }
        db.append_event("p1", EventKind::Stamped, json!({}), None, None)
            .unwrap();
        let only = db
            .timeline("p1", None, 50, Some(&[EventKind::Stamped]))
            .unwrap();
        assert_eq!(only.len(), 1);
        let page = db.timeline("p1", Some(2), 2, None).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].event.id, 3);
    }

    /// US54：轨迹导出——过滤维度（阶段/Agent/kind）生效，文件可回放核对。
    #[test]
    fn us54_export_events_filters_and_replays() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO stage_runs (id, project_id, seq, stage_name, state) VALUES ('r1','p1',0,'规格','active'),('r2','p1',1,'实现','active');
                 INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','后端','active'),('a2','p1','前端','active');",
            )
            .unwrap();
        db.append_event(
            "p1",
            EventKind::StageStarted,
            json!({"stage":0}),
            None,
            Some("r1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::TurnStarted,
            json!({}),
            Some("a1"),
            Some("r1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::TurnFinished,
            json!({}),
            Some("a1"),
            Some("r1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::TurnStarted,
            json!({}),
            Some("a2"),
            Some("r2"),
        )
        .unwrap();
        db.append_event("p1", EventKind::Stamped, json!({}), None, Some("r2"))
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("trace.json");

        // 无过滤：全量 5 条，按 id 升序可回放
        let n = db
            .export_events("p1", &dest, &ExportFilter::default())
            .unwrap();
        assert_eq!(n, 5);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&dest).unwrap()).unwrap();
        assert_eq!(doc["format"], "hexagon-trace-export");
        assert_eq!(doc["count"], 5);
        let kinds: Vec<&str> = doc["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "stage_started",
                "turn_started",
                "turn_finished",
                "turn_started",
                "stamped"
            ]
        );

        // 过滤：阶段 r1 → 3 条
        let n = db
            .export_events(
                "p1",
                &dest,
                &ExportFilter {
                    stage_run_id: Some("r1".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(n, 3);
        // 过滤：Agent a2 → 1 条
        let n = db
            .export_events(
                "p1",
                &dest,
                &ExportFilter {
                    agent_id: Some("a2".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(n, 1);
        // 过滤：kind=turn_started → 2 条
        let n = db
            .export_events(
                "p1",
                &dest,
                &ExportFilter {
                    kinds: Some(vec![EventKind::TurnStarted]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(n, 2);
    }
}
