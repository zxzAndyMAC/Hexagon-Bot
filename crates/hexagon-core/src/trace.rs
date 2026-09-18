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
}
