//! 回合内核：tool-loop + 流式分发 + 终态映射。
//!
//! 子模块（arch-review 票 03 拆分）：`prompt` = 提示词装配/简报/请求信封，
//! `context` = 上下文估算/裁剪/只删工具记录/升级卡/消息卫生。
//!
//! 休眠语义：status='sleeping' 的 Agent 不调度、不召模型——run_turn 直接
//! 返回，provider 零调用。

use crate::db::Db;
use crate::provider::{
    ChatRequest, ChatResponse, ContentBlock, Message, ModelProvider, Role, StopReason,
};
use crate::tools::{CallOutcome, Registry, ToolContext, ToolError};
use crate::trace::{EventKind, TraceError};
use serde_json::{json, Value};

pub mod context;
pub mod prompt;

// 兼容再导出（arch 票 03 纯位移）：原 crate::turn::* 平铺路径上的 pub 项
// 保持可用，下游不必跟着子模块路径走。
pub use context::repair_dangling_tool_uses;
pub(crate) use prompt::request_envelope;
pub use prompt::{
    build_brief_context, build_system_prompt, load_instructions, BriefContext, LayerLevel,
    PromptLayer,
};

use context::{
    context_overflow, estimate_tokens, mechanical_compact, model_visible, trim_context,
    CONTEXT_CAP_TOKENS,
};
use prompt::{layer_meta, with_dynamic_tail};

const MAX_TOOL_ROUNDS: usize = 8;
/// 瞬时重试（US57）：Transport 类抖动按 100/250/500ms 指数退避，最多 3 次；
/// Refused（4xx/权限拒绝）、MissingCredential、ScriptExhausted 不重试直接上报。
const RETRY_DELAYS_MS: [u64; 3] = [100, 250, 500];
/// 同工具同错熔断（US57）：连续 N 次相同失败结束回合等负责人。
const BREAKER_STREAK: usize = 3;

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
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
}

/// 截断续推指令（票 13）：不重复推理，直接下一步。
const TRUNCATION_NUDGE: &str = "输出被截断。不要重复推理，直接给下一步。";

// ---------- 回合执行 ----------

/// 回合级 delta（turn-streaming 票 03）：带归属的瞬时增量，壳层据此
/// emit 到 webview，UI 按 agent 挂流式气泡。**不落库、不回放**——
/// 持久层仍只收最终拼装文本（spec「持久层只存成品」）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TurnDelta {
    pub agent_id: String,
    pub stage_run_id: Option<String>,
    /// 本回合第几次模型调用（plan_first 的方案调用=0，工具循环轮 1..）。
    /// UI 用 (agent_id, call) 分段：新一轮调用另起一段。
    #[ts(type = "number")] // JS number 域（wire 是 JSON number）
    pub call: usize,
    /// 瞬时重试复位：true 时本次调用已发文本作废重起——否则重试
    /// 重吐全文会在气泡里叠成双份（US57 重试与流式共存的补丁）。
    pub reset: bool,
    /// 流终信号：回合收口时发一次（无论成败/叫停），UI 收气泡，
    /// 不必猜是哪个命令触发的回合。
    pub done: bool,
    pub text: String,
    /// 本帧的思考增量（hands-free 票 06）。空串 = 这一帧没有推理文本，
    /// UI 不因此画思考行。与 text 分列，不把推理拼进可见回复。
    pub thinking: String,
}

/// delta 回调接缝。不绑 Send——turn 内核同线程调用 sink；
/// 跨线程要求（壳层 Tauri emit）由 Workbench 的 hook 存储类型承担。
pub type DeltaSink<'a> = dyn FnMut(&TurnDelta) + 'a;

/// 可重试的供应商错：只有 Transport 类瞬时抖动；4xx/拒绝/缺凭据/脚本耗尽直传。
fn retryable(e: &crate::provider::ProviderError) -> bool {
    matches!(e, crate::provider::ProviderError::Transport(_))
}

