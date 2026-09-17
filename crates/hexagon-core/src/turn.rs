//! 回合内核：提示词装配（优先级链）+ 窄上下文 + tool-loop。
//!
//! 优先级链（高→低）：工作台约束 > 流程包 > AGENTS.md > 角色定义 > 激活简报。
//! 同 key 的层冲突时高层胜出、低层整段丢弃；无 key 层全部保留。
//!
//! 休眠语义：status='sleeping' 的 Agent 不调度、不召模型——run_turn 直接
//! 返回，provider 零调用。

use crate::db::Db;
use crate::provider::{
    ChatRequest, ChatResponse, ContentBlock, Message, ModelProvider, Role, StopReason,
};
use crate::tools::{CallOutcome, Registry, ToolContext, ToolError};
use crate::trace::{EventKind, MessageToken, TraceError};
use serde_json::{json, Value};

const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Provider(#[from] crate::provider::ProviderError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent not found: {0}")]
    NoAgent(String),
}

// ---------- 提示词装配：优先级链 ----------

/// 层级，数值越小优先级越高。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerLevel {
    Workbench = 0,
    Pack = 1,
    AgentsMd = 2,
    RoleDef = 3,
    Brief = 4,
}

impl LayerLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Workbench => "workbench",
            Self::Pack => "pack",
            Self::AgentsMd => "agents.md",
            Self::RoleDef => "role",
            Self::Brief => "brief",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptLayer {
    pub level: LayerLevel,
    /// 同 key 冲突时高层胜出；None = 不参与冲突判定的自由段落。
    pub key: Option<String>,
    pub text: String,
}

impl PromptLayer {
    pub fn new(level: LayerLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            key: None,
            text: text.into(),
        }
    }
    pub fn keyed(level: LayerLevel, key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            level,
            key: Some(key.into()),
            text: text.into(),
        }
    }
}

