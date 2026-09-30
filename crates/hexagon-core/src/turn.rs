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

use context::{context_overflow, estimate_tokens, mechanical_compact, model_visible, trim_context};
use prompt::{layer_meta, with_dynamic_tail};

/// 工具循环轮数上限（prompt-engineering 票 04）：8 → 32，即规格上限。
/// 8 轮在「读、搜、读、改、测、再修」的普通编码任务上就会撞顶收 Failed。
/// ADR 0052 当年担心长回合锁死 UI，读/控/回合三组命令拆分后已不成立；
/// 用量硬上限与同错熔断两道闸仍在。模型经动态尾部块看到本预算。
pub(crate) const MAX_TOOL_ROUNDS: usize = 32;
/// 截断续推上限（票 04）：1 → 3，对齐 Claude Code 的
/// MAX_OUTPUT_TOKENS_RECOVERY_LIMIT。不调 max_tokens——各模型输出上限
/// 不同，model_meta 里没有可靠数据。
const MAX_TRUNC_CONTINUES: usize = 3;
/// 方案预告（US15）的指令：本轮不给工具，只要一段方案。
pub(crate) const PLAN_FIRST_INSTRUCTION: &str =
    "In one paragraph, state your plan for this task. Do not call tools in this reply; execution follows.";
/// 任务清单提醒间隔（票 05）：有未关闭条目、连续这么多轮没碰 `tasks`
/// 就提醒一次。出处：Claude Code 的 todo_reminder。
const TASK_REMINDER_ROUNDS: usize = 5;
/// 被拒绝的工具结果后缀（票 04）：旧写法只回 `denied: …`，模型往往原样
/// 重试，要连错三次才被熔断收场。
pub(crate) const DENIED_GUIDANCE: &str = "Do not repeat this call unchanged. Work out why it was denied and take another approach, or say in your reply what you need from the owner.";
/// 瞬时重试（US57）：Transport 类抖动按 100/250/500ms 指数退避，最多 3 次；
/// Refused（4xx/权限拒绝）、MissingCredential、ScriptExhausted 不重试直接上报。
const RETRY_DELAYS_MS: [u64; 3] = [100, 250, 500];
/// 同工具同错熔断（US57）：连续 N 次相同失败结束回合等负责人。
const BREAKER_STREAK: usize = 3;

/// 断网等网策略（network-resilience 票 01）：快重试耗尽后的第二段。
/// Copy + Default——ctx 每处克隆零成本；测试注入毫秒级参数。
#[derive(Debug, Clone, Copy)]
pub struct WaitPolicy {
    /// 探针间隔：等网中每隔这么久原样重发同一调用（生产 5s）。
    pub probe_interval: std::time::Duration,
    /// 等网总预算：累计超限即挂起（生产 5min）。
    pub budget: std::time::Duration,
    /// 睡眠切片粒度：等网不是一觉睡死——每片末尾查暂停旗/停旗，
    /// 负责人叫停的响应延迟被 tick 限定。
    pub tick: std::time::Duration,
}

impl Default for WaitPolicy {
    /// 生产常量（票 01 规格值：5s 探针 / 5min 预算 / 100ms 暂停响应粒度）。
    fn default() -> Self {
        Self {
            probe_interval: std::time::Duration::from_secs(5),
            budget: std::time::Duration::from_secs(300),
            tick: std::time::Duration::from_millis(100),
        }
    }
}

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
    /// 断网等网超预算挂起（network-resilience 票 01）：哨兵错误——
    /// run 已在 suspend_run 里收口，这里只负责把终态带回调用方。
    #[error("suspended: network wait budget exhausted")]
    Suspended,
}

/// 截断续推指令（票 13）：不重复推理，直接下一步。
pub(crate) const TRUNCATION_NUDGE: &str = "Output limit hit. Continue exactly where you stopped — no apology, no recap. Break the remaining work into smaller pieces.";
/// 票 NR-03：恢复重触发的固定提示。中断回合的原指令靠未推进的简报
/// 游标自然重读，这句只提醒模型先看盘上已有状态再动手——重触发是
/// 新回合，模型必须识别已有产物/进展而不是从头重做。
pub const RECOVERY_NUDGE: &str =
    "The run you are responsible for was interrupted and has now resumed. Continue the pending instruction from your brief: first check the artifacts and state already on disk, then carry on from there — do not start over.";

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
    /// 断网等网旗（network-resilience 票 02）：true 帧 = 进入等网
    /// （气泡转「重连中」但保留已收文本）；false 帧清旗——正常增量
    /// 与 done 帧隐式清，显式 false 帧只在「探针成功后没立刻出文本」
    /// 的窗口里兜底。
    pub waiting: bool,
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
    matches!(e.cause(), crate::provider::ProviderError::Transport(_))
}

