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
use std::path::Path;

const MAX_TOOL_ROUNDS: usize = 8;
/// 瞬时重试（US57）：Transport 类抖动按 100/250/500ms 指数退避，最多 3 次；
/// Refused（4xx/权限拒绝）、MissingCredential、ScriptExhausted 不重试直接上报。
const RETRY_DELAYS_MS: [u64; 3] = [100, 250, 500];
/// 同工具同错熔断（US57）：连续 N 次相同失败结束回合等负责人。
const BREAKER_STREAK: usize = 3;
/// 上下文估算上限（US37）：~120k tok；超了轻量裁剪后仍超 → 暂停问负责人。
const CONTEXT_CAP_TOKENS: usize = 120_000;
/// 轻量裁剪的单块上限（字符）：超长 tool_result 截断带标记，其余不动。
const TRIM_BLOCK_CHARS: usize = 4_000;

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
    #[error(transparent)]
    Orch(#[from] crate::orchestra::OrchError),
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

/// 项目说明注入上限：超 ~32KB 降级「头部+节标题目录+必读指引」并提醒负责人（US72）。
const INSTRUCTIONS_CAP: usize = 32 * 1024;
/// 降级时保留的头部字节数。
const INSTRUCTIONS_HEAD: usize = 4 * 1024;

/// 截断续推指令（票 13）：不重复推理，直接下一步。
const TRUNCATION_NUDGE: &str = "输出被截断。不要重复推理，直接给下一步。";

/// 动态尾部块（票 14，OPE `_trailing_block`）：易变内容（时间戳、轮次）
/// 独立成末尾 user 消息、发送时才拼——系统提示与历史前缀逐字节稳定，
/// provider prompt cache 才能命中。时间戳这类易变值别塞进系统层。
fn with_dynamic_tail(messages: &[Message], round: usize) -> Vec<Message> {
    let mut out = messages.to_vec();
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    out.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: json!({"env": {"unix_time": secs, "round": round}}).to_string(),
        }],
    });
    out
}