/// 装配系统提示词：key 冲突只留最高层；输出按层级高→低排序、带段标。
pub fn build_system_prompt(layers: Vec<PromptLayer>) -> String {
    let mut per_key: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, l) in layers.iter().enumerate() {
        if let Some(k) = &l.key {
            let e = per_key.entry(k.clone()).or_insert(i);
            if layers[*e].level > l.level {
                *e = i;
            }
        }
    }
    let winning: std::collections::HashSet<usize> = per_key.values().copied().collect();
    let mut out: Vec<&PromptLayer> = layers
        .iter()
        .enumerate()
        .filter(|(i, l)| l.key.is_none() || winning.contains(i))
        .map(|(_, l)| l)
        .collect();
    out.sort_by_key(|l| l.level);
    out.iter()
        .map(|l| format!("## {}\n{}", l.level.label(), l.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---------- 窄上下文 ----------

/// 激活简报上下文：只装指针，不装全文。
#[derive(Debug)]
pub struct BriefContext {
    /// 当前阶段已交付产物的指针（path + kind + version）
    pub artifacts: Vec<Value>,
    /// 上游交接说明（当前阶段产物的 upstream 链）
    pub upstream: Vec<Value>,
    /// 点名/回复该 Agent 的消息
    pub mentions: Vec<String>,
    /// 唤醒/打回通知
    pub notices: Vec<Value>,
}

pub fn build_brief_context(
    db: &Db,
    agent_id: &str,
    stage_run_id: Option<&str>,
) -> Result<BriefContext, TurnError> {
    let role: String =
        db.conn()
            .query_row("SELECT role FROM agents WHERE id = ?1", [agent_id], |r| {
                r.get(0)
            })?;
    let project_id: String = db.conn().query_row(
        "SELECT project_id FROM agents WHERE id = ?1",
        [agent_id],
        |r| r.get(0),
    )?;

    let artifacts = {
        let mut st = db.conn().prepare(
            "SELECT path, kind, version FROM artifacts
             WHERE project_id = ?1 AND status IN ('valid','stamped')
             AND (?2 IS NULL OR stage_run_id = ?2) ORDER BY created_at",
        )?;
        let rows = st.query_map(rusqlite::params![project_id, stage_run_id], |r| {
            Ok(json!({"path": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?, "version": r.get::<_,i64>(2)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let upstream = {
        let mut st = db.conn().prepare(
            "SELECT a.path, a.kind, u.path FROM artifacts a
             JOIN artifacts u ON u.id = a.upstream_id
             WHERE a.project_id = ?1 AND (?2 IS NULL OR a.stage_run_id = ?2)",
        )?;
        let rows = st.query_map(rusqlite::params![project_id, stage_run_id], |r| {
            Ok(json!({"path": r.get::<_,String>(0)?, "kind": r.get::<_,String>(1)?, "upstream": r.get::<_,String>(2)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    // 点名消息：tokens JSON 里含该角色 mention
    let mentions = {
        let mut st = db
            .conn()
            .prepare("SELECT body, tokens FROM messages WHERE project_id = ?1 ORDER BY id")?;
        let rows = st
            .query_map([project_id.clone()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .filter(|(_, tokens)| {
                serde_json::from_str::<Vec<MessageToken>>(tokens)
                .unwrap_or_default()
                .iter()
                .any(|t| matches!(t, MessageToken::Mention { agent_role } if agent_role == &role))
            })
            .map(|(body, _)| body)
            .collect()
    };

    // 唤醒/打回通知：指向该 Agent 的裁决/唤醒事件
    let notices = {
        let mut st = db.conn().prepare(
            "SELECT kind, payload FROM events
             WHERE project_id = ?1 AND agent_id = ?2
             AND kind IN ('flag_adjudicated','consult_wakeup','review_rejected')
             ORDER BY id",
        )?;
        let rows = st
            .query_map(rusqlite::params![project_id, agent_id], |r| {
                Ok(json!({"kind": r.get::<_,String>(0)?, "payload": r.get::<_,String>(1)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    Ok(BriefContext {
        artifacts,
        upstream,
        mentions,
        notices,
    })
}

// ---------- 回合执行 ----------

#[derive(Debug, PartialEq)]
pub enum TurnOutcome {
    /// EndTurn：文本已上时间线
    Finished,
    /// 有工具调用转必问，回合挂起（qid）
    AwaitingPermission(String),
    /// 失败（供应商错/限步/工具错）
    Failed(String),
    /// 休眠：零调用直接返回
    SkippedSleeping,
    /// 用量触顶：硬闸，零调用（票 12）
    SkippedCap,
}

/// 跑一个回合。provider 是接缝（测试给 ScriptedProvider）。
pub fn run_turn(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
) -> Result<TurnOutcome, TurnError> {
    // 休眠语义：不调度、不召模型
    let (status, model_slot): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT status, model_slot FROM agents WHERE id = ?1",
            [&ctx.agent_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| TurnError::NoAgent(ctx.agent_id.clone()))?;
    if status == "sleeping" {
        return Ok(TurnOutcome::SkippedSleeping);
    }
    // 用量上限硬闸：触顶即全员休眠，压过自治档位与进行中的激活
    if crate::usage::enforce_cap(db, &ctx.project_id)? {
        log::warn!("turn blocked by usage cap: agent={}", ctx.agent_id);
        return Ok(TurnOutcome::SkippedCap);
    }
    log::info!(
        "turn start: agent={} run={:?}",
        ctx.agent_id,
        ctx.stage_run_id
    );

    db.append_event(
        &ctx.project_id,
        EventKind::TurnStarted,
        json!({ "agent": ctx.agent_id }),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;

    let brief = build_brief_context(db, &ctx.agent_id, ctx.stage_run_id.as_deref())?;
    let system = build_system_prompt(layers);
    let mut messages = vec![
        Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: system }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: json!({
                    "instruction": user_input,
                    "context": {
                        "artifacts": brief.artifacts,
                        "upstream": brief.upstream,
                        "mentions": brief.mentions,
                        "notices": brief.notices,
                    }
                })
                .to_string(),
            }],
        },
    ];

    let req_base = ChatRequest {
        model_slot: model_slot.unwrap_or_else(|| "default".into()),
        messages: vec![],
        tools: registry.defs(),
    };

    let outcome = (|| -> Result<TurnOutcome, TurnError> {
        for round in 0..MAX_TOOL_ROUNDS {
            log::debug!(
                "model call round={round} agent={} slot={}",
                ctx.agent_id,
                req_base.model_slot
            );
            let resp = provider.complete(&ChatRequest {
                messages: messages.clone(),
                ..req_base.clone()
            })?;
            log::debug!(
                "model resp: stop={:?} prompt_tok={} completion_tok={}",
                resp.stop,
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens
            );
            crate::usage::record(db, ctx, &req_base.model_slot, &resp.usage, 0)?;
            messages.push(Message {
                role: Role::Assistant,
                content: resp.content.clone(),
            });

            let tool_uses: Vec<_> = resp
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();

            if resp.stop != StopReason::ToolUse && tool_uses.is_empty() {
                // 回合结束：文本上时间线
                let text: String = resp
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    db.append_message(
                        &ctx.project_id,
                        &ctx.agent_id,
                        &text,
                        &[],
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                }
                return Ok(TurnOutcome::Finished);
            }

            // 执行工具调用，结果回喂
            let mut results = Vec::new();
            for (id, name, input) in tool_uses {
                match registry.call(db, ctx, &name, input)? {
                    CallOutcome::Done(v) => results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: v.to_string(),
                        is_error: false,
                    }),
                    CallOutcome::Denied(reason) => results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: format!("denied: {reason}"),
                        is_error: true,
                    }),
                    CallOutcome::Asked(qid) => {
                        return Ok(TurnOutcome::AwaitingPermission(qid));
                    }
                }
            }
            let tool_bytes: usize = results
                .iter()
                .map(|b| match b {
                    ContentBlock::ToolResult { content, .. } => content.len(),
                    _ => 0,
                })
                .sum();
            crate::usage::record(
                db,
                ctx,
                &req_base.model_slot,
                &Default::default(),
                tool_bytes,
            )?;
            messages.push(Message {
                role: Role::Tool,
                content: results,
            });
        }
        Ok(TurnOutcome::Failed(format!(
            "tool-loop exceeded {MAX_TOOL_ROUNDS} rounds"
        )))
    })();

    db.append_event(
        &ctx.project_id,
        match &outcome {
            Ok(TurnOutcome::Finished) => EventKind::TurnFinished,
            Ok(TurnOutcome::AwaitingPermission(_)) => EventKind::TurnFinished,
            Ok(TurnOutcome::SkippedSleeping) => EventKind::TurnFinished,
            Ok(TurnOutcome::SkippedCap) => EventKind::TurnFinished,
            Ok(TurnOutcome::Failed(_)) | Err(_) => EventKind::TurnFailed,
        },
        json!({ "agent": ctx.agent_id, "outcome": format!("{outcome:?}") }),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    outcome
}

// 供测试构造响应
pub fn text_response(text: &str) -> ChatResponse {
    ChatResponse {
        content: vec![ContentBlock::Text { text: text.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    }
}
pub fn tool_response(calls: Vec<(&str, &str, Value)>) -> ChatResponse {
    ChatResponse {
        content: calls
            .into_iter()
            .map(|(id, name, input)| ContentBlock::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            })
            .collect(),
        stop: StopReason::ToolUse,
        usage: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::tools::ToolContext;

    fn setup() -> (Db, Registry, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a1','p1','后端开发','active')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (
            db,
            Registry::builtin(),
            ToolContext {
                project_id: "p1".into(),
                agent_id: "a1".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: crate::artifacts::TierMap::new(),
            },
            dir,
        )
    }

    #[test]
    fn higher_layer_wins_key_conflict() {
        let prompt = build_system_prompt(vec![
            PromptLayer::keyed(LayerLevel::Brief, "commit_style", "no-verify 提交"),
            PromptLayer::keyed(LayerLevel::Workbench, "commit_style", "永远带签名提交"),
            PromptLayer::keyed(LayerLevel::Pack, "output", "产物写 .hexagon/"),
            PromptLayer::new(LayerLevel::RoleDef, "你是后端开发"),
        ]);
        assert!(prompt.contains("永远带签名提交"));
        assert!(!prompt.contains("no-verify"));
        assert!(prompt.contains("产物写 .hexagon/"));
        assert!(prompt.contains("你是后端开发"));
        // 层级序：workbench 段在最前
        assert!(prompt.find("workbench").unwrap() < prompt.find("role").unwrap());
    }

    #[test]
    fn end_to_end_turn_brief_model_tool_artifact() {
        let (db, reg, ctx, dir) = setup();
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![(
                "t1",
                "artifact_write",
                json!({"path":"impl/plan.md","content":"# 方案\n自由档产物"}),
            )]),
            text_response("已交付实现方案"),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "写个方案").unwrap();
        assert_eq!(out, TurnOutcome::Finished);
        assert!(dir.path().join(".hexagon/impl/plan.md").exists());
        // 全程事件可回放
        let items = db.timeline("p1", None, 50, None).unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
        for k in [
            EventKind::TurnStarted,
            EventKind::ToolCalled,
            EventKind::ToolResult,
            EventKind::AgentMessage,
            EventKind::TurnFinished,
        ] {
            assert!(kinds.contains(&k), "missing {k:?}");
        }
        // 供应商收到 2 次调用：第一次带工具清单
        assert_eq!(provider.recorded().len(), 2);
        assert!(!provider.recorded()[0].tools.is_empty());
    }

    #[test]
    fn usage_cap_blocks_scheduling() {
        let (db, reg, ctx, dir) = setup();
        // 设 1 分上限 + 高价表，先记一笔超限账
        std::fs::create_dir_all(dir.path().join(".hexagon")).unwrap();
        std::fs::write(
            dir.path().join(".hexagon/prices.json"),
            r#"{"default":{"prompt_per_1k_mc":2000,"completion_per_1k_mc":0}}"#,
        )
        .unwrap();
        db.conn()
            .execute("UPDATE projects SET usage_limit_cents=1 WHERE id='p1'", [])
            .unwrap();
        crate::usage::record(
            &db,
            &ctx,
            "chat",
            &crate::provider::Usage {
                prompt_tokens: 1000,
                completion_tokens: 0,
            },
            0,
        )
        .unwrap();
        let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "继续").unwrap();
        assert_eq!(out, TurnOutcome::SkippedCap);
        assert!(provider.recorded().is_empty());
        // 触顶事件 + 全员休眠已落
        let kinds: Vec<_> = db
            .timeline("p1", None, 50, None)
            .unwrap()
            .iter()
            .map(|i| i.event.kind)
            .collect();
        assert!(kinds.contains(&EventKind::UsageCapHit));
        assert!(kinds.contains(&EventKind::TeamSlept));
    }

    #[test]
    fn sleeping_agent_makes_zero_model_calls() {
        let (db, reg, ctx, _dir) = setup();
        db.conn()
            .execute("UPDATE agents SET status='sleeping' WHERE id='a1'", [])
            .unwrap();
        let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap();
        assert_eq!(out, TurnOutcome::SkippedSleeping);
        assert!(provider.recorded().is_empty());
        // 连 TurnStarted 都没有——根本没被调度
        assert!(db
            .timeline("p1", None, 50, Some(&[EventKind::TurnStarted]))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn ask_pauses_turn_with_question() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![tool_response(vec![(
            "t1",
            "bash",
            json!({"cmd":"cargo build"}),
        )])]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "构建").unwrap();
        let TurnOutcome::AwaitingPermission(qid) = out else {
            panic!("expected ask, got {out:?}")
        };
        assert!(qid.starts_with('q'));
    }

    #[test]
    fn denied_tool_result_feeds_back() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![("t1", "fs_read", json!({"path":".env"}))]),
            text_response("被拒绝了"),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "读 env").unwrap();
        assert_eq!(out, TurnOutcome::Finished);
        // 第二轮请求的 messages 里带 is_error 的 ToolResult
        let second = &provider.recorded()[1];
        let has_err = second.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { is_error: true, .. }))
        });
        assert!(has_err);
    }

    #[test]
    fn narrow_context_carries_pointers_not_bodies() {
        let (db, reg, ctx, _dir) = setup();
        // 塞一个产物行 + 一条点名消息
        db.conn()
            .execute(
                "INSERT INTO artifacts (id, project_id, path, kind, tier, version)
             VALUES ('art1','p1','specs/prd.md','规格','skeleton',1)",
                [],
            )
            .unwrap();
        db.append_message(
            "p1",
            "owner",
            "看一下规格 @后端开发",
            &[MessageToken::Mention {
                agent_role: "后端开发".into(),
            }],
            None,
            None,
        )
        .unwrap();
        db.append_message("p1", "owner", "无关消息", &[], None, None)
            .unwrap();

        let provider = ScriptedProvider::new(vec![text_response("ok")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "继续").unwrap();
        let req = &provider.recorded()[0];
        let ctx_text = match &req.messages[1].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!(),
        };
        // 有产物指针，无产物全文
        assert!(ctx_text.contains("specs/prd.md"));
        // 有点名消息，无无关消息
        assert!(ctx_text.contains("看一下规格"));
        assert!(!ctx_text.contains("无关消息"));
    }
}
