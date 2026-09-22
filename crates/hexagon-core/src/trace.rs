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
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
    ReviewerVerdict,
    ReviewerTripped,
    PermissionAsked,
    PermissionAllowed,
    PermissionDenied,
    PermissionShapeRemembered,
    // ui-audit-2 票 03：已记规则的撤销也留痕——审计面不能自身是盲区。
    PermissionRuleRevoked,
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
    /// 票 08：项目经理对没点名的负责人发言做的封闭选择（派给谁 / 先不派活）。
    PmRouted,
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

/// 失败/打回理由闭集（rsi-research 票 02；仿 DSH `TurnEndReason`/
/// `GoalBlockReason` 的 code+message 分离）：code 是机器可判的稳定词表，
/// message 永远自由文本给人看。自治裁决（repairable→可自动回填）与回放
/// 报告的结局分类共用同一词表——词表只增不改，消费方按字面匹配。
/// 出处：`.scratch/rsi-research/issues/02-failure-taxonomy.md`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    /// 可修复：声明的回路能自愈（回填边拨回、复审通道返工）
    Repairable,
    /// 歧义：机器无法裁决，需人看（无复审者/重复打回/意外失败）
    Ambiguous,
    /// 硬阻断：结构上不可继续（异议对象是已盖章产物、熔断停回合）
    HardBlocked,
    /// 预算/容量触顶（上下文撞限、用量触顶、输出截断）
    BudgetExceeded,
}

impl FailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repairable => "repairable",
            Self::Ambiguous => "ambiguous",
            Self::HardBlocked => "hard-blocked",
            Self::BudgetExceeded => "budget-exceeded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct Event {
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub id: i64,
    pub project_id: String,
    pub stage_run_id: Option<String>,
    pub agent_id: Option<String>,
    pub kind: EventKind,
    /// wire 上恒为对象（append_event 序列化点兜底 {}）；导出类型定死。
    #[ts(type = "Record<string, unknown>")]
    pub payload: Value,
    pub created_at: String,
}

/// 消息图片附件引用（agent-senses 票 03）：字节本体在
/// `.hexagon/inbox/` 文件里，行内只存引用（见迁移 0013 注释）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct AttachRef {
    pub media_type: String,
    /// 仓内相对路径（.hexagon/inbox/…）。
    pub path: String,
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub bytes: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct MessageRow {
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub id: i64,
    pub author: String, // "owner" 或 agent_id
    pub body: String,
    pub tokens: Vec<MessageToken>,
    /// 票 03：图片附件引用（无附件为 []）。
    pub attachments: Vec<AttachRef>,
    pub created_at: String,
    /// 模型给出的推理文本（hands-free 票 06）。空串 = 没给，时间线不渲染思考行。
    pub thinking: String,
}

/// composer 解析出的结构化 token：@点名 / #路径指针。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum MessageToken {
    Mention { agent_role: String },
    PathRef { path: String },
}

/// 时间线条目：事件行，若是消息事件则带消息体。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
}

/// 轨迹导出过滤（US54）：阶段 / Agent / kind 三维，None = 不过滤。
#[derive(Debug, Default, Clone, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExportFilter {
    #[ts(optional)]
    pub stage_run_id: Option<String>,
    #[ts(optional)]
    pub agent_id: Option<String>,
    #[ts(optional)]
    pub kinds: Option<Vec<EventKind>>,
}

impl Db {
    /// 追加事件。events.id 自增即全序。
    /// 事件载荷上限（票 12）：超限完整落 spill 文件、库里留指针——
    /// 对齐工具结果 spill 设施，事件表不被巨型 meta 撑爆。
    const EVENT_PAYLOAD_CAP: usize = 64 * 1024;

    /// 事件 schema 版本（迁移 0012）：payload 形状演化时递增——回放/导出
    /// 按列区分旧格式，不用解析 JSON 猜形状（对齐 aisuite TRACE_SCHEMA_VERSION）。
    const EVENT_SCHEMA_VERSION: i64 = 1;