/// 读项目说明（AGENTS.md 优先，CLAUDE.md 次）供激活注入。
/// 返回 (注入文本, 是否降级)；无说明文件返回 None。
pub fn load_instructions(repo_root: &Path) -> Option<(String, bool)> {
    let (name, full) = ["AGENTS.md", "CLAUDE.md"].iter().find_map(|n| {
        std::fs::read_to_string(repo_root.join(n))
            .ok()
            .map(|c| (*n, c))
    })?;
    if full.len() <= INSTRUCTIONS_CAP {
        return Some((format!("{name} 全文：\n{full}"), false));
    }
    // 降级：头部 + 节标题目录 + 必读指引（不做自动全量摘要——影响语义的决策不自动做）
    let mut end = INSTRUCTIONS_HEAD.min(full.len());
    while !full.is_char_boundary(end) {
        end -= 1;
    }
    let headings: Vec<&str> = full
        .lines()
        .filter(|l| l.starts_with('#'))
        .map(|l| l.trim())
        .collect();
    let text = format!(
        "{name}（全文 {} 字节 > 32KB，已降级）\n\
         必读指引：下面是文件头与节标题目录；需要某节细节时用 fs_read 读 {name} 对应位置。\n\
         --- 文件头 ---\n{}\n--- 节标题目录 ---\n{}",
        full.len(),
        &full[..end],
        headings.join("\n")
    );
    Some((text, true))
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
    /// `#` 路径指针：随点名消息携带的仓内路径，Agent 经工具层去读（非全文注入）
    pub paths: Vec<String>,
    /// 唤醒/打回通知
    pub notices: Vec<Value>,
    /// 事件高水位（票 10）：brief 组装时库里的最大事件 id。
    /// 首个 provider 响应到手后推进 agent_cursors 至此——推进早于送达就丢增量。
    pub watermark: i64,
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

    // 票 10：per-agent 事件游标——brief 只增量读「上次读到位置之后」的事件，
    // 不再全表扫 mentions/notices。游标在首个 provider 响应到手后推进
    // （调用方负责 advance_cursor）——推进早了丢增量，晚了只是重送。
    let cursor = db.cursor(agent_id);
    let watermark: i64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1",
            [&project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // 点名消息：tokens JSON 里含该角色 mention；走事件表（消息都有配对事件）
    // 才能用游标过滤——messages.id 与 events.id 不共享序列。
    let (mentions, paths) = {
        let mut st = db.conn().prepare(
            "SELECT m.body, m.tokens FROM events e
             JOIN messages m ON m.id = json_extract(e.payload,'$.message_id')
             WHERE e.project_id = ?1 AND e.id > ?2
             AND e.kind IN ('owner_message','agent_message')
             ORDER BY e.id",
        )?;
        let rows = st
            .query_map(rusqlite::params![project_id.clone(), cursor], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut paths = Vec::new();
        let mentions = rows
            .into_iter()
            .filter(|(_, tokens)| {
                let toks: Vec<MessageToken> = serde_json::from_str(tokens).unwrap_or_default();
                let named = toks.iter().any(
                    |t| matches!(t, MessageToken::Mention { agent_role } if agent_role == &role),
                );
                if named {
                    for t in toks {
                        if let MessageToken::PathRef { path } = t {
                            paths.push(path);
                        }
                    }
                }
                named
            })
            .map(|(body, _)| body)
            .collect();
        (mentions, paths)
    };

    // 唤醒/打回通知：指向该 Agent 的裁决/唤醒事件——同样按游标取增量。
    let notices = {
        let mut st = db.conn().prepare(
            "SELECT kind, payload FROM events
             WHERE project_id = ?1 AND agent_id = ?2 AND id > ?3
             AND kind IN ('flag_adjudicated','consult_wakeup','review_rejected')
             ORDER BY id",
        )?;
        let rows = st
            .query_map(rusqlite::params![project_id, agent_id, cursor], |r| {
                Ok(json!({"kind": r.get::<_,String>(0)?, "payload": r.get::<_,String>(1)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    Ok(BriefContext {
        artifacts,
        upstream,
        mentions,
        paths,
        notices,
        watermark,
    })
}

/// 悬空 tool_use 修复（票 13，OPE `_repair_dangling_tool_calls`）：
/// assistant 消息里没等到结果的每个 ToolUse，紧跟其后补 error
/// tool_result stub——Anthropic 形状 provider 拒收孤儿 tool_use。
/// stub 如实写「中断无结果」，模型需要可自行重发。
/// 截断续推路径的配对保证也走这里：截断回复里的 tool_use 不执行。
/// 返回补的 stub 数。
pub fn repair_dangling_tool_uses(messages: &mut Vec<Message>) -> usize {
    let answered: std::collections::HashSet<String> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect();
    let mut stubs_made = 0usize;
    let mut i = 0;
    while i < messages.len() {
        let dangling: Vec<String> = if matches!(messages[i].role, Role::Assistant) {
            messages[i]
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } if !answered.contains(id) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            vec![]
        };
        if !dangling.is_empty() {
            stubs_made += dangling.len();
            let stubs: Vec<ContentBlock> = dangling
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: "{\"error\":\"tool call interrupted — no result recorded; \
                              re-issue the call if it is still needed\"}"
                        .into(),
                    is_error: true,
                })
                .collect();
            // 后续已有 Tool 消息则并入其头部，否则插一条新 Tool 消息——
            // 保持「assistant → tool_result」紧邻序。
            if i + 1 < messages.len() && matches!(messages[i + 1].role, Role::Tool) {
                let mut merged = stubs;
                merged.append(&mut messages[i + 1].content);
                messages[i + 1].content = merged;
            } else {
                messages.insert(
                    i + 1,
                    Message {
                        role: Role::Tool,
                        content: stubs,
                    },
                );
            }
        }
        i += 1;
    }
    stubs_made
}

// ---------- 回合执行 ----------

/// 可重试的供应商错：只有 Transport 类瞬时抖动；4xx/拒绝/缺凭据/脚本耗尽直传。
fn retryable(e: &crate::provider::ProviderError) -> bool {
    matches!(e, crate::provider::ProviderError::Transport(_))
}

/// 模型调用 + 瞬时重试（US57）。重试全程落 System 事件留痕。
fn complete_with_retry(
    db: &Db,
    ctx: &ToolContext,
    provider: &dyn ModelProvider,
    req: &ChatRequest,
) -> Result<ChatResponse, TurnError> {
    let mut attempt = 0usize;
    loop {
        match provider.complete(req) {
            Ok(resp) => return Ok(resp),
            Err(e) if retryable(&e) && attempt < RETRY_DELAYS_MS.len() => {
                attempt += 1;
                log::warn!(
                    "provider transient error, retry {attempt}/{}: {e}",
                    RETRY_DELAYS_MS.len()
                );
                db.append_event(
                    &ctx.project_id,
                    EventKind::System,
                    json!({"kind": "provider_retry", "attempt": attempt, "err": e.to_string()}),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(
                    RETRY_DELAYS_MS[attempt - 1],
                ));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// 模型可自救的工具错 → is_error 回喂；基建错（trace/db/sqlite）上抛。
fn model_visible(e: &ToolError) -> bool {
    matches!(
        e,
        ToolError::PathEscape(_)
            | ToolError::Io(_)
            | ToolError::BadInput(_)
            | ToolError::Exec(_)
            | ToolError::Artifact(_)
            | ToolError::UnknownQuestion(_)
    )
}

/// 上下文估算（US37）：按 ~4 字符/tok 粗算，只用于撞限判断不用于计费。
fn estimate_tokens(messages: &[Message]) -> usize {
    let chars: usize = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .map(|b| match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
        })
        .sum();
    chars / 4
}

/// 轻量裁剪（US37 + openworker-borrow 票 02）：超长 tool_result 走 spill——
/// 完整内容落 `.hexagon/spill/`，上下文留 head+marker+tail；被裁部分可读回，
/// 不再永久丢失。spill 失败时 spill_trim 内部退回旧式纯截断。
fn trim_context(ctx: &ToolContext, mut messages: Vec<Message>) -> Vec<Message> {
    for m in &mut messages {
        for b in &mut m.content {
            if let ContentBlock::ToolResult { content, .. } = b {
                if content.len() > TRIM_BLOCK_CHARS {
                    *content = crate::tools::spill_trim(ctx, content, TRIM_BLOCK_CHARS);
                }
            }
        }
    }
    messages
}

/// 撞限前的机械降级（openworker-borrow 票 06）：最旧轮次逐字落 transcript
/// 文件，出站视图换机械状态块（trace 派生）+ 首条负责人指令指引。
/// 零模型参与——守 US37「影响语义的决策不自动做」，compaction.py 的
/// LLM 摘要部分刻意不搬。
fn mechanical_compact(db: &Db, ctx: &ToolContext, mut messages: Vec<Message>) -> Vec<Message> {
    // 保留 [0]=system、[1]=首条负责人指令；切 [2..k) 移出。
    if messages.len() <= 3 {
        return messages;
    }
    let mut k = messages.len() / 2;
    // 切点不能落在 Tool 消息上：那会把它和（已移出的）tool_use 拆开，
    // 出站历史出现孤儿 tool_result，Anthropic 类供应商直接 400。
    while k < messages.len() && messages[k].role == Role::Tool {
        k += 1;
    }
    if k <= 2 || k >= messages.len() {
        return messages;
    }
    let removed: Vec<Message> = messages.drain(2..k).collect();
    let Some(transcript) = write_transcript(ctx, &removed) else {
        // transcript 落不了盘就不悄悄丢消息——原样返回走升级路径
        return messages;
    };
    let block = format!(
        "[机械降级] 最旧 {} 条消息已逐字移到 {transcript}，需要细节用 fs_read 读。\n\
         {}\n\
         负责人首条指令见上文首条消息（逐字保留）。\n\
         续写契约：不重复已答问题、不复盘已交付内容。",
        removed.len(),
        crate::provenance::state_block(db, ctx),
    );
    messages.insert(
        2,
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: block }],
        },
    );
    let _ = db.append_event(
        &ctx.project_id,
        EventKind::System,
        json!({"kind": "context_compacted", "removed": k - 2, "transcript": transcript}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    );
    messages
}

/// 逐字 transcript：消息与块原样落盘（tool_use/tool_result 含全部字段）。
fn write_transcript(ctx: &ToolContext, removed: &[Message]) -> Option<String> {
    let dir = ctx.repo_root.join(crate::tools::SPILL_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    let n = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("transcript-"))
        .count()
        + 1;
    let rel = format!("{}/transcript-{n}.md", crate::tools::SPILL_DIR);
    let mut s = String::new();
    for m in removed {
        s.push_str(&format!("## {:?}\n", m.role));
        for b in &m.content {
            match b {
                ContentBlock::Text { text } => {
                    s.push_str(text);
                    s.push('\n');
                }
                ContentBlock::ToolUse { id, name, input } => {
                    s.push_str(&format!("[tool_use {name} #{id}] {input}\n"));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    s.push_str(&format!(
                        "[tool_result #{tool_use_id} err={is_error}] {content}\n"
                    ));
                }
            }
        }
        s.push('\n');
    }
    std::fs::write(ctx.repo_root.join(&rel), s).ok()?;
    Some(rel)
}

/// 撞限升级（US37）：escalation 待决卡（sub=context_overflow）问负责人。
fn context_overflow(
    db: &Db,
    ctx: &ToolContext,
    est_tokens: usize,
    reason: &str,
) -> Result<TurnOutcome, TurnError> {
    let role: String = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE id=?1",
            [&ctx.agent_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| ctx.agent_id.clone());
    let qid = format!("q{}", db.next_id("q")?);
    db.conn().execute(
        "INSERT INTO pending_questions (id, project_id, agent_id, kind, payload)
         VALUES (?1,?2,?3,'escalation',?4)",
        rusqlite::params![
            qid,
            ctx.project_id,
            ctx.agent_id,
            json!({
                "sub": "context_overflow",
                "role": role,
                "est_tokens": est_tokens,
                "cap": CONTEXT_CAP_TOKENS,
                "reason": reason,
            })
            .to_string()
        ],
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::Escalated,
        json!({"reason": "context_overflow", "question_id": qid, "est_tokens": est_tokens, "cap": CONTEXT_CAP_TOKENS}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(TurnOutcome::AwaitingPermission(qid))
}
fn bump_streak(
    state: &mut Option<(String, String)>,
    streak: &mut usize,
    tool: &str,
    sig: &str,
) -> usize {
    if state.as_ref() == Some(&(tool.to_string(), sig.to_string())) {
        *streak += 1;
    } else {
        *state = Some((tool.to_string(), sig.to_string()));
        *streak = 1;
    }
    *streak
}

#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    /// EndTurn：文本已上时间线
    Finished,
    /// 有工具调用转必问，回合挂起（qid）
    AwaitingPermission(String),
    /// 失败（供应商错/限步/工具错）
    Failed(String),
    /// 截断终态（票 13）：续推一次后仍 max_tokens——与 Finished 区分，
    /// 让上层看得出「给了机会还是没说完」。
    Truncated,
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
    run_turn_impl(db, provider, registry, ctx, layers, user_input, false)
}

/// 方案先行回合（US15 快速通道）：工具循环前先发一段不阻塞方案消息，
/// 负责人有打断窗口；方案进执行上下文。
pub fn run_turn_planned(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
) -> Result<TurnOutcome, TurnError> {
    run_turn_impl(db, provider, registry, ctx, layers, user_input, true)
}

#[allow(clippy::too_many_arguments)]
fn run_turn_impl(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
    plan_first: bool,
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
    // US72：项目说明全文进激活首条消息（AgentsMd 层，优先级链位 2）。
    // 超 ~32KB 降级为头部+节标题目录+必读指引，并提醒负责人。
    let mut layers = layers;
    if let Some((text, degraded)) = load_instructions(&ctx.repo_root) {
        layers.push(PromptLayer::new(LayerLevel::AgentsMd, text));
        if degraded {
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                json!({"kind": "instructions_degraded",
                       "note": "项目说明超 32KB，已降级为头部+节标题目录注入"}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
        }
    }
    // 票 09：技能 catalog 一行式注入（AgentsMd 族——仓提供的说明类内容）。
    // 全文不进提示词，agent 按需 load_skill 取；会话 mute 生效于过滤。
    {
        let session = ctx
            .stage_run_id
            .clone()
            .unwrap_or_else(|| ctx.agent_id.clone());
        let muted = crate::skills::SkillMutes::load(&ctx.repo_root).muted_set(&session);
        let loader = crate::skills::SkillLoader::new(crate::skills::skill_dirs(&ctx.repo_root));
        if let Some(text) = loader.catalog_text(&muted) {
            layers.push(PromptLayer::new(LayerLevel::AgentsMd, text));
        }
    }
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
                        "paths": brief.paths,
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
        // US15 方案预告：剥掉工具要一段方案，落群聊（不阻塞），方案进执行上下文
        if plan_first {
            let plan_req = ChatRequest {
                messages: vec![
                    messages[0].clone(),
                    Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: json!({
                                "instruction": "用一段话给出执行方案（本轮不要调用工具），随后进入执行",
                                "task": user_input
                            })
                            .to_string(),
                        }],
                    },
                ],
                tools: vec![],
                ..req_base.clone()
            };
            let resp = complete_with_retry(db, ctx, provider, &plan_req)?;
            crate::usage::record(db, ctx, &req_base.model_slot, &resp.usage, 0)?;
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
            messages.push(Message {
                role: Role::Assistant,
                content: resp.content,
            });
        }
        // 同工具同错熔断状态：跨 round 连击计数（US57）
        let mut last_fail: Option<(String, String)> = None;
        let mut streak = 0usize;
        // 票 13：截断续推只给一次（OPE-171 连败防循环）
        let mut trunc_continued = false;
        let breaker = |db: &Db, tool: &str, sig: &str| -> Result<TurnOutcome, TurnError> {
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                json!({"kind": "tool_breaker", "tool": tool, "streak": BREAKER_STREAK, "err": sig}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            Ok(TurnOutcome::Failed(format!(
                "tool breaker: {tool} failed {BREAKER_STREAK}x consecutively: {sig}"
            )))
        };
        for round in 0..MAX_TOOL_ROUNDS {
            // 负责人在工具循环期间可暂停（US15）：每轮顶检，叫停即收回合
            if crate::orchestra::is_paused(db, &ctx.project_id)? {
                return Ok(TurnOutcome::Failed("paused by owner mid-turn".into()));
            }
            // US37：轻量裁剪始终做；仍超上限先走机械降级（票 06），
            // 降级后仍超才暂停问负责人（不做自动全量摘要）
            messages = trim_context(ctx, messages);
            let mut est = estimate_tokens(&messages);
            if est > CONTEXT_CAP_TOKENS {
                messages = mechanical_compact(db, ctx, messages);
                messages = trim_context(ctx, messages);
                est = estimate_tokens(&messages);
            }
            if est > CONTEXT_CAP_TOKENS {
                return context_overflow(db, ctx, est, "estimate");
            }
            log::debug!(
                "model call round={round} agent={} slot={}",
                ctx.agent_id,
                req_base.model_slot
            );
            // 票 13：发送前修复悬空 tool_use——截断/中断留下的未配对
            // tool_use 会让 Anthropic 形状 provider 拒收整段历史。
            repair_dangling_tool_uses(&mut messages);
            let resp = complete_with_retry(
                db,
                ctx,
                provider,
                &ChatRequest {
                    messages: with_dynamic_tail(&messages, round),
                    ..req_base.clone()
                },
            )?;
            log::debug!(
                "model resp: stop={:?} prompt_tok={} completion_tok={}",
                resp.stop,
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens
            );
            crate::usage::record(db, ctx, &req_base.model_slot, &resp.usage, 0)?;
            // 票 10：首个响应到手 = brief 送达模型——推进游标到组装水位。
            // 早了丢增量（下次激活漏读），晚了只是重送——这个点是正确侧。
            if round == 0 {
                db.advance_cursor(&ctx.agent_id, brief.watermark)?;
            }
            // 票 13（OPE-171）：stop=max_tokens 不再直接升级负责人——先续推
            // 一次（截断回复入史 + nudge），仍截断才以 truncated 收场。
            // 旧写法（票 06 前）把输出截断误判成上下文满，直接弹升级卡——
            // 后果是每次长输出都打断负责人。est>cap 的输入侧撞限仍在
            // 循环顶的机械降级+升级路径。
            if resp.stop == StopReason::MaxTokens {
                messages.push(Message {
                    role: Role::Assistant,
                    content: resp.content.clone(),
                });
                if !trunc_continued {
                    trunc_continued = true;
                    messages.push(Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: TRUNCATION_NUDGE.into(),
                        }],
                    });
                    continue;
                }
                return Ok(TurnOutcome::Truncated);
            }
            trunc_continued = false;
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

            // 执行工具调用，结果回喂；同工具同错连 BREAKER_STREAK 次熔断（US57）
            let mut results = Vec::new();
            // 票 11：位置序 r{round}i{idx} 作幂等序号——provider 的 tool_use id
            // 跨进程不复现，位置序在「同会话重放」中稳定，中断恢复命中既有卡。
            for (idx, (id, name, input)) in tool_uses.into_iter().enumerate() {
                let seq = format!("r{round}i{idx}");
                // US36 研究助手：research 由 turn 层截获跑嵌套只读回合
                // （provider/只读注册表都在这里才够得着，exec 层拿不到）
                let called = if name == "research" {
                    crate::research::call_nested(db, provider, registry, ctx, input)
                } else {
                    registry.call_with_seq(db, ctx, &name, input, Some(&seq))
                };
                match called {
                    Ok(CallOutcome::Done(v)) => {
                        last_fail = None;
                        streak = 0;
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: v.to_string(),
                            is_error: false,
                        })
                    }
                    Ok(CallOutcome::Denied(reason)) => {
                        let sig = format!("denied: {reason}");
                        if bump_streak(&mut last_fail, &mut streak, &name, &sig) >= BREAKER_STREAK {
                            return breaker(db, &name, &sig);
                        }
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: sig,
                            is_error: true,
                        })
                    }
                    Ok(CallOutcome::Asked(qid)) => {
                        // 票 04/05 审查者：shadow 记录判定；live+allow 放行。
                        // deny/unsure/未咨询 → 卡照出（理由已注进卡载荷）。
                        if let crate::reviewer::ReviewOutcome::Allow = crate::reviewer::adjudicate(
                            db,
                            ctx,
                            provider,
                            &req_base.model_slot,
                            &qid,
                            user_input,
                        ) {
                            // 复用必问卡裁决路径：标 answered + 落事件 + 执行。
                            // pack=None —— reviewer 放行永不沉淀形状记忆：
                            // 机器判定是一次性的，不是负责人同意（票 05）。
                            match registry.resolve(
                                db,
                                ctx,
                                &qid,
                                true,
                                None,
                                "activation",
                                None,
                                "reviewer",
                            ) {
                                Ok(CallOutcome::Done(result)) => {
                                    results.push(ContentBlock::ToolResult {
                                        tool_use_id: id,
                                        content: result.to_string(),
                                        is_error: false,
                                    });
                                    continue;
                                }
                                Ok(CallOutcome::Denied(r)) => {
                                    results.push(ContentBlock::ToolResult {
                                        tool_use_id: id,
                                        content: format!("denied: {r}"),
                                        is_error: true,
                                    });
                                    continue;
                                }
                                Ok(CallOutcome::Asked(_)) => unreachable!(),
                                Err(e) if model_visible(&e) => {
                                    results.push(ContentBlock::ToolResult {
                                        tool_use_id: id,
                                        content: format!("error: {e}"),
                                        is_error: true,
                                    });
                                    continue;
                                }
                                Err(e) => return Err(e.into()),
                            }
                        }
                        return Ok(TurnOutcome::AwaitingPermission(qid));
                    }
                    Err(e) if model_visible(&e) => {
                        // 模型可自救的错回喂，下轮换参/换法；同错连击熔断
                        let sig = e.to_string();
                        if bump_streak(&mut last_fail, &mut streak, &name, &sig) >= BREAKER_STREAK {
                            return breaker(db, &name, &sig);
                        }
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: format!("error: {sig}"),
                            is_error: true,
                        })
                    }
                    Err(e) => return Err(e.into()), // 基建错（trace/db/sqlite）上抛
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
            // 票 13：截断终态=没说完——归 TurnFailed，轨迹里与正常完成可区分
            Ok(TurnOutcome::Truncated) | Ok(TurnOutcome::Failed(_)) | Err(_) => {
                EventKind::TurnFailed
            }
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

    /// US35：工具循环有硬顶（MAX_TOOL_ROUNDS ≤ 规格上限 32）——超顶结束回合不空转。
    #[test]
    fn us35_tool_loop_capped() {
        let (db, reg, ctx, dir) = setup();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn main() {}").unwrap();
        // 脚本给 9 轮 tool_use（> 8 轮上限）：回合必须报错收场
        let provider = ScriptedProvider::new(
            (0..9)
                .map(|i| {
                    tool_response(vec![(
                        &format!("t{i}"),
                        "fs_read",
                        json!({"path":"src/lib.rs"}),
                    )])
                })
                .collect(),
        );
        match run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap() {
            TurnOutcome::Failed(e) => assert!(e.contains("tool-loop"), "got {e}"),
            o => panic!("expected Failed, got {o:?}"),
        }
        assert_eq!(provider.recorded().len(), 8);
    }

    /// US72：项目说明全文进激活首条消息；超 32KB 降级为目录+指引并提醒负责人。
    #[test]
    fn us72_agents_md_enters_first_message() {
        let (db, reg, ctx, dir) = setup();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# 项目约束\n永远先跑测试再提交。",
        )
        .unwrap();
        let provider = ScriptedProvider::new(vec![text_response("ok")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        let sys = match &provider.recorded()[0].messages[0].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!(),
        };
        assert!(sys.contains("agents.md"), "layer label missing: {sys}");
        assert!(sys.contains("永远先跑测试再提交"));
    }

    #[test]
    fn us72_oversized_instructions_degrade_and_remind() {
        let (db, reg, ctx, dir) = setup();
        let mut big = String::from("# 头部\n");
        for i in 0..900 {
            big.push_str(&format!("## 第{i}节\n{}\n", "x".repeat(64)));
        }
        assert!(big.len() > 32 * 1024);
        std::fs::write(dir.path().join("AGENTS.md"), &big).unwrap();
        let provider = ScriptedProvider::new(vec![text_response("ok")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        let sys = match &provider.recorded()[0].messages[0].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!(),
        };
        assert!(sys.contains("已降级"), "no degrade marker: {}", &sys[..200]);
        assert!(sys.contains("节标题目录"));
        assert!(sys.contains("## 第899节"), "outline truncated wrongly");
        assert!(!sys.contains(&"x".repeat(200)));
        // 负责人提醒事件已落
        let evs = db
            .timeline("p1", None, 50, Some(&[EventKind::System]))
            .unwrap();
        assert!(evs.iter().any(|i| i
            .event
            .payload
            .to_string()
            .contains("instructions_degraded")));
    }

    #[test]
    fn us72_no_instructions_no_empty_layer() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("ok")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        let sys = match &provider.recorded()[0].messages[0].content[0] {
            ContentBlock::Text { text } => text.clone(),
            _ => panic!(),
        };
        assert!(!sys.contains("agents.md"));
    }

    /// 票 02：撞限裁剪走 spill——超长 tool_result 全文落盘，上下文留
    /// head+marker+tail；重复裁剪同内容幂等（文件名取内容哈希）。
    #[test]
    fn spill_trim_keeps_tail_and_spills_full() {
        let (_db, _reg, ctx, dir) = setup();
        let big = format!("{}{}", "h".repeat(6000), "FATAL_TAIL");
        let out = trim_context(
            &ctx,
            vec![Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: big.clone(),
                    is_error: false,
                }],
            }],
        );
        let ContentBlock::ToolResult { content, .. } = &out[0].content[0] else {
            panic!()
        };
        assert!(content.contains("full output: .hexagon/spill/"));
        assert!(content.ends_with("FATAL_TAIL"));
        let rel = content
            .split("full output: ")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap()
            .trim();
        assert_eq!(std::fs::read_to_string(dir.path().join(rel)).unwrap(), big);
    }

    /// 票 06：撞限机械降级——最旧轮次逐字落 transcript，机械状态块入队，
    /// 切点不拆 tool_use/tool_result 对。
    #[test]
    fn mechanical_compact_spills_and_keeps_boundaries() {
        let (db, _reg, ctx, dir) = setup();
        let mut msgs = vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: "sys".into() }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "首条指令".into(),
                }],
            },
        ];
        for i in 0..8 {
            msgs.push(Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: format!("t{i}"),
                    name: "fs_read".into(),
                    input: json!({"path": format!("f{i}")}),
                }],
            });
            msgs.push(Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: format!("t{i}"),
                    content: "x".repeat(100),
                    is_error: false,
                }],
            });
        }
        msgs.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "latest".into(),
            }],
        });
        let out = mechanical_compact(&db, &ctx, msgs);
        // [0]sys [1]首条指令原样；[2] 是机械块 User
        assert_eq!(out[1].role, Role::User);
        assert_eq!(out[2].role, Role::User);
        let ContentBlock::Text { text } = &out[2].content[0] else {
            panic!()
        };
        assert!(text.contains("机械降级") && text.contains("transcript-"));
        assert!(text.contains("首条指令") || text.contains("续写契约"));
        // 保留段首不是孤儿 tool_result
        assert_ne!(out[3].role, Role::Tool);
        // transcript 落盘且逐字
        let t = std::fs::read_to_string(dir.path().join(".hexagon/spill/transcript-1.md")).unwrap();
        assert!(t.contains("tool_use fs_read"));
        assert!(t.contains("tool_result"));
    }

    /// 票 06：没东西可切（或切点跑穿）→ 原样返回，升级路径不变。
    #[test]
    fn mechanical_compact_noop_when_nothing_to_cut() {
        let (db, _reg, ctx, _dir) = setup();
        let msgs = vec![
            Message {
                role: Role::System,
                content: vec![ContentBlock::Text { text: "s".into() }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "u".into() }],
            },
        ];
        assert_eq!(mechanical_compact(&db, &ctx, msgs).len(), 2);
    }

    // ---- US57：瞬时重试与同错熔断 ----

    /// 可回放错误的假供应商：脚本条目是 Result，记录调用次数。
    struct FlakyProvider {
        script: std::sync::Mutex<
            std::collections::VecDeque<Result<ChatResponse, crate::provider::ProviderError>>,
        >,
        calls: std::sync::Mutex<usize>,
    }
    impl FlakyProvider {
        fn new(script: Vec<Result<ChatResponse, crate::provider::ProviderError>>) -> Self {
            Self {
                script: std::sync::Mutex::new(script.into()),
                calls: std::sync::Mutex::new(0),
            }
        }
        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }
    impl ModelProvider for FlakyProvider {
        fn complete(
            &self,
            _req: &ChatRequest,
        ) -> Result<ChatResponse, crate::provider::ProviderError> {
            *self.calls.lock().unwrap() += 1;
            self.script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(crate::provider::ProviderError::ScriptExhausted))
        }
    }

    fn system_events(db: &Db, marker: &str) -> usize {
        db.timeline("p1", None, 200, None)
            .unwrap()
            .iter()
            .filter(|i| {
                i.event.kind == EventKind::System && i.event.payload.to_string().contains(marker)
            })
            .count()
    }

    #[test]
    fn us57_transient_retry_recovers() {
        let (db, reg, ctx, _dir) = setup();
        let provider = FlakyProvider::new(vec![
            Err(crate::provider::ProviderError::Transport("timeout".into())),
            Err(crate::provider::ProviderError::Transport("reset".into())),
            Ok(text_response("好了")),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        assert_eq!(out, TurnOutcome::Finished);
        assert_eq!(provider.calls(), 3); // 1 初调 + 2 重试
        assert_eq!(system_events(&db, "provider_retry"), 2);
    }

    #[test]
    fn us57_retry_exhausted_fails_turn() {
        let (db, reg, ctx, _dir) = setup();
        // 4 次全 Transport：初调 + 3 次重试用尽仍败 → 回合失败
        let provider = FlakyProvider::new(vec![
            Err(crate::provider::ProviderError::Transport("t".into())),
            Err(crate::provider::ProviderError::Transport("t".into())),
            Err(crate::provider::ProviderError::Transport("t".into())),
            Err(crate::provider::ProviderError::Transport("t".into())),
            Ok(text_response("不该到")),
        ]);
        assert!(run_turn(&db, &provider, &reg, &ctx, vec![], "go").is_err());
        assert_eq!(provider.calls(), 4); // 最多 3 次重试
        assert_eq!(system_events(&db, "provider_retry"), 3);
    }

    #[test]
    fn us57_refused_never_retried() {
        let (db, reg, ctx, _dir) = setup();
        // Refused（4xx/权限拒绝类）：不重试，一次即败
        let provider = FlakyProvider::new(vec![
            Err(crate::provider::ProviderError::Refused("403".into())),
            Ok(text_response("不该到")),
        ]);
        assert!(run_turn(&db, &provider, &reg, &ctx, vec![], "go").is_err());
        assert_eq!(provider.calls(), 1);
        assert_eq!(system_events(&db, "provider_retry"), 0);
    }

    #[test]
    fn us57_same_tool_same_error_breaker() {
        let (db, reg, ctx, _dir) = setup();
        // 同一 fs_read 读不存在的文件连错：第 3 次熔断结束回合
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![("t1", "fs_read", json!({"path": "nope.md"}))]),
            tool_response(vec![("t2", "fs_read", json!({"path": "nope.md"}))]),
            tool_response(vec![("t3", "fs_read", json!({"path": "nope.md"}))]),
            text_response("不该到"),
        ]);
        match run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap() {
            TurnOutcome::Failed(e) => assert!(e.contains("tool breaker"), "got {e}"),
            o => panic!("expected breaker Failed, got {o:?}"),
        }
        assert_eq!(provider.recorded().len(), 3); // 熔断在第 3 次错，第 4 次模型调用不发生
        assert_eq!(system_events(&db, "tool_breaker"), 1);
    }

    #[test]
    fn us57_streak_resets_on_success() {
        let (db, reg, ctx, dir) = setup();
        std::fs::write(dir.path().join("ok.md"), "hi").unwrap();
        // 错、错、成、错、错、完：连击从未到 3，不熔断
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![("t1", "fs_read", json!({"path": "nope.md"}))]),
            tool_response(vec![("t2", "fs_read", json!({"path": "nope.md"}))]),
            tool_response(vec![("t3", "fs_read", json!({"path": "ok.md"}))]),
            tool_response(vec![("t4", "fs_read", json!({"path": "nope.md"}))]),
            tool_response(vec![("t5", "fs_read", json!({"path": "nope.md"}))]),
            text_response("done"),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        assert_eq!(out, TurnOutcome::Finished);
        assert_eq!(system_events(&db, "tool_breaker"), 0);
    }

    // ---- US37：上下文撞停问负责人 ----

    /// 估算 prompt 仍超上限 → 升级卡问负责人，不做自动全量摘要，模型零调用。
    #[test]
    fn us37_context_overflow_escalates() {
        let (db, reg, ctx, _dir) = setup();
        // 600KB 系统层 ≈ 150k tok > 120k 上限——轻量裁剪救不回系统层
        let big = PromptLayer::new(LayerLevel::Brief, "x".repeat(600_000));
        let provider = ScriptedProvider::new(vec![text_response("不该被调用")]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![big], "go").unwrap();
        match out {
            TurnOutcome::AwaitingPermission(qid) => {
                let (kind, payload): (String, String) = db
                    .conn()
                    .query_row(
                        "SELECT kind, payload FROM pending_questions WHERE id=?1",
                        [&qid],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(kind, "escalation");
                assert!(payload.contains("context_overflow"), "payload {payload}");
            }
            o => panic!("expected AwaitingPermission, got {o:?}"),
        }
        assert!(provider.recorded().is_empty(), "撞限在模型调用前");
    }

    /// 票 13：stop=max_tokens 续推一次——后续正常响应则回合完成。
    #[test]
    fn t13_max_tokens_continuation_finishes() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![
            ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "半句话".into(),
                }],
                stop: StopReason::MaxTokens,
                usage: Default::default(),
            },
            text_response("说完的后半句"),
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        assert!(
            matches!(out, TurnOutcome::Finished),
            "续推成功应 Finished，got {out:?}"
        );
        let calls = provider.recorded();
        assert_eq!(calls.len(), 2, "截断+续推=两次调用");
        // 续推注入 nudge 用户消息而非重发原题（末条是票 14 动态块，
        // nudge 在倒数第二）
        let msgs = &calls[1].messages;
        let nudge = msgs.get(msgs.len().saturating_sub(2)).expect("nudge msg");
        assert!(
            matches!(&nudge.content[0], ContentBlock::Text { text } if text.contains("截断")),
            "续推应带 nudge"
        );
    }

    /// 票 13：续推后仍截断 → truncated 终态（与 Finished 区分），零升级卡。
    #[test]
    fn t13_still_truncated_terminal() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![
            ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "半句".into(),
                }],
                stop: StopReason::MaxTokens,
                usage: Default::default(),
            },
            ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "又是半句".into(),
                }],
                stop: StopReason::MaxTokens,
                usage: Default::default(),
            },
        ]);
        let out = run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        assert!(
            matches!(out, TurnOutcome::Truncated),
            "仍截断应 Truncated，got {out:?}"
        );
    }

    /// 票 14：易变内容（时间/轮次）独立成末尾块；系统提示+首条指令
    /// 跨回合逐字节一致——provider prompt cache 命中的前提。
    #[test]
    fn t14_dynamic_tail_keeps_prefix_stable() {
        let (db, reg, ctx, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &provider, &reg, &ctx, vec![], "go").unwrap();
        let provider2 = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &provider2, &reg, &ctx, vec![], "go").unwrap();
        let c1 = provider.recorded();
        let c2 = provider2.recorded();
        let tail = c1[0].messages.last().expect("dynamic tail");
        assert!(
            matches!(&tail.content[0], ContentBlock::Text { text } if text.contains("\"env\"")),
            "末条应是动态块"
        );
        // 前缀逐字节稳定：系统提示与首条指令跨回合相同
        // （Message 无 PartialEq，序列化后比对）
        assert_eq!(
            serde_json::to_value(&c1[0].messages[0]).unwrap(),
            serde_json::to_value(&c2[0].messages[0]).unwrap(),
            "system 前缀漂移"
        );
        assert_eq!(
            serde_json::to_value(&c1[0].messages[1]).unwrap(),
            serde_json::to_value(&c2[0].messages[1]).unwrap(),
            "首条指令漂移"
        );
    }

    /// 票 13：悬空 tool_use 补 error stub——provider 形状配对恢复。
    #[test]
    fn t13_dangling_tool_use_repaired() {
        let mut messages = vec![
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "fs_read".into(),
                        input: serde_json::json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "t2".into(),
                        name: "fs_read".into(),
                        input: serde_json::json!({}),
                    },
                ],
            },
            Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "ok".into(),
                    is_error: false,
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "t3".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                }],
            },
        ];
        let n = repair_dangling_tool_uses(&mut messages);
        assert_eq!(n, 2, "t2、t3 悬空");
        // 每个 ToolUse 后都有配对 ToolResult
        let uses: Vec<&str> = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let results: Vec<&str> = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect();
        for u in &uses {
            assert!(results.contains(u), "{u} 无配对结果");
        }
        // stub 如实标注中断
        assert!(messages
            .iter()
            .flat_map(|m| m.content.iter())
            .any(|b| matches!(
                b,
                ContentBlock::ToolResult { content, is_error: true, .. }
                    if content.contains("interrupted")
            )));
        // 幂等：再修一遍无新增
        assert_eq!(repair_dangling_tool_uses(&mut messages), 0);
    }

    /// 轻量裁剪：超长 tool_result 截断带标记；小消息不动。
    #[test]
    fn us37_trim_truncates_tool_results() {
        let (_db, _reg, ctx, _dir) = setup();
        let msgs = vec![
            Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "y".repeat(9000),
                    is_error: false,
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "short".into(),
                }],
            },
        ];
        let out = trim_context(&ctx, msgs);
        match &out[0].content[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert!(content.len() < 5000, "still {} chars", content.len());
                assert!(content.contains("trimmed"), "marker missing: {content}");
            }
            _ => panic!(),
        }
        match &out[1].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "short"),
            _ => panic!(),
        }
    }

    // ---- 票 10：事件游标 + taint ----

    fn owner_mention(db: &Db, body: &str) {
        db.append_message(
            "p1",
            "owner",
            body,
            &[crate::trace::MessageToken::Mention {
                agent_role: "后端开发".into(),
            }],
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn cursor_scopes_brief_to_incremental_events() {
        let (db, _reg, _ctx, _d) = setup();
        owner_mention(&db, "第一条点名");
        let b1 = build_brief_context(&db, "a1", None).unwrap();
        assert_eq!(b1.mentions, vec!["第一条点名"]);
        assert!(b1.watermark > 0);
        // 游标推进后再来消息 → 第二次组装只见增量
        db.advance_cursor("a1", b1.watermark).unwrap();
        owner_mention(&db, "第二条点名");
        let b2 = build_brief_context(&db, "a1", None).unwrap();
        assert_eq!(b2.mentions, vec!["第二条点名"], "不重读游标前的消息");
        // 不推进 → 再读仍是同一条增量（休眠唤醒不漏不重）
        let b3 = build_brief_context(&db, "a1", None).unwrap();
        assert_eq!(b3.mentions, vec!["第二条点名"]);
    }

    #[test]
    fn cursor_advances_only_after_first_response() {
        let (db, _reg, ctx, _d) = setup();
        owner_mention(&db, "点1");
        // provider 第一轮就失败：游标不该推进
        let failing = ScriptedProvider::new(vec![]);
        let _ = run_turn(&db, &failing, &_reg, &ctx, vec![], "干活");
        assert_eq!(db.cursor("a1"), 0, "brief 没送达模型前游标不动");
        // 成功的回合推进到组装水位
        let ok = ScriptedProvider::new(vec![text_response("done")]);
        run_turn(&db, &ok, &_reg, &ctx, vec![], "干活").unwrap();
        assert!(db.cursor("a1") > 0, "首个响应到手后游标推进");
    }

    #[test]
    fn external_results_taint_subsequent_outputs() {
        let (db, _reg, _ctx, _d) = setup();
        db.conn()
            .execute(
                "INSERT INTO stage_runs (id, project_id, stage_name, seq, state)
                 VALUES ('sr1','p1','实现',0,'active')",
                [],
            )
            .unwrap();
        // 外部内容回喂前：消息不带标
        db.append_message("p1", "a1", "之前的话", &[], Some("a1"), Some("sr1"))
            .unwrap();
        // mcp 结果回喂（tool_result 事件）
        db.append_event(
            "p1",
            EventKind::ToolResult,
            json!({"tool":"mcp:gh:list","output":{}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        db.append_message("p1", "a1", "读完外部后的判断", &[], Some("a1"), Some("sr1"))
            .unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::AgentMessage]))
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].event.payload.get("after_external").is_none());
        assert_eq!(items[1].event.payload["after_external"], true);
        // 负责人消息永不打标
        db.append_message("p1", "owner", "负责人说的", &[], None, Some("sr1"))
            .unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::OwnerMessage]))
            .unwrap();
        assert!(items[0].event.payload.get("after_external").is_none());
    }
}