/// 子代理停旗（票 04）：非子代理恒 false。
fn halted_flag(ctx: &ToolContext) -> bool {
    ctx.subagent
        .as_ref()
        .is_some_and(|s| s.halt.load(std::sync::atomic::Ordering::Relaxed))
}

/// 叫停判定（暂停旗 + 子代理停旗）：等网睡眠切片与流 delta 缝共用一处。
pub(crate) fn halted(db: &Db, ctx: &ToolContext) -> bool {
    crate::orchestra::is_paused(db, &ctx.project_id).unwrap_or(false)
        || halted_flag(ctx)
        || crate::evaluation::control::checkpoint(&ctx.repo_root).is_err()
}

/// 等网退出留痕（票 01）：一条系统事件带探针数与等网时长 + 一帧清旗。
/// 进入/退出各一条；失败探针不逐条落事件——5s 一次的探针刷屏没有信息量，
/// 「等了多久、探了几次、怎么退出的」在退出事件里一次说清。
fn leave_wait(
    db: &Db,
    ctx: &ToolContext,
    sink: &mut DeltaSink<'_>,
    call: usize,
    since: std::time::Instant,
    probes: usize,
    reason: &str,
) {
    let _ = db.append_event(
        &ctx.project_id,
        EventKind::System,
        json!({"kind": "net_wait_exit", "call": call, "probes": probes,
               "elapsed_ms": since.elapsed().as_millis() as u64, "reason": reason}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    );
    sink(&TurnDelta {
        agent_id: ctx.agent_id.clone(),
        stage_run_id: ctx.stage_run_id.clone(),
        call,
        reset: false,
        done: false,
        waiting: false,
        text: String::new(),
        thinking: String::new(),
    });
}

/// 模型调用（流式）+ 两段式重试（US57 + network-resilience 票 01）。
///
/// 为什么两段而不是无限退避：快重试（100/250/500ms）吃亚秒级抖动，
/// 断网是真状态——Wi-Fi/VPN 抖一下不该判死回合，也不该用指数退避把
/// 恢复探测越推越慢。等网是「状态」不是「计数」：进入发 waiting 旗让
/// UI 把气泡转「重连中」，睡眠按 tick 切片随时响应叫停，进出各落一条
/// 系统事件，预算耗尽把 run 挂起等负责人恢复——这些语义塞不进
/// 「retry_count」一个字里。
#[allow(clippy::too_many_arguments)]
fn stream_with_retry(
    db: &Db,
    ctx: &ToolContext,
    provider: &dyn ModelProvider,
    req: &ChatRequest,
    call: usize,
    purpose: &str,
    sink: &mut DeltaSink<'_>,
) -> Result<ChatResponse, TurnError> {
    let mut attempt = 0usize;
    // 等网态三要素：起点（时长/预算）、探针数、是否已进入（事件只发一次）。
    let mut waiting_since: Option<std::time::Instant> = None;
    let mut probes = 0usize;
    loop {
        let mut emitted = false;
        let r = crate::usage::request(
            db,
            ctx,
            &req.model_slot,
            purpose,
            provider,
            Some(req),
            || {
                provider.stream(req, &mut |d| {
                    // 票 04：delta 间隙叫停检查——流循环阻塞在读上时，这里是
                    // 唯一能让负责人暂停生效的缝。is_paused 读库失败按未暂停
                    // 处理：暂停是尽力而为的中途检查，轮顶检查仍是权威闸。
                    // 票 04（code-search）：子代理的 halt 旗同缝检查——tasks stop
                    // 不需要等下一轮顶。
                    if halted(db, ctx) {
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
                        waiting: false,
                        text,
                        thinking,
                    });
                    true
                })
            },
        );
        match r {
            Ok(resp) => {
                // 探针成功：等网退出（resumed）再交回响应——半截文本的复位帧
                // 早随失败发过了，这里只收旗标。
                if let Some(since) = waiting_since.take() {
                    leave_wait(db, ctx, sink, call, since, probes, "resumed");
                }
                return Ok(resp);
            }
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
                        waiting: false,
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
            Err(e) if retryable(&e) => {
                // ---- 第二段：等网（Transport 才进，别的错上面直传）----
                if waiting_since.is_none() {
                    waiting_since = Some(std::time::Instant::now());
                    crate::diag::note(
                        crate::diag::CLASS_JUDGE,
                        false,
                        Some(&ctx.project_id),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                        None,
                        "net_wait",
                        "enter",
                        std::time::Instant::now(),
                    );
                    db.append_event(
                        &ctx.project_id,
                        EventKind::System,
                        json!({"kind": "net_wait_enter", "call": call,
                               "attempts": RETRY_DELAYS_MS.len()}),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                    // 进入等网：waiting 旗与复位帧合一——已吐半截文本的
                    // 先作废再转重连态，UI 上是一帧语义不是两帧。
                    sink(&TurnDelta {
                        agent_id: ctx.agent_id.clone(),
                        stage_run_id: ctx.stage_run_id.clone(),
                        call,
                        reset: emitted,
                        done: false,
                        waiting: true,
                        text: String::new(),
                        thinking: String::new(),
                    });
                } else if emitted {
                    // 等网中某次探针吐了半截又挂：重发前作废它。
                    sink(&TurnDelta {
                        agent_id: ctx.agent_id.clone(),
                        stage_run_id: ctx.stage_run_id.clone(),
                        call,
                        reset: true,
                        done: false,
                        waiting: true,
                        text: String::new(),
                        thinking: String::new(),
                    });
                }
                let since = waiting_since.unwrap();
                if since.elapsed() > ctx.wait.budget {
                    // 预算耗尽：等网转挂起。run 标 interrupted + 恢复卡入队
                    // 归 orchestra::suspend_run（属主模块），本层不碰两表。
                    // 挂起复用 interrupted 终态不是第三种 run 状态词：
                    // 恢复语义与进程被杀相同（解锁→重触发），另造词只会让
                    // 恢复面分叉。
                    leave_wait(db, ctx, sink, call, since, probes, "timeout");
                    crate::diag::note(
                        crate::diag::CLASS_REJECT,
                        true,
                        Some(&ctx.project_id),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                        None,
                        "net_wait",
                        "suspend_budget",
                        since,
                    );
                    if let Some(run_id) = ctx.stage_run_id.as_deref() {
                        crate::orchestra::suspend_run(db, &ctx.project_id, run_id, &ctx.agent_id)?;
                    }
                    return Err(TurnError::Suspended);
                }
                // 等网睡眠按 tick 切片：每片末尾查叫停，暂停响应延迟 ≤ tick。
                let mut left = ctx.wait.probe_interval;
                while left > std::time::Duration::ZERO {
                    let slice = left.min(ctx.wait.tick);
                    std::thread::sleep(slice);
                    left -= slice;
                    if halted(db, ctx) {
                        leave_wait(db, ctx, sink, call, since, probes, "paused");
                        return Err(crate::provider::ProviderError::Interrupted.into());
                    }
                }
                probes += 1;
            }
            Err(e) => {
                // 等网中探到非 Transport 错：等网态收口再上报（退出原因
                // 「error」——不是恢复也不是暂停，是另一类失败把它顶出去）。
                if let Some(since) = waiting_since.take() {
                    leave_wait(db, ctx, sink, call, since, probes, "error");
                }
                return Err(e.into());
            }
        }
    }
}

/// 工具执行值 → ToolResult 块（票 02）：`v["image"]`（fs_read 读图产出
/// `{media_type, data}`）剥成 images 载荷；content 里图片占位符替数据，
/// 字节本体不进文本。Anthropic 形状进 tool_result.content；OpenAI
/// 由映射层补 user 消息（ADR 0058-2）。
pub(crate) fn tool_result_block(tool_use_id: String, v: &Value) -> ContentBlock {
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
        content: render_tool_value(content),
        is_error: false,
        images,
    }
}