/// 模型调用（流式）+ 瞬时重试（US57）。重试全程落 System 事件留痕；
/// 重试前已发 delta 时先补一发 reset，让 UI 丢弃半截文本重起。
fn stream_with_retry(
    db: &Db,
    ctx: &ToolContext,
    provider: &dyn ModelProvider,
    req: &ChatRequest,
    call: usize,
    sink: &mut DeltaSink<'_>,
) -> Result<ChatResponse, TurnError> {
    let mut attempt = 0usize;
    loop {
        let mut emitted = false;
        let r = provider.stream(req, &mut |d| {
            // 票 04：delta 间隙叫停检查——流循环阻塞在读上时，这里是
            // 唯一能让负责人暂停生效的缝。is_paused 读库失败按未暂停
            // 处理：暂停是尽力而为的中途检查，轮顶检查仍是权威闸。
            if crate::orchestra::is_paused(db, &ctx.project_id).unwrap_or(false) {
                return false;
            }
            let (text, thinking) = match d {
                crate::provider::StreamDelta::Text(t) => (t.clone(), String::new()),
                crate::provider::StreamDelta::Thinking(t) => (String::new(), t.clone()),
            };
            emitted = true;
            sink(&TurnDelta {
                agent_id: ctx.agent_id.clone(),
                stage_run_id: ctx.stage_run_id.clone(),
                call,
                reset: false,
                done: false,
                text,
                thinking,
            });
            true
        });
        match r {
            Ok(resp) => return Ok(resp),
            Err(e) if retryable(&e) && attempt < RETRY_DELAYS_MS.len() => {
                attempt += 1;
                log::warn!(
                    "provider transient error, retry {attempt}/{}: {e}",
                    RETRY_DELAYS_MS.len()
                );
                if emitted {
                    sink(&TurnDelta {
                        agent_id: ctx.agent_id.clone(),
                        stage_run_id: ctx.stage_run_id.clone(),
                        call,
                        reset: true,
                        done: false,
                        text: String::new(),
                        thinking: String::new(),
                    });
                }
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

/// 工具执行值 → ToolResult 块（票 02）：`v["image"]`（fs_read 读图产出
/// `{media_type, data}`）剥成 images 载荷；content 里图片占位符替数据，
/// 字节本体不进文本。Anthropic 形状进 tool_result.content；OpenAI
/// 由映射层补 user 消息（ADR 0058-2）。
fn tool_result_block(tool_use_id: String, v: &Value) -> ContentBlock {
    let mut images = Vec::new();
    let mut content = v.clone();
    if let Some(im) = v["image"].as_object() {
        let mt = im["media_type"].as_str().unwrap_or("").to_string();
        let data = im["data"].as_str().unwrap_or("").to_string();
        if !mt.is_empty() && !data.is_empty() {
            images.push(crate::provider::ImageData {
                media_type: mt.clone(),
                data,
            });
            content["image"] =
                serde_json::json!({"media_type": mt, "omitted": "carried as image block"});
        }
    }
    ContentBlock::ToolResult {
        tool_use_id,
        content: content.to_string(),
        is_error: false,
        images,
    }
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
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
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
    /// 负责人叫停（turn-streaming 票 04）：主动干预不是失败——
    /// 取代旧的 Failed("paused by owner mid-turn")，轨迹上可与
    /// 真失败区分（OPE-171 教训：终态语义诚实，别拿 Failed 凑数）。
    Interrupted,
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
    run_turn_impl(
        db,
        provider,
        registry,
        ctx,
        layers,
        user_input,
        &[],
        false,
        None,
    )
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
    run_turn_impl(
        db,
        provider,
        registry,
        ctx,
        layers,
        user_input,
        &[],
        true,
        None,
    )
}

/// 带 delta 通道的回合（票 03）：sink 收带归属的瞬时增量；传 None
/// 与 run_turn/run_turn_planned 等价。壳层 hook 从这接 Tauri emit。
#[allow(clippy::too_many_arguments)]
pub fn run_turn_streaming(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
    attachments: &[crate::trace::AttachRef],
    plan_first: bool,
    sink: Option<&mut DeltaSink<'_>>,
) -> Result<TurnOutcome, TurnError> {
    run_turn_impl(
        db,
        provider,
        registry,
        ctx,
        layers,
        user_input,
        attachments,
        plan_first,
        sink,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_turn_impl(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
    attachments: &[crate::trace::AttachRef],
    plan_first: bool,
    sink: Option<&mut DeltaSink<'_>>,
) -> Result<TurnOutcome, TurnError> {
    let mut noop = |_d: &TurnDelta| {};
    let sink: &mut DeltaSink = match sink {
        Some(s) => s,
        None => &mut noop,
    };
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
        // ADR 0057：mute 判定 = 会话集 ∪ 遗留 "*" ∪ 全局文件
        // （~/.hexagon/skill-mutes.json）；*" 行是旧版全局开关的落点，
        // 保留读以不丢存量开关，新写一律进全局文件。
        let muted = crate::skills::effective_muted(&ctx.repo_root, &session);
        let loader = crate::skills::SkillLoader::new(crate::skills::skill_dirs(&ctx.repo_root));
        if let Some(text) = loader.catalog_text(&muted) {
            layers.push(PromptLayer::new(LayerLevel::AgentsMd, text));
        }
    }
    // 票 03：layer 指纹素材先预取——build_system_prompt 会消费 layers。
    let env_layers = layer_meta(&layers);
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

    // 票 03：负责人附件注入。vision 槽 → Image 块进首条 user 消息；
    // 非 vision 槽 → [image: name](path) 降级文本 + attachments_degraded
    // 事件（时间线提示行）。字节嗅探复核——行内 media_type 不可信。
    if !attachments.is_empty() {
        let vision = ctx.caps.contains("vision");
        let mut degraded = 0usize;
        for r in attachments.iter().take(crate::tools::ATTACH_MAX_COUNT) {
            let p = ctx.repo_root.join(&r.path);
            let blk = if r.path.starts_with(".hexagon/inbox/") {
                std::fs::read(&p).ok().and_then(|b| {
                    (b.len() <= crate::tools::ATTACH_IMG_CAP
                        && crate::tools::sniff_image(&b) == Some(r.media_type.as_str()))
                    .then(|| {
                        if vision {
                            use base64::Engine;
                            ContentBlock::Image {
                                media_type: r.media_type.clone(),
                                data: base64::engine::general_purpose::STANDARD.encode(&b),
                            }
                        } else {
                            degraded += 1;
                            ContentBlock::Text {
                                text: format!("[image: {}]({})", r.name, r.path),
                            }
                        }
                    })
                })
            } else {
                None
            };
            if let Some(blk) = blk {
                if let Some(m) = messages.get_mut(1) {
                    m.content.push(blk);
                }
            }
        }
        if degraded > 0 {
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                json!({"kind": "attachments_degraded", "count": degraded}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
        }
    }

    let req_base = ChatRequest {
        model_slot: model_slot.unwrap_or_else(|| "default".into()),
        messages: vec![],
        tools: registry.defs(),
    };

    let outcome = (|| -> Result<TurnOutcome, TurnError> {
        // 票 05 steering 水位：回合起跑线之后新来的 owner 消息在
        // 每轮顶注入出站副本。只追加进本轮出站 messages——
        // owner_message 由 send_message 落库；注入模型可见 ≠ 改持久历史
        // （archive _outbound_messages 同款分层：模型视图与持久层分离）。
        // 票 06：工具轮的思考先攒着，跟下一条可见回复一起落，不另起一条消息。
        let mut carried_thinking = String::new();
        let mut steer_mark: i64 = db.conn().query_row(
            "SELECT COALESCE(MAX(id),0) FROM messages WHERE project_id=?1 AND author='owner'",
            [&ctx.project_id],
            |r| r.get(0),
        )?;
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
            // 票 03 信封：方案派发也落指纹（call=0）。
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                request_envelope(0, &plan_req, &plan_req.messages, &env_layers),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            // 票 03：方案消息也走流式——call=0，工具循环从 1 起。
            let resp = match stream_with_retry(db, ctx, provider, &plan_req, 0, sink) {
                Ok(r) => r,
                // 票 04：流中被叫停 → Interrupted 终态（不上抛成错误）
                Err(TurnError::Provider(crate::provider::ProviderError::Interrupted)) => {
                    return Ok(TurnOutcome::Interrupted);
                }
                Err(e) => return Err(e),
            };
            crate::usage::record(db, ctx, &req_base.model_slot, &resp.usage, 0)?;
            absorb_thinking(&resp.content, &mut carried_thinking);
            let text = visible_text(&resp.content);
            persist_visible(db, ctx, &text, &mut carried_thinking)?;
            push_assistant(&mut messages, resp.content);
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
            // 负责人在工具循环期间可暂停（US15）：每轮顶检，叫停即收回合。
            // 票 04：Interrupted 终态取代 Failed——叫停不是失败。
            if crate::orchestra::is_paused(db, &ctx.project_id)? {
                return Ok(TurnOutcome::Interrupted);
            }
            // 票 05 steering 排水：把水位后新来的 owner 消息按 id 序注入
            // 出站上下文（结构化信封与首条 instruction 同款）。留痕可审计。
            {
                let mut st = db.conn().prepare(
                    "SELECT id, body FROM messages
                     WHERE project_id=?1 AND author='owner' AND id>?2 ORDER BY id",
                )?;
                let fresh: Vec<(i64, String)> = st
                    .query_map(rusqlite::params![ctx.project_id, steer_mark], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?
                    .collect::<Result<_, _>>()?;
                for (mid, body) in fresh {
                    steer_mark = mid;
                    // 票 05：文本指令（"/pause"、"退回" 等）是控制面不是内容——
                    // 水位照进（不重读）但不进模型上下文，不然 "/pause" 会被
                    // 当成业务插话喂给模型。
                    if crate::commands::parse_command(&body).is_some() {
                        continue;
                    }
                    messages.push(Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: json!({"steering": body}).to_string(),
                        }],
                    });
                    // 载荷用 msg_id 不用 message_id：timeline 的 JOIN 认
                    // message_id 会把本事件渲染成 owner 消息重影（票 05）。
                    db.append_event(
                        &ctx.project_id,
                        EventKind::System,
                        json!({"kind": "steering_injected", "msg_id": mid, "round": round}),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                }
            }
            // US37 + 票 07：轻量裁剪始终做；仍超上限只删工具记录
            // （负责人与角色原文不换成摘要）。删完仍超才暂停问负责人。
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
            // 票 03 信封：对「实际发送物」（裁剪/修复/动态尾之后）取指纹——
            // 信封的语义是「这次往线上发了什么」,不是「想发什么」。
            let req = ChatRequest {
                messages: with_dynamic_tail(&messages, round),
                ..req_base.clone()
            };
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                request_envelope(
                    round + usize::from(plan_first),
                    &req,
                    &messages,
                    &env_layers,
                ),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            let resp = match stream_with_retry(
                db,
                ctx,
                provider,
                &req,
                // 票 03：call 序号——plan_first 的方案占用 0，循环轮顺延。
                round + usize::from(plan_first),
                sink,
            ) {
                Ok(r) => r,
                // 票 04：流中被叫停 → Interrupted 终态（不上抛成错误）
                Err(TurnError::Provider(crate::provider::ProviderError::Interrupted)) => {
                    return Ok(TurnOutcome::Interrupted);
                }
                Err(e) => return Err(e),
            };
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
            // 循环顶的工具记录删减+升级路径。
            absorb_thinking(&resp.content, &mut carried_thinking);
            if resp.stop == StopReason::MaxTokens {
                push_assistant(&mut messages, resp.content.clone());
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
            push_assistant(&mut messages, resp.content.clone());

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
                // 回合结束：可见回复上时间线；思考随这条留下，没有就不写。
                let text = visible_text(&resp.content);
                persist_visible(db, ctx, &text, &mut carried_thinking)?;
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
                        results.push(tool_result_block(id, &v))
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
                            images: vec![],
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
                                    results.push(tool_result_block(id, &result));
                                    continue;
                                }
                                Ok(CallOutcome::Denied(r)) => {
                                    results.push(ContentBlock::ToolResult {
                                        tool_use_id: id,
                                        content: format!("denied: {r}"),
                                        is_error: true,
                                        images: vec![],
                                    });
                                    continue;
                                }
                                Ok(CallOutcome::Asked(_)) => unreachable!(),
                                Err(e) if model_visible(&e) => {
                                    results.push(ContentBlock::ToolResult {
                                        tool_use_id: id,
                                        content: format!("error: {e}"),
                                        is_error: true,
                                        images: vec![],
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
                            images: vec![],
                        })
                    }
                    Err(e) => return Err(e.into()), // 基建错（trace/db/sqlite）上抛
                }
            }
            let tool_bytes: usize = results
                .iter()
                .map(|b| match b {
                    ContentBlock::ToolResult {
                        content, images, ..
                    } => content.len() + images.iter().map(|i| i.data.len()).sum::<usize>(),
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

    // 票 03：流终信号——无论成败都发一次，UI 据此收气泡。
    sink(&TurnDelta {
        agent_id: ctx.agent_id.clone(),
        stage_run_id: ctx.stage_run_id.clone(),
        call: usize::MAX,
        reset: false,
        done: true,
        text: String::new(),
        thinking: String::new(),
    });

    db.append_event(
        &ctx.project_id,
        match &outcome {
            Ok(TurnOutcome::Finished) => EventKind::TurnFinished,
            Ok(TurnOutcome::AwaitingPermission(_)) => EventKind::TurnFinished,
            Ok(TurnOutcome::SkippedSleeping) => EventKind::TurnFinished,
            Ok(TurnOutcome::SkippedCap) => EventKind::TurnFinished,
            // 票 13：截断终态=没说完——归 TurnFailed，轨迹里与正常完成可区分。
            // 票 04：Interrupted 也归 TurnFailed（收束语义=回合没正常完），
            // 但 outcome 载荷带 Interrupted 字样可区分主动叫停与真失败。
            Ok(TurnOutcome::Interrupted)
            | Ok(TurnOutcome::Truncated)
            | Ok(TurnOutcome::Failed(_))
            | Err(_) => EventKind::TurnFailed,
        },
        // 票 02 闭集 code：只在失败终态上发——成功回合没有「失败理由」，
        // 无条件盖 code 会把每个正常回合计进回放报告的失败分布（review
        // 发现过：turn_finished 带 hard-blocked → judge 误判退步）。
        // 映射按枚举注释语义：截断=容量触顶 budget-exceeded（不是可修）;
        // 叫停=人已经裁决过的 ambiguous;模型/基建意外失败=ambiguous
        // （hard-blocked 留给结构性死路,由升级路径携带）。
        {
            let mut p = json!({ "agent": ctx.agent_id, "outcome": format!("{outcome:?}") });
            let code = match &outcome {
                Ok(TurnOutcome::Truncated) => Some(crate::trace::FailureCode::BudgetExceeded),
                Ok(TurnOutcome::Interrupted) | Ok(TurnOutcome::Failed(_)) | Err(_) => {
                    Some(crate::trace::FailureCode::Ambiguous)
                }
                _ => None,
            };
            if let Some(c) = code {
                p["code"] = json!(c.as_str());
            }
            p
        },
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    outcome
}

/// 把本轮推理并进待落账缓冲。空串不算「给了思考」。
fn absorb_thinking(content: &[ContentBlock], carried: &mut String) {
    for b in content {
        if let ContentBlock::Thinking { text } = b {
            if text.is_empty() {
                continue;
            }
            if !carried.is_empty() {
                carried.push('\n');
            }
            carried.push_str(text);
        }
    }
}

fn visible_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 思考不进下一轮出站历史（无 signature，见 ContentBlock::Thinking）。
fn push_assistant(messages: &mut Vec<Message>, content: Vec<ContentBlock>) {
    let content: Vec<_> = content
        .into_iter()
        .filter(|b| !matches!(b, ContentBlock::Thinking { .. }))
        .collect();
    if content.is_empty() {
        return;
    }
    messages.push(Message {
        role: Role::Assistant,
        content,
    });
}

/// 可见回复或思考至少有一边才落一条。思考不单独变成多条消息。
fn persist_visible(
    db: &Db,
    ctx: &ToolContext,
    text: &str,
    carried: &mut String,
) -> Result<(), TurnError> {
    if text.is_empty() && carried.is_empty() {
        return Ok(());
    }
    let thinking = if carried.is_empty() {
        None
    } else {
        Some(std::mem::take(carried))
    };
    db.append_message_with(
        &ctx.project_id,
        &ctx.agent_id,
        text,
        &[],
        &[],
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        thinking.as_deref(),
    )?;
    Ok(())
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
mod tests;