    pub fn append_event(
        &self,
        project_id: &str,
        kind: EventKind,
        payload: Value,
        agent_id: Option<&str>,
        stage_run_id: Option<&str>,
    ) -> Result<i64, TraceError> {
        let mut text = serde_json::to_string(&payload)?;
        if text.len() > Self::EVENT_PAYLOAD_CAP {
            text = self.spill_payload(project_id, &text);
        }
        self.conn().execute(
            "INSERT INTO events (project_id, stage_run_id, agent_id, kind, payload, schema_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                project_id,
                stage_run_id,
                agent_id,
                kind_str(kind),
                text,
                Self::EVENT_SCHEMA_VERSION,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// 超限载荷落 .hexagon/spill/event-<hash>.json，返回指针 JSON 文本。
    /// 项目目录不可得 → 截断保指针信息（fail toward 留痕不丢事件）。
    fn spill_payload(&self, project_id: &str, text: &str) -> String {
        let dir: Option<String> = self
            .conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
                r.get(0)
            })
            .ok();
        if let Some(dir) = dir {
            // 内容寻址文件名：同载荷幂等重写、无时间戳。
            let mut h: u64 = 0xcbf29ce484222325;
            for b in text.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            let spill = std::path::Path::new(&dir).join(".hexagon/spill");
            if std::fs::create_dir_all(&spill).is_ok() {
                let gi = spill.join(".gitignore");
                if !gi.exists() {
                    let _ = std::fs::write(&gi, "*\n");
                }
                let file = spill.join(format!("event-{h:016x}.json"));
                if std::fs::write(&file, text).is_ok() {
                    return serde_json::to_string(&serde_json::json!({
                        "spilled": file.to_string_lossy(),
                        "bytes": text.len(),
                    }))
                    .unwrap_or_default();
                }
            }
        }
        // spill 写不动：截断 + 标记，事件本身不丢
        let mut end = Self::EVENT_PAYLOAD_CAP.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{{\"truncated_payload\":{:?},\"bytes\":{}}}",
            &text[..end],
            text.len()
        )
    }

    /// 写消息：messages 行 + 一个配对事件（时间线序因此只需 events.id）。
    /// 票 03：`attachments` 是图片附件引用（字节在 .hexagon/inbox/ 文件，
    /// 行内只存 {media_type,path,bytes,name}——见迁移 0013）。
    #[allow(clippy::too_many_arguments)] // 消息行七元组都是正交字段
    pub fn append_message(
        &self,
        project_id: &str,
        author: &str,
        body: &str,
        tokens: &[MessageToken],
        attachments: &[AttachRef],
        agent_id: Option<&str>,
        stage_run_id: Option<&str>,
    ) -> Result<i64, TraceError> {
        self.append_message_with(
            project_id,
            author,
            body,
            tokens,
            attachments,
            agent_id,
            stage_run_id,
            None,
        )
    }

    /// 带思考的消息（hands-free 票 06）。`thinking` 空或 None = 不落推理文本。
    #[allow(clippy::too_many_arguments)]
    pub fn append_message_with(
        &self,
        project_id: &str,
        author: &str,
        body: &str,
        tokens: &[MessageToken],
        attachments: &[AttachRef],
        agent_id: Option<&str>,
        stage_run_id: Option<&str>,
        thinking: Option<&str>,
    ) -> Result<i64, TraceError> {
        let thinking = thinking.filter(|t| !t.is_empty());
        let tx = self.conn().unchecked_transaction()?;
        tx.execute(
            "INSERT INTO messages (project_id, author, body, tokens, attachments, thinking)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                project_id,
                author,
                body,
                serde_json::to_string(&tokens)?,
                serde_json::to_string(&attachments)?,
                thinking,
            ],
        )?;
        let msg_id = tx.last_insert_rowid();
        let kind = if author == "owner" {
            EventKind::OwnerMessage
        } else {
            EventKind::AgentMessage
        };
        let mut payload = serde_json::json!({ "message_id": msg_id });
        // 票 10 taint：外部内容（research/mcp:* 结果）回喂过该 agent 后，
        // 其产出的消息事件打 after_external——下游看得出「读了外部内容后写的」。
        if author != "owner"
            && crate::provenance::tainted(self, agent_id.unwrap_or(""), stage_run_id)
        {
            payload["after_external"] = serde_json::json!(true);
        }
        tx.execute(
            "INSERT INTO events (project_id, stage_run_id, agent_id, kind, payload, schema_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                project_id,
                stage_run_id,
                agent_id,
                kind_str(kind),
                serde_json::to_string(&payload)?,
                Self::EVENT_SCHEMA_VERSION,
            ],
        )?;
        tx.commit()?;
        Ok(msg_id)
    }

    /// 读游标（票 10）：无记录视为 0——首次激活全量读。
    pub fn cursor(&self, agent_id: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT last_event_id FROM agent_cursors WHERE agent_id=?1",
                [agent_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// 推进游标（票 10）：单调只前进。在「brief 真的喂给了模型」之后调——
    /// 推进早于模型收到就丢增量，宁多送不丢（fail toward redelivery）。
    pub fn advance_cursor(&self, agent_id: &str, watermark: i64) -> Result<(), TraceError> {
        self.conn().execute(
            "INSERT INTO agent_cursors (agent_id, last_event_id) VALUES (?1, ?2)
             ON CONFLICT(agent_id) DO UPDATE SET last_event_id=MAX(last_event_id, ?2)",
            rusqlite::params![agent_id, watermark],
        )?;
        Ok(())
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
                    m.id, m.author, m.body, m.tokens, m.created_at, m.attachments, m.thinking
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
                r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<String>>(13)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                pid,
                srid,
                aid,
                kind,
                payload,
                at,
                mid,
                author,
                body,
                tokens,
                mat,
                matt,
                thinking,
            ) = row?;
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
                        attachments: serde_json::from_str(&matt.unwrap_or_else(|| "[]".into()))?,
                        created_at: mat.unwrap_or_default(),
                        thinking: thinking.unwrap_or_default(),
                    }),
                    _ => None,
                },
            });
        }
        Ok(out)
    }

    /// 待决卡队列读模型（ADR 0052 读组）：壳层经控制连接直查。
    /// 数据面归 cards.rs（票 04），本函数只剩转发；行类型化见 ADR 0054。
    pub fn queued_questions(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::cards::QueuedCard>, TraceError> {
        Ok(crate::cards::queued(self, project_id)?)
    }

    /// 事件断言原料（arch-review 票 05）：scenario/验收套件与测试直接消费
    /// ——timeline 全窗 + kind 过滤，剥壳取 Event。
    pub fn events(
        &self,
        project_id: &str,
        kinds: Option<&[EventKind]>,
    ) -> Result<Vec<Event>, TraceError> {
        Ok(self
            .timeline(project_id, None, 10000, kinds)?
            .into_iter()
            .map(|i| i.event)
            .collect())
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
                    m.id, m.author, m.body, m.tokens, m.attachments, m.thinking
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
                            "attachments": serde_json::from_str::<Value>(
                                &r.get::<_, Option<String>>(10)?.unwrap_or_else(|| "[]".into())
                            ).unwrap_or(Value::Null),
                            "thinking": r.get::<_, Option<String>>(11)?,
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
            &[],
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

    // ---------- timeline 属性测试 + System 子 kind 词表（arch 票 08）----------

    /// D10 登记表落地前的 de-facto 词表钉死：System 事件 payload.kind
    /// 的全部合法二级分类。新增 System 子事件的纪律是「先加这里再写
    /// 入点」——漏登会让本测试在生产路径上观测到词表外 kind 时变红。
    const SYSTEM_SUBKINDS: &[&str] = &[
        "attachments_degraded",
        "context_compacted",
        "context_denied",
        "context_resumed",
        "exec_timeout",
        "flag_routed",
        "instructions_degraded",
        "invariant_violation",
        "judge_verdict",
        "known_world",
        "provider_retry",
        "request_envelope",
        "sandbox_unavailable",
        "session_exited",
        "session_started",
        "steering_injected",
        "task_killed",
        "task_started",
        "tool_breaker",
    ];

    /// 词表钉死：跑真实回合（脚本化 provider + 超限 AGENTS.md 触发
    /// instructions_degraded + Transport 抖动触发 provider_retry），
    /// 落库的全部 System 子 kind 必须在登记表内。
    #[test]
    fn system_subkinds_stay_registered() {
        struct Flaky(std::sync::atomic::AtomicUsize);
        impl crate::provider::ModelProvider for Flaky {
            fn complete(
                &self,
                _r: &crate::provider::ChatRequest,
            ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
                Ok(crate::turn::text_response("x"))
            }
            fn stream(
                &self,
                _r: &crate::provider::ChatRequest,
                sink: &mut crate::provider::StreamSink<'_>,
            ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
                if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    return Err(crate::provider::ProviderError::Transport("blip".into()));
                }
                sink(&crate::provider::StreamDelta::Text("ok".into()));
                Ok(crate::turn::text_response("ok"))
            }
        }

        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','后端','active')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        // >32KB 的 AGENTS.md → instructions_degraded 子 kind
        std::fs::write(dir.path().join("AGENTS.md"), "# h\n".repeat(9000)).unwrap();
        let ctx = crate::tools::ToolContext {
            project_id: "p1".into(),
            agent_id: "a1".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
        };
        crate::turn::run_turn(
            &db,
            &Flaky(0.into()),
            &crate::tools::Registry::builtin(),
            &ctx,
            vec![],
            "干活",
        )
        .unwrap();

        let mut st = db
            .conn()
            .prepare(
                "SELECT payload FROM events WHERE kind='system'
                 AND json_extract(payload,'$.kind') IS NOT NULL",
            )
            .unwrap();
        let kinds: Vec<String> = st
            .query_map([], |r| {
                let p: Value = serde_json::from_str(&r.get::<_, String>(0)?).unwrap_or_default();
                Ok(p["kind"].as_str().unwrap_or("").to_string())
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!kinds.is_empty(), "回合应产出 System 子事件");
        for k in &kinds {
            assert!(
                SYSTEM_SUBKINDS.contains(&k.as_str()),
                "词表外 System 子 kind 出库: {k}（先登记 SYSTEM_SUBKINDS 再写入点）"
            );
        }
        // 本回合至少覆盖 request_envelope/provider_retry/instructions_degraded
        for want in [
            "request_envelope",
            "provider_retry",
            "instructions_degraded",
        ] {
            assert!(kinds.iter().any(|k| k == want), "未覆盖 {want}: {kinds:?}");
        }
    }

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;
        use proptest::{collection, sample};

        fn db() -> Db {
            let db = Db::open_in_memory().unwrap();
            db.conn()
                .execute(
                    "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                    [],
                )
                .unwrap();
            db
        }

        fn kind() -> impl Strategy<Value = EventKind> {
            sample::select(vec![
                EventKind::TurnStarted,
                EventKind::AgentActivated,
                EventKind::System,
                EventKind::Stamped,
                EventKind::FlagSubmitted,
                EventKind::StageStarted,
            ])
        }

        proptest! {
            /// 分页无损：任意事件数 × 任意页长，翻页串起来 == 全集，
            /// id 严格递增，每页不超 limit。
            #[test]
            fn pagination_is_lossless(
                n in 0..60usize,
                page in 1..17usize,
            ) {
                let db = db();
                for i in 0..n {
                    db.append_event("p1", EventKind::System, json!({"i": i}), None, None)
                        .unwrap();
                }
                let mut got = Vec::new();
                let mut after = None;
                loop {
                    let items = db.timeline("p1", after, page, None).unwrap();
                    prop_assert!(items.len() <= page);
                    if items.is_empty() {
                        break;
                    }
                    after = items.last().map(|i| i.event.id);
                    got.extend(items.into_iter().map(|i| i.event.id));
                }
                let want: Vec<i64> = (1..=n as i64).collect();
                prop_assert_eq!(got, want);
            }

            /// kind 过滤不漏不混：返回值恒为过滤子集且保持序。
            #[test]
            fn kind_filter_is_exact_subset(
                kinds in collection::vec(kind(), 0..40),
                pick in collection::hash_set(0..6usize, 1..4),
            ) {
                let db = db();
                for k in &kinds {
                    db.append_event("p1", *k, json!({}), None, None).unwrap();
                }
                let all = [
                    EventKind::TurnStarted,
                    EventKind::AgentActivated,
                    EventKind::System,
                    EventKind::Stamped,
                    EventKind::FlagSubmitted,
                    EventKind::StageStarted,
                ];
                let filter: Vec<EventKind> =
                    pick.iter().map(|i| all[i % all.len()]).collect();
                let items = db.timeline("p1", None, 200, Some(&filter)).unwrap();
                let want: Vec<i64> = kinds
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| filter.contains(k))
                    .map(|(i, _)| i as i64 + 1)
                    .collect();
                let got: Vec<i64> = items.iter().map(|i| i.event.id).collect();
                prop_assert_eq!(got, want);
            }

            /// after_id 严格排他：timeline(k) == 第 k 条之后的全集。
            #[test]
            fn after_id_is_exclusive_prefix(n in 0..40usize, k in 0..40usize) {
                let db = db();
                for i in 0..n {
                    db.append_event("p1", EventKind::System, json!({"i": i}), None, None)
                        .unwrap();
                }
                let items = db.timeline("p1", Some(k as i64), 500, None).unwrap();
                let got: Vec<i64> = items.iter().map(|i| i.event.id).collect();
                let want: Vec<i64> =
                    (1..=n as i64).filter(|id| *id > k as i64).collect();
                prop_assert_eq!(got, want);
            }
        }
    }
}