/// 结果正文按原文呈现（prompt-engineering 票 01）：整体 JSON 序列化会把
/// 换行和引号转义，模型读代码要先反转义，抄进 fs_patch.old 时还容易带上
/// `\n` 字面量。其余字段压成一行元信息放在正文之前。
fn render_tool_value(v: Value) -> String {
    let Value::Object(mut map) = v else {
        return v.to_string();
    };
    let body = match map.remove("content") {
        Some(Value::String(body)) => body,
        other => {
            if let Some(v) = other {
                map.insert("content".into(), v);
            }
            return Value::Object(map).to_string();
        }
    };
    if map.is_empty() {
        body
    } else {
        format!("{}\n{body}", Value::Object(map))
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
    /// 断网等网超预算挂起（票 01）：run 已被 suspend_run 标
    /// interrupted + 恢复卡入队；回合以「没正常完」收口但与
    /// Interrupted（人主动叫停）可区分。
    Suspended,
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
        &[],
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
        &[],
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
        &[],
    )
}

/// Resume evidence is already scoped by the calling facade. Preserve message
/// roles so historical tool output never becomes an owner/system instruction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_turn_streaming_with_history(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    layers: Vec<PromptLayer>,
    user_input: &str,
    attachments: &[crate::trace::AttachRef],
    plan_first: bool,
    sink: Option<&mut DeltaSink<'_>>,
    history: &[Message],
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
        history,
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
    history: &[Message],
) -> Result<TurnOutcome, TurnError> {
    let mut noop = |_d: &TurnDelta| {};
    let sink: &mut DeltaSink = match sink {
        Some(s) => s,
        None => &mut noop,
    };
    crate::actions::ensure_clear(db, ctx)?;
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
    // 票 07：激活任务清单随激活生灭——父级回合起跑线即激活边界，
    // 残留条目（含上次没收尾的子代理派遣）先 halt 再清空。
    // 子代理回合（ctx.subagent 在场）共享父板，不得清场。
    if ctx.subagent.is_none() {
        ctx.tasks
            .begin_activation(&crate::subagent::activation_key(ctx));
    }
    log::info!(
        "turn start: agent={} run={:?}",
        ctx.agent_id,
        ctx.stage_run_id
    );

    let source_mode = prompt::source_mode(user_input);
    let trigger_turn_id = db.append_event(
        &ctx.project_id,
        EventKind::TurnStarted,
        json!({ "agent": ctx.agent_id, "source_mode": source_mode, "subagent": ctx.subagent.is_some() }),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;

    let brief = build_brief_context(db, &ctx.agent_id, ctx.stage_run_id.as_deref())?;
    // 票 07（prompt-engineering）：工作台基础层 + 回复语言段在装配点统一
    // 注入——所有走回合内核的入口（派活、点名、恢复重触发、子代理）同享。
    let mut layers = {
        let mut base = crate::turn::prompt::workbench_layers(ctx.subagent.is_some());
        base.extend(layers);
        base
    };
    // US72：项目说明全文进激活首条消息（AgentsMd 层，优先级链位 2）。
    // 超 ~32KB 降级为头部+节标题目录+必读指引，并提醒负责人。
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

    // 2026-09-28 evaluation pilot: permission resolution starts a fresh turn.
    // Rehydrate recorded observations before steering/cap handling, instead of
    // silently losing the task's tool receipts whenever the CLI is reopened.
    messages.extend_from_slice(history);

    // 票 03：负责人附件注入。vision 槽 → Image 块进首条 user 消息；
    // 非 vision 槽 → [image: name](path) 降级文本 + attachments_degraded
    // 事件（时间线提示行）。字节嗅探复核——行内 media_type 不可信。
    if !attachments.is_empty() {
        let vision = ctx.caps.contains("vision");
        let mut degraded = 0usize;
        for r in attachments.iter().take(crate::tools::ATTACH_MAX_COUNT) {
            let Ok(p) = crate::tools::readable_repo_path(&ctx.repo_root, &r.path) else {
                continue;
            };
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
        // 票 03：caps 含 web 的槽（Anthropic 原生 web_search 在场）
        // 本地 web_search 不上清单——同名撞车且规格要求二选一。
        tools: registry.defs_for_ctx(Some(ctx)),
    };

    // 票 02 / ADR 0068：撞限闸按「实际服务的那个模型」的窗口收编——
    // 元数据在 make_provider 挂槽位时已解析进实例；测试桩/未识别模型
    // → None → 回落 120k，旧行为不回归。
    let cap = context::effective_cap(provider.model_meta().context_window);

    let outcome = (|| -> Result<TurnOutcome, TurnError> {
        // 票 05 steering 水位：回合起跑线之后新来的 owner 消息在
        // 每轮顶注入出站副本。只追加进本轮出站 messages——
        // owner_message 由 send_message 落库；注入模型可见 ≠ 改持久历史
        // （archive _outbound_messages 同款分层：模型视图与持久层分离）。
        // 票 06：工具轮的思考先攒着，跟下一条可见回复一起落，不另起一条消息。
        let mut carried_thinking = String::new();
        // 方案正文。不单独上时间线；执行轮没有可见回复时用它兜底，避免方案丢光。
        let mut plan_text = String::new();
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
                                "instruction": PLAN_FIRST_INSTRUCTION,
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
            let resp = match stream_with_retry(db, ctx, provider, &plan_req, 0, "planning", sink) {
                Ok(r) => r,
                // 票 04：流中被叫停 → Interrupted 终态（不上抛成错误）
                Err(TurnError::Provider(crate::provider::ProviderError::Interrupted)) => {
                    return Ok(TurnOutcome::Interrupted);
                }
                // 票 01：等网超预算 → Suspended（run 已在 suspend_run 收口）
                Err(TurnError::Suspended) => return Ok(TurnOutcome::Suspended),
                Err(e) => return Err(e),
            };
            absorb_thinking(&resp.content, &mut carried_thinking);
            // 方案只进模型上下文，不另落一条时间线消息。再落一次会和后面的
            // 可见回复叠成两条几乎一样的发言（2026-09-22）。流式缓冲也清掉，
            // 避免气泡里把方案和正式回复拼成两段。
            plan_text = visible_text(&resp.content);
            sink(&TurnDelta {
                agent_id: ctx.agent_id.clone(),
                stage_run_id: ctx.stage_run_id.clone(),
                call: 0,
                reset: true,
                done: false,
                waiting: false,
                text: String::new(),
                thinking: String::new(),
            });
            push_assistant(&mut messages, resp.content);
        }
        // 同工具同错熔断状态：跨 round 连击计数（US57）
        let mut last_fail: Option<(String, String)> = None;
        let mut streak = 0usize;
        // 票 13：截断续推有上限（OPE-171 连败防循环；票 04 起上限 3）
        let mut trunc_count = 0usize;
        // 票 05：最近一次调用 tasks（或上次提醒）所在轮。
        let mut tasks_mark = 0usize;
        let mut repository_evidence = prompt::RepositoryEvidence::default();
        // An explicit owner-selected mode, not a language/keyword classifier.
        // Filename listing and ordinary conversation must remain valid without reads.
        let mut source_repair = false;
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
            // 票 04（code-search）：子代理停旗同闸——tasks stop 在轮顶生效。
            if crate::orchestra::is_paused(db, &ctx.project_id)? || halted_flag(ctx) {
                return Ok(TurnOutcome::Interrupted);
            }
            // 子代理中途复查父休眠（票 04：父休眠子代理不跑——派遣前置闸
            // 只管起跑，跑中睡死的在这里收口）。多读一行换一个语义闸。
            if ctx.subagent.is_some() {
                let sleeping: bool = db
                    .conn()
                    .query_row(
                        "SELECT status='sleeping' FROM agents WHERE id=?1",
                        [&ctx.agent_id],
                        |r| r.get(0),
                    )
                    .unwrap_or(false);
                if sleeping {
                    return Ok(TurnOutcome::Interrupted);
                }
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
            // 票 05（prompt-engineering）：任务清单提醒。普通 user 消息而非
            // 动态尾部块——尾部不进信封指纹，提醒却是模型可见的语义输入。
            // 子代理共享父板但不被提醒（它看不到 tasks 工具）。
            if ctx.subagent.is_none() && round >= tasks_mark + TASK_REMINDER_ROUNDS {
                if let Some(text) = task_reminder(ctx) {
                    tasks_mark = round;
                    messages.push(Message {
                        role: Role::User,
                        content: vec![ContentBlock::Text { text }],
                    });
                    db.append_event(
                        &ctx.project_id,
                        EventKind::System,
                        json!({"kind": "task_reminder", "round": round}),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                }
            }
            // US37 + 票 07：轻量裁剪始终做；仍超上限只删工具记录
            // （负责人与角色原文不换成摘要）。删完仍超才暂停问负责人。
            messages = trim_context(ctx, messages, true);
            let tail_tokens = estimate_tokens(&with_dynamic_tail(
                &[],
                round,
                MAX_TOOL_ROUNDS,
                source_mode.then_some(&repository_evidence),
            ));
            let mut est = estimate_tokens(&messages) + tail_tokens;
            if est > cap {
                // Evidence survives compaction; reserve its space before trimming.
                messages = mechanical_compact(db, ctx, messages, cap.saturating_sub(tail_tokens));
                messages = trim_context(ctx, messages, false);
                est = estimate_tokens(&messages) + tail_tokens;
            }
            if est > cap {
                return context_overflow(db, ctx, est, "estimate", cap, trigger_turn_id);
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
                messages: with_dynamic_tail(
                    &messages,
                    round,
                    MAX_TOOL_ROUNDS,
                    source_mode.then_some(&repository_evidence),
                ),
                ..req_base.clone()
            };
            let request_id = db.append_event(
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
                // Reliability 12: call index zero is also an ordinary first
                // turn. Purpose comes from this dispatch site, not its index.
                if ctx.subagent.is_some() {
                    "subagent"
                } else {
                    "turn"
                },
                sink,
            ) {
                Ok(r) => r,
                // 票 04：流中被叫停 → Interrupted 终态（不上抛成错误）
                Err(TurnError::Provider(crate::provider::ProviderError::Interrupted)) => {
                    return Ok(TurnOutcome::Interrupted);
                }
                // 票 01：等网超预算 → Suspended（run 已在 suspend_run 收口）
                Err(TurnError::Suspended) => return Ok(TurnOutcome::Suspended),
                Err(e) => return Err(e),
            };
            log::debug!(
                "model resp: stop={:?} prompt_tok={} completion_tok={}",
                resp.stop,
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens
            );
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
                if trunc_count < MAX_TRUNC_CONTINUES {
                    trunc_count += 1;
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
            trunc_count = 0;
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
                // 执行轮没吐字时用方案兜底，仍然只落一条。
                let mut text = visible_text(&resp.content);
                if text.trim().is_empty() {
                    text = std::mem::take(&mut plan_text);
                }
                let evidence_started = std::time::Instant::now();
                let supported = !source_mode || repository_evidence.supports_citation(&text);
                if source_mode {
                    crate::diag::note(
                        if supported {
                            crate::diag::CLASS_JUDGE
                        } else {
                            crate::diag::CLASS_REJECT
                        },
                        !supported,
                        Some(&ctx.project_id),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                        Some(&request_id.to_string()),
                        "source_answer",
                        if supported {
                            "read_reference_present"
                        } else {
                            "missing_source_evidence"
                        },
                        evidence_started,
                    );
                }
                if !supported {
                    // The streamed draft is provisional; do not leave it visible as
                    // the accepted answer while repairing or returning failure.
                    sink(&TurnDelta {
                        agent_id: ctx.agent_id.clone(),
                        stage_run_id: ctx.stage_run_id.clone(),
                        call: round + usize::from(plan_first),
                        reset: true,
                        done: false,
                        waiting: false,
                        text: String::new(),
                        thinking: String::new(),
                    });
                    if source_repair || round + 1 == MAX_TOOL_ROUNDS {
                        db.append_message(
                            &ctx.project_id,
                            crate::pm_route::WORKBENCH_AUTHOR,
                            crate::owner_text::source_unverified(),
                            &[],
                            &[],
                            None,
                            ctx.stage_run_id.as_deref(),
                        )?;
                        return Ok(TurnOutcome::Failed(
                            "source evidence missing: answer remains unverified".into(),
                        ));
                    }
                    source_repair = true;
                    messages.push(Message { role: Role::User, content: vec![ContentBlock::Text {
                        text: "The draft was not accepted: missing source evidence. Read the relevant implementation with fs_read and cite its exact repo-relative path:line (or path:start-end) within the returned non-truncated range. A search result, description or invented citation is insufficient. One repair opportunity remains; do not invent an answer if you cannot verify it.".into(),
                    }] });
                    continue;
                }
                persist_visible(db, ctx, &text, &mut carried_thinking)?;
                return Ok(TurnOutcome::Finished);
            }

            // 执行工具调用，结果回喂；同工具同错连 BREAKER_STREAK 次熔断（US57）
            let mut results = Vec::new();
            // reliability 08: round/index collide across fresh turns and fast
            // paths. Persisted request identity scopes the provider's call ID.
            for (id, name, input) in tool_uses {
                let evidence_input = (source_mode
                    && matches!(name.as_str(), "fs_read" | "fs_find" | "fs_grep"))
                .then(|| input.clone());
                let seq = json!([request_id, id]).to_string();
                if name == "tasks" {
                    tasks_mark = round;
                }
                // code-search 票 04：subagent 由 turn 层截获跑嵌套回合——
                // provider/子代理注册表/任务板都在这里才够得着，exec 层拿不到。
                let called = if name == "subagent" {
                    registry
                        .get("subagent")
                        .map_or(Ok(()), |t| registry.validate_input(ctx, t.as_ref(), &input))
                        .and_then(|()| {
                            crate::subagent::call_nested_with_seq(
                                db,
                                provider,
                                registry,
                                ctx,
                                input,
                                Some(&seq),
                            )
                        })
                } else {
                    registry.call_with_seq(db, ctx, &name, input, Some(&seq))
                };
                match called {
                    Ok(CallOutcome::Done(v)) => {
                        if let Some(input) = evidence_input.filter(|_| source_mode) {
                            repository_evidence.record(&name, &input, &v);
                        }
                        last_fail = None;
                        streak = 0;
                        results.push(tool_result_block(id, &v))
                    }
                    Ok(CallOutcome::Denied(reason)) => {
                        let sig = format!("denied {reason}");
                        if bump_streak(&mut last_fail, &mut streak, &name, &sig) >= BREAKER_STREAK {
                            return breaker(db, &name, &sig);
                        }
                        results.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: format!("{sig}. {DENIED_GUIDANCE}"),
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
                                        content: format!("denied {r}. {DENIED_GUIDANCE}"),
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
            crate::usage::record_tools(db, ctx, &req_base.model_slot, tool_bytes)?;
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
    // waiting:false 是 NR-02 的兜底清旗（等网中被打停/挂起的路径
    // 都以这一帧收尾，不必各处再补清旗帧）。
    sink(&TurnDelta {
        agent_id: ctx.agent_id.clone(),
        stage_run_id: ctx.stage_run_id.clone(),
        call: usize::MAX,
        reset: false,
        done: true,
        waiting: false,
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
            | Ok(TurnOutcome::Suspended)
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
                // Reliability 13: admission failure has a budget reason, not ambiguity.
                Ok(TurnOutcome::Truncated)
                | Err(TurnError::Provider(crate::provider::ProviderError::BudgetUnavailable)) => {
                    Some(crate::trace::FailureCode::BudgetExceeded)
                }
                Ok(TurnOutcome::Interrupted)
                | Ok(TurnOutcome::Suspended)
                | Ok(TurnOutcome::Failed(_))
                | Err(_) => Some(crate::trace::FailureCode::Ambiguous),
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

/// 未关闭条目（open/running）的提醒文案；没有则 None。
fn task_reminder(ctx: &ToolContext) -> Option<String> {
    let open: Vec<String> = ctx
        .tasks
        .list(&crate::subagent::activation_key(ctx))
        .into_iter()
        .filter(|t| matches!(t["status"].as_str(), Some("open" | "running")))
        .map(|t| {
            format!(
                "- [{}] {} ({})",
                t["status"].as_str().unwrap_or(""),
                t["title"].as_str().unwrap_or(""),
                t["id"].as_str().unwrap_or("")
            )
        })
        .collect();
    if open.is_empty() {
        return None;
    }
    Some(task_reminder_text(&open.join("\n")))
}

/// 压缩注记的模板形态（提示词页目录用）。
pub(crate) fn compaction_note_template() -> String {
    context::compaction_note("{count}", "{transcript path}")
}

/// 任务清单提醒正文（票 05）；提示词页目录与运行时共用这一份。
pub(crate) fn task_reminder_text(open_tasks: &str) -> String {
    format!(
        "[workbench] Task list reminder: you have open tasks in this activation and have not touched the list for {TASK_REMINDER_ROUNDS} rounds. If it no longer matches your work, update it with the tasks tool (close finished items, collect subagent results). Ignore this if it is still accurate, and do not mention this reminder to the owner.\nOpen tasks:\n{open_tasks}"
    )
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
/// 票 04（code-search）：子代理域不上时间线——它的产出走回执格
/// （scope.answer），气泡永远是父代理自己的回复；只读回合曾在
/// 时间线叠第二条「助手」消息是 US36 的已纠行为。
fn persist_visible(
    db: &Db,
    ctx: &ToolContext,
    text: &str,
    carried: &mut String,
) -> Result<(), TurnError> {
    if text.is_empty() && carried.is_empty() {
        return Ok(());
    }
    if let Some(scope) = &ctx.subagent {
        *scope.answer.lock().unwrap() = Some(text.to_string());
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
