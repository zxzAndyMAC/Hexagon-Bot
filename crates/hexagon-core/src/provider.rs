//! 模型供应商抽象 + 脚本化假供应商。
//!
//! 这是测试主接缝的关键替身：回合内核只面向 `ModelProvider` trait，
//! 测试用 `ScriptedProvider` 按脚本回放响应（含工具调用），把 LLM 的
//! 非确定性隔离在接缝之外。
//!
//! trait 是同步的：模型调用是叶子级阻塞 IO，回合内核（票 06）用
//! `spawn_blocking` 包装即可，本层不引入运行时依赖。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// base64 图（agent-senses 票 02）：media_type + data。
/// 字节本体只走这张图通道进上下文，不走文本载荷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageData {
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        /// 票 02：工具结果随带图（fs_read 读图）。Anthropic 形状进
        /// tool_result.content 数组；OpenAI 形状 tool 消息只收字符串——
        /// 映射层在其后补一条带图的 user 消息（ADR 0058-2 降级路径）。
        #[serde(default)]
        images: Vec<ImageData>,
    },
    /// 消息内联图（用户贴图等；票 03  Composer 附件产它）。
    Image {
        media_type: String,
        data: String,
    },
    /// 供应商原生块透传（agent-senses 票 04）：server_tool_use /
    /// web_search_tool_result 等服务端已执行块——原样保史并在下次
    /// 请求原样回显（Anthropic 要求 server 工具块随历史回传）。
    /// 不落成 ToolUse：本地注册表无 web_search，执行会回 unknown tool
    /// 假错——「turn 层无感」的正确含义是不执行而不是换个名执行。
    Opaque {
        /// 供应商原始块 JSON（含 type 字段）。
        raw: Value,
    },
    /// 模型给出的推理文本（hands-free 票 06）。只进时间线，不回灌下一轮请求。
    /// 否决：拼进 Text（推理会变成可见回复，也变成已承诺的计划）；
    /// 否决：原样回传 thinking 块（Anthropic 要 signature，我们没有，
    /// 回传会被拒，或把推理当成下一步）。没收到明文就不构造这一块。
    Thinking {
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model_slot: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Error,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    /// Additional billing dimensions outside the local two-rate table.
    pub unpriced: bool,
    /// Missing counters remain explicitly unknown; zero defaults are not
    /// evidence that a request was free (D06 / reliability 12).
    pub prompt_reported: bool,
    pub completion_reported: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl Usage {
    fn from_value(value: &Value, input: &str, output: &str) -> Self {
        let mut usage = Self::default();
        usage.observe(value, input, output);
        usage
    }

    fn observe(&mut self, value: &Value, input: &str, output: &str) {
        if let Some(tokens) = value[input].as_u64() {
            self.prompt_tokens = tokens;
            self.prompt_reported = true;
        }
        if let Some(tokens) = value[output].as_u64() {
            self.completion_tokens = tokens;
            self.completion_reported = true;
        }
        // Reliability 12 / Anthropic Usage + streaming accumulator contract:
        // delta totals overwrite each field independently; absent fields retain
        // earlier counts. Cache/server-tool charges need separate prices. A
        // false unknown costs review; false known hides an unbudgeted charge.
        self.unpriced |= value.as_object().is_some_and(|fields| {
            fields.iter().any(|(key, v)| {
                !matches!(
                    key.as_str(),
                    "input_tokens"
                        | "output_tokens"
                        | "prompt_tokens"
                        | "completion_tokens"
                        | "total_tokens"
                        | "output_tokens_details"
                ) && !(key == "service_tier" && v == "standard")
                    && nonzero_usage(v)
            })
        });
    }
}

fn nonzero_usage(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64() != Some(0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => v.iter().any(nonzero_usage),
        Value::Object(v) => v.values().any(nonzero_usage),
    }
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub stop: StopReason,
    pub usage: Usage,
}

/// 流式增量（turn-streaming 票 01）：文本 + 思考。tool_use 增量不进
/// delta 通道，由最终 ChatResponse 统一承载（拼装正确性归折叠器，
/// UI 不该面对 partial tool_call）。
/// Thinking 只在供应商明文推理增量上发——没有就不发，不编造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDelta {
    Text(String),
    /// hands-free 票 06：模型给出的推理增量。
    Thinking(String),
}

/// sink 返回 false = 叫停（票 04 的唯一反压通道：流循环阻塞在读上，
/// 外部唯一能说「停」的时刻就是两次 delta 之间）。
pub type StreamSink<'a> = dyn FnMut(&StreamDelta) -> bool + 'a;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("scripted provider: script exhausted")]
    ScriptExhausted,
    /// Reliability 13: cancellation/transport failure cannot erase usage
    /// already received. The request ledger settles this before propagating cause.
    #[error("{0}")]
    WithUsage(Box<ProviderError>, Usage),
    #[error("transport: {0}")]
    Transport(String),
    #[error("refused: {0}")]
    Refused(String),
    /// Reliability 13: local admission failure, never a supplier refusal.
    #[error(
        "budget unavailable for this request; wait for in-flight requests or adjust the budget"
    )]
    BudgetUnavailable,
    #[error("missing credential: {0}")]
    MissingCredential(String),
    /// 流被 sink 叫停（票 04）：显式终态——不是传输故障不可重试，
    /// 也不是拒绝；turn 层映射为 Interrupted 终态。
    #[error("interrupted")]
    Interrupted,
}

impl ProviderError {
    fn with_usage(self, usage: &Usage) -> Self {
        if matches!(self, Self::WithUsage(..))
            || !(usage.prompt_reported || usage.completion_reported)
        {
            self
        } else {
            Self::WithUsage(Box::new(self), usage.clone())
        }
    }

    pub(crate) fn cause(&self) -> &Self {
        match self {
            Self::WithUsage(source, _) => source.cause(),
            _ => self,
        }
    }

    pub(crate) fn usage(&self) -> Option<&Usage> {
        match self {
            Self::WithUsage(_, usage) => Some(usage),
            _ => None,
        }
    }

    pub(crate) fn into_cause(self) -> Self {
        match self {
            Self::WithUsage(source, _) => source.into_cause(),
            _ => self,
        }
    }
}

/// 实例所服务模型的元数据（context-window 票 02 / ADR 0068）：
/// `make_provider` 挂槽位时从 ModelEntry（或内置前缀表）解析好随实例
/// 带上——撞限闸与 max_tokens 以实际服务的模型为准。None = 未知，
/// 调用方回落默认（撞限闸 120k；聊天 HTTP 输出上限 8192）。
/// reliability 13 的预算预占与实际 HTTP 请求使用同一输出上限。
/// 测试桩保持 None：撞限测试恒定跑 120k，不随
/// 本机 providers.json 漂移。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelMeta {
    pub context_window: Option<u64>,
    pub max_output: Option<u64>,
}

pub trait ModelProvider: Send + Sync {
    /// Enforced output bound, not an expected response length. Implementations
    /// advertising this must apply it to the actual transport request (D06).
    fn output_token_limit(&self) -> Option<u64> {
        self.model_meta().max_output.filter(|v| *v > 0)
    }
    /// Physical model identity, if known; a logical slot is not a model name.
    fn billing_model(&self) -> Option<&str> {
        None
    }
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError>;

    /// 所服务模型的元数据（见 [`ModelMeta`]）。默认全 None = 未知。
    fn model_meta(&self) -> ModelMeta {
        ModelMeta::default()
    }

    /// 供应商原生 server tools（agent-senses 票 04）：返回请求组装时
    /// 并入 tools 数组的原始 JSON 定义（如 Anthropic web_search_20250305）。
    /// 默认空集——无此能力的供应商（OpenAI 形状）缺席是预期（ADR 0058-1：
    /// 不自建爬虫顶替；caps 过滤在调用方做，这里只给形状）。
    fn server_tools(&self, _model: &str) -> Vec<Value> {
        vec![]
    }

    /// 流式完成（票 01）。默认实现 = complete + 一次性发全文 delta——
    /// 没实现真流式的 provider 零改动兼容（aisuite Provider 基类同款
    /// 「默认兜底」先例：能力缺口走诚实路径，不造假流）。
    /// sink 返回 false → Err(Interrupted)（票 04：叫停是显式终态不算失败）。
    fn stream(
        &self,
        req: &ChatRequest,
        sink: &mut StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        let resp = self.complete(req)?;
        if !emit_content_deltas(&resp.content, usize::MAX, sink) {
            return Err(ProviderError::Interrupted.with_usage(&resp.usage));
        }
        Ok(resp)
    }

    /// 决策模型（Jev）走自己的选择题接口。聊天供应商保持 false，封闭选择仍走 [`complete`]。
    fn uses_decision_api(&self) -> bool {
        false
    }

    /// `options` 是（选项原文，短说明）。返回正文必须是被选中的那一项原文。
    fn decide(
        &self,
        _state: &str,
        _options: &[(&str, &str)],
    ) -> Result<ChatResponse, ProviderError> {
        Err(ProviderError::Refused(
            "this provider has no decision API".into(),
        ))
    }
}

/// 按内容块顺序把可见文本和思考切成 ≤n 字符的 delta（n=usize::MAX 整段一次发）。
/// 按 char 不按 byte。思考块单独走 Thinking，不并进 Text。
/// 返回 false = sink 叫停。
fn emit_content_deltas(blocks: &[ContentBlock], n: usize, sink: &mut StreamSink<'_>) -> bool {
    for b in blocks {
        let (text, thinking) = match b {
            ContentBlock::Text { text } => (text.as_str(), false),
            ContentBlock::Thinking { text } => (text.as_str(), true),
            _ => continue,
        };
        if !emit_chunks(text, thinking, n, sink) {
            return false;
        }
    }
    true
}

fn emit_chunks(text: &str, thinking: bool, n: usize, sink: &mut StreamSink<'_>) -> bool {
    if text.is_empty() {
        return true;
    }
    let emit = |chunk: String, sink: &mut StreamSink<'_>| -> bool {
        let d = if thinking {
            StreamDelta::Thinking(chunk)
        } else {
            StreamDelta::Text(chunk)
        };
        sink(&d)
    };
    if n == usize::MAX {
        return emit(text.to_string(), sink);
    }
    let mut buf = String::new();
    for (i, ch) in text.chars().enumerate() {
        buf.push(ch);
        if (i + 1) % n == 0 && !emit(std::mem::take(&mut buf), sink) {
            return false;
        }
    }
    if !buf.is_empty() && !emit(buf, sink) {
        return false;
    }
    true
}

// ---------- SSE（turn-streaming 票 02）----------

/// SSE 块读取器：逐行读，空行派发一块（event 名 + data 负载）。
/// 注释行（`:` 开头）与畸形行跳过不崩——fail toward 完整性（spec 约定）：
/// 宁可丢一行噪声，不让一行畸形文本掐断整个流。
struct SseBlocks<R: std::io::BufRead> {
    r: R,
    event: String,
    data: String,
}

impl<R: std::io::BufRead> SseBlocks<R> {
    fn new(r: R) -> Self {
        Self {
            r,
            event: String::new(),
            data: String::new(),
        }
    }

    /// 下一块：(event 名, data 负载)；None = EOF。块间状态自复。
    fn next_block(&mut self) -> Result<Option<(String, String)>, ProviderError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .r
                .read_line(&mut line)
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            if n == 0 {
                // EOF：手里有半块也派发（不少端点末块后不补空行）
                if !self.data.is_empty() {
                    let out = (
                        std::mem::take(&mut self.event),
                        std::mem::take(&mut self.data),
                    );
                    return Ok(Some(out));
                }
                return Ok(None);
            }
            let l = line.trim_end_matches(['\r', '\n']);
            if l.is_empty() {
                if !self.data.is_empty() {
                    let out = (
                        std::mem::take(&mut self.event),
                        std::mem::take(&mut self.data),
                    );
                    return Ok(Some(out));
                }
                self.event.clear();
                continue;
            }
            if l.starts_with(':') {
                continue; // 注释/心跳行
            }
            if let Some(rest) = l.strip_prefix("data:") {
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(rest);
                continue;
            }
            if let Some(rest) = l.strip_prefix("event:") {
                self.event = rest.strip_prefix(' ').unwrap_or(rest).to_string();
                continue;
            }
            // id:/retry:/畸形行：跳过
        }
    }
}

/// 模型槽：角色绑定的模型配置。凭据只按名字引用，明文永不出现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSlot {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub credential_ref: Option<String>,
}

/// 脚本化假供应商：按序回放响应，并记录每个收到的请求供断言。
pub struct ScriptedProvider {
    script: Mutex<VecDeque<ChatResponse>>,
    calls: Mutex<Vec<ChatRequest>>,
    /// >0 时 stream() 把文本按该字符数切块发 delta（0=整块，票 01 测试驱动）。
    chunk_chars: usize,
}

impl ScriptedProvider {
    pub fn new(script: Vec<ChatResponse>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            calls: Mutex::new(Vec::new()),
            chunk_chars: 0,
        }
    }

    /// 分块回放：delta 切块只是展示节奏，最终 ChatResponse 与 complete 一致。
    pub fn chunked(script: Vec<ChatResponse>, chunk_chars: usize) -> Self {
        Self {
            script: Mutex::new(script.into()),
            calls: Mutex::new(Vec::new()),
            chunk_chars,
        }
    }

    pub fn recorded(&self) -> Vec<ChatRequest> {
        self.calls.lock().unwrap().clone()
    }
}

impl ModelProvider for ScriptedProvider {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.calls.lock().unwrap().push(req.clone());
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(ProviderError::ScriptExhausted)
    }

    fn stream(
        &self,
        req: &ChatRequest,
        sink: &mut StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        let resp = self.complete(req)?;
        let n = if self.chunk_chars == 0 {
            usize::MAX
        } else {
            self.chunk_chars
        };
        if !emit_content_deltas(&resp.content, n, sink) {
            return Err(ProviderError::Interrupted.with_usage(&resp.usage));
        }
        Ok(resp)
    }
}

/// OpenAI 兼容形状的请求/响应映射（纯函数，不碰网络——传输层归具体供应商实现）。
pub mod openai_shape {
    use super::*;

    /// 票 02：OpenAI tool 消息只收字符串——带图 tool_result 降级为
    /// 「tool 消息（文本）+ 紧随的 user 消息（image_url 部件）」
    /// （ADR 0058-2；缺图沉默丢失比多一条 user 消息更坏）。
    pub fn to_request(req: &ChatRequest) -> Value {
        let mut messages: Vec<Value> = Vec::new();
        for m in &req.messages {
            let role = serde_json::to_value(&m.role).unwrap();
            let mut content_parts = Vec::new();
            let mut tool_calls = Vec::new();
            let mut carried_images: Vec<&ImageData> = Vec::new();
            let mut tool_msgs = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::Text { text } => {
                        content_parts.push(serde_json::json!({"type":"text","text":text}))
                    }
                    ContentBlock::Image { media_type, data } => {
                        content_parts.push(serde_json::json!({
                            "type":"image_url",
                            "image_url":{"url":format!("data:{media_type};base64,{data}")}
                        }))
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(serde_json::json!({
                            "id": id, "type": "function",
                            "function": {"name": name, "arguments": input.to_string()}
                        }))
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        images,
                        ..
                    } => {
                        carried_images.extend(images.iter());
                        tool_msgs.push(serde_json::json!({
                            "role": "tool", "tool_call_id": tool_use_id, "content": content
                        }));
                    }
                    // hands-free 票 06：思考不进出站请求（见 ContentBlock::Thinking）。
                    ContentBlock::Thinking { .. } => {}
                    // 票 04：OpenAI 形状无 server tool 概念——折成文本占位
                    // 保史（跨供应商回放时语义不断片，而非整块蒸发）。
                    ContentBlock::Opaque { raw } => {
                        let t = raw["type"].as_str().unwrap_or("server_tool");
                        content_parts.push(serde_json::json!({
                            "type":"text","text":format!("[{t}]")
                        }))
                    }
                }
            }
            if !tool_msgs.is_empty() {
                messages.extend(tool_msgs);
                if !carried_images.is_empty() {
                    let mut parts = vec![serde_json::json!({
                        "type":"text","text":"[tool result image(s) above]"}
                    )];
                    for im in &carried_images {
                        parts.push(serde_json::json!({
                            "type":"image_url",
                            "image_url":{"url":format!("data:{};base64,{}",im.media_type,im.data)}
                        }));
                    }
                    messages.push(serde_json::json!({"role":"user","content":parts}));
                }
                continue;
            }
            let mut msg = serde_json::json!({"role": role});
            if !content_parts.is_empty() {
                msg["content"] = serde_json::json!(content_parts);
            }
            if !tool_calls.is_empty() {
                msg["tool_calls"] = serde_json::json!(tool_calls);
            }
            messages.push(msg);
        }
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        serde_json::json!({ "messages": messages, "tools": tools })
    }

    pub fn from_response(v: &Value) -> Result<ChatResponse, ProviderError> {
        let choice = v["choices"][0].clone();
        let msg = &choice["message"];
        let mut content = Vec::new();
        // 明文推理才收：reasoning 若是对象（结构化摘要）不当成思考文本。
        if let Some(t) = msg["reasoning_content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| msg["reasoning"].as_str().filter(|s| !s.is_empty()))
        {
            content.push(ContentBlock::Thinking {
                text: t.to_string(),
            });
        }
        if let Some(text) = msg["content"].as_str() {
            content.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for c in calls {
                let input: Value =
                    serde_json::from_str(c["function"]["arguments"].as_str().unwrap_or("{}"))
                        .map_err(|e| ProviderError::Transport(e.to_string()))?;
                content.push(ContentBlock::ToolUse {
                    id: c["id"].as_str().unwrap_or_default().to_string(),
                    name: c["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    input,
                });
            }
        }
        let stop = match choice["finish_reason"].as_str() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };
        Ok(ChatResponse {
            content,
            stop,
            usage: Usage::from_value(&v["usage"], "prompt_tokens", "completion_tokens"),
        })
    }

    /// SSE 流折叠器（票 02）：增量收 choices[].delta——content 即文本；
    /// tool_calls 按 index 累积 id/name/arguments 字符串片段；
    /// `data: [DONE]` 终；usage 走末块（需请求带 stream_options.include_usage）。
    /// sink 叫停 → Err(Interrupted)（票 04 通道）。
    #[derive(Default)]
    pub struct SseFold {
        text: String,
        /// 明文推理（reasoning_content / reasoning 字符串）。对象形态不收。
        thinking: String,
        /// index → (id, name, arguments 片段缓冲)。BTreeMap 保 index 序。
        tools: std::collections::BTreeMap<usize, (String, String, String)>,
        finish: Option<String>,
        pub(super) usage: Usage,
    }

    impl SseFold {
        /// 喂一块 data 负载；true = 流终（[DONE]）。
        pub fn data(
            &mut self,
            data: &str,
            sink: &mut StreamSink<'_>,
        ) -> Result<bool, ProviderError> {
            if data.trim() == "[DONE]" {
                return Ok(true);
            }
            let v: Value = serde_json::from_str(data)
                .map_err(|e| ProviderError::Transport(format!("sse json: {e}")))?;
            if v["usage"].is_object() {
                self.usage
                    .observe(&v["usage"], "prompt_tokens", "completion_tokens");
            }
            for ch in v["choices"].as_array().into_iter().flatten() {
                let d = &ch["delta"];
                if let Some(t) = d["reasoning_content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| d["reasoning"].as_str().filter(|s| !s.is_empty()))
                {
                    self.thinking.push_str(t);
                    if !sink(&StreamDelta::Thinking(t.to_string())) {
                        return Err(ProviderError::Interrupted);
                    }
                }
                if let Some(t) = d["content"].as_str() {
                    if !t.is_empty() {
                        self.text.push_str(t);
                        if !sink(&StreamDelta::Text(t.to_string())) {
                            return Err(ProviderError::Interrupted);
                        }
                    }
                }
                if let Some(calls) = d["tool_calls"].as_array() {
                    for c in calls {
                        let idx = c["index"].as_u64().unwrap_or(0) as usize;
                        let e = self.tools.entry(idx).or_default();
                        if let Some(id) = c["id"].as_str() {
                            e.0 = id.into();
                        }
                        if let Some(n) = c["function"]["name"].as_str() {
                            e.1.push_str(n);
                        }
                        if let Some(a) = c["function"]["arguments"].as_str() {
                            e.2.push_str(a);
                        }
                    }
                }
                if let Some(f) = ch["finish_reason"].as_str() {
                    self.finish = Some(f.into());
                }
            }
            Ok(false)
        }

        /// 流终折叠成 ChatResponse。arguments 片段拼不出 JSON = 完整性
        /// 问题 → Transport（不是丢一半静默收场）。
        pub fn finish(self) -> Result<ChatResponse, ProviderError> {
            let mut content = Vec::new();
            if !self.thinking.is_empty() {
                content.push(ContentBlock::Thinking {
                    text: self.thinking,
                });
            }
            if !self.text.is_empty() {
                content.push(ContentBlock::Text { text: self.text });
            }
            for (_i, (id, name, args)) in self.tools {
                let input: Value = serde_json::from_str(if args.is_empty() { "{}" } else { &args })
                    .map_err(|e| ProviderError::Transport(format!("tool args json: {e}")))?;
                content.push(ContentBlock::ToolUse { id, name, input });
            }
            let stop = match self.finish.as_deref() {
                Some("tool_calls") => StopReason::ToolUse,
                Some("length") => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            };
            Ok(ChatResponse {
                content,
                stop,
                usage: self.usage,
            })
        }
    }
}
pub mod anthropic_shape {
    use super::*;

    /// system 消息抽顶字段；ToolResult 归 user 消息；其余角色/块近直译。
    /// `server_tools`：供应商原生工具定义（票 04 web_search_20250305），
    /// 已是目标形状原样并入 tools 数组。
    ///
    /// 挂账（ADR 0068 票 03 否决项）：Anthropic `cache_control` 显式断点
    /// 未加——前缀已按字节稳定（turn 层 with_dynamic_tail 把易变内容压
    /// 在末尾），主力流量走 OpenAI 形状（阿里系服务端自动前缀缓存，
    /// 无此字段），唯一受益的 anthropic 兼容端点是否透传断点未验证。
    /// 直连真 Anthropic 出现时再加断点，勿提前上复杂度。
    pub fn to_request(
        req: &ChatRequest,
        model: &str,
        max_tokens: u64,
        server_tools: &[Value],
    ) -> Value {
        let mut system_parts = Vec::new();
        let mut messages = Vec::new();
        for m in &req.messages {
            let blocks: Vec<Value> = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => {
                        Some(serde_json::json!({"type":"text","text":text}))
                    }
                    ContentBlock::Image { media_type, data } => Some(serde_json::json!({
                        "type":"image",
                        "source":{"type":"base64","media_type":media_type,"data":data}
                    })),
                    ContentBlock::ToolUse { id, name, input } => Some(
                        serde_json::json!({"type":"tool_use","id":id,"name":name,"input":input}),
                    ),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        images,
                    } => {
                        // Anthropic tool_result.content 原生收 image 块——
                        // 图留在结果体内（与 OpenAI 补 user 消息的降级不同轴，
                        // ADR 0058-2）。
                        let mut parts = vec![serde_json::json!({"type":"text","text":content})];
                        for im in images {
                            parts.push(serde_json::json!({
                                "type":"image",
                                "source":{"type":"base64","media_type":im.media_type,"data":im.data}
                            }));
                        }
                        Some(serde_json::json!({
                            "type":"tool_result","tool_use_id":tool_use_id,
                            "content":parts,
                            "is_error":is_error,
                        }))
                    }
                    // 票 04：server 工具块原样回显——Anthropic 要求它们
                    // 以原块类型留在历史里，否则下一轮 400。
                    ContentBlock::Opaque { raw } => Some(raw.clone()),
                    // hands-free 票 06：思考不回灌（无 signature，见 ContentBlock::Thinking）。
                    ContentBlock::Thinking { .. } => None,
                })
                .collect();
            match m.role {
                Role::System => {
                    for b in &m.content {
                        if let ContentBlock::Text { text } = b {
                            system_parts.push(text.clone());
                        }
                    }
                }
                // Anthropic 无 tool 角色：tool_result 块走 user 消息。
                Role::Tool => messages.push(serde_json::json!({"role":"user","content":blocks})),
                _ => messages.push(serde_json::json!({
                    "role": serde_json::to_value(&m.role).unwrap(), "content": blocks
                })),
            }
        }
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name, "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            // 票 04：server tools 原样并入（已是供应商形状，不走 ToolDef 映射）
            .chain(server_tools.iter().cloned())
            .collect();
        let mut body = serde_json::json!({
            "model": model, "max_tokens": max_tokens, "messages": messages,
        });
        if !system_parts.is_empty() {
            body["system"] = serde_json::json!(system_parts.join("\n\n"));
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        body
    }

    pub fn from_response(v: &Value) -> Result<ChatResponse, ProviderError> {
        let mut usage = Usage::from_value(&v["usage"], "input_tokens", "output_tokens");
        let mut content = Vec::new();
        if let Some(blocks) = v["content"].as_array() {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => content.push(ContentBlock::Text {
                        text: b["text"].as_str().unwrap_or_default().to_string(),
                    }),
                    // 明文 thinking 才收。redacted_thinking 是密文，不解码、不编一段可见推理。
                    Some("thinking") => {
                        if let Some(t) = b["thinking"].as_str().filter(|s| !s.is_empty()) {
                            content.push(ContentBlock::Thinking {
                                text: t.to_string(),
                            });
                        }
                    }
                    Some("tool_use") => content.push(ContentBlock::ToolUse {
                        id: b["id"].as_str().unwrap_or_default().to_string(),
                        name: b["name"].as_str().unwrap_or_default().to_string(),
                        input: b["input"].clone(),
                    }),
                    // 票 04：服务端工具块透传保史——server_tool_use /
                    // web_search_tool_result 由供应商执行完毕才回传，
                    // 本地无对应工具；不折成 ToolUse 防止二次执行。
                    Some("server_tool_use") | Some("web_search_tool_result") => {
                        // Reliability 13: keep billing uncertainty even if the
                        // consumer cancels after this complete response arrives.
                        usage.unpriced = true;
                        content.push(ContentBlock::Opaque { raw: b.clone() })
                    }
                    _ => {}
                }
            }
        }
        let stop = match v["stop_reason"].as_str() {
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };
        Ok(ChatResponse {
            content,
            stop,
            usage,
        })
    }

    /// content_block 生命周期中的一块：text 累积文本；tool_use 的
    /// partial_json 片段持续拼接，stop 时才解析成 Value。
    enum ABlock {
        Text(String),
        Thinking(String),
        ToolUse {
            id: String,
            name: String,
            args: String,
        },
        /// 票 04：服务端工具块（server_tool_use 的 query 经 input_json_delta
        /// 累积；web_search_tool_result 整块随 start 事件到齐）。
        ServerTool {
            raw: Value,
            args: String,
        },
    }

    /// Anthropic SSE 折叠器（票 02）：content_block_start/delta/stop
    /// 三段式生命周期；input_json_delta 的 partial_json 片段拼接是
    /// 本协议真实复杂度（aisuite convert_stream_event 的 state dict 同款）。
    /// 事件名优先取 SSE `event:` 行，缺省回落 data 里的 type 字段。
    #[derive(Default)]
    pub struct SseFold {
        blocks: Vec<ABlock>,
        stop: Option<String>,
        pub(super) usage: Usage,
    }

    impl SseFold {
        /// 喂一块（event, data）；true = message_stop 流终。
        /// sink 叫停 → Err(Interrupted)；`event: error` → Transport。
        pub fn event(
            &mut self,
            event: &str,
            data: &str,
            sink: &mut StreamSink<'_>,
        ) -> Result<bool, ProviderError> {
            if data.is_empty() {
                return Ok(false);
            }
            let v: Value = serde_json::from_str(data)
                .map_err(|e| ProviderError::Transport(format!("sse json: {e}")))?;
            let kind = if event.is_empty() {
                v["type"].as_str().unwrap_or("")
            } else {
                event
            };
            match kind {
                "message_start" => {
                    self.usage
                        .observe(&v["message"]["usage"], "input_tokens", "output_tokens");
                }
                "content_block_start" => {
                    let b = &v["content_block"];
                    // Reliability 13: a later abort can discard content blocks,
                    // but cannot erase evidence of unpriced native service use.
                    self.usage.unpriced |= matches!(
                        b["type"].as_str(),
                        Some("server_tool_use" | "web_search_tool_result")
                    );
                    self.blocks.push(match b["type"].as_str() {
                        Some("thinking") => {
                            let initial = b["thinking"].as_str().unwrap_or("").to_string();
                            if !initial.is_empty() && !sink(&StreamDelta::Thinking(initial.clone()))
                            {
                                return Err(ProviderError::Interrupted);
                            }
                            ABlock::Thinking(initial)
                        }
                        Some("tool_use") => ABlock::ToolUse {
                            id: b["id"].as_str().unwrap_or_default().into(),
                            name: b["name"].as_str().unwrap_or_default().into(),
                            args: String::new(),
                        },
                        // 票 04：server 工具块整块保史（input 后续 delta 回填）
                        Some("server_tool_use") | Some("web_search_tool_result") => {
                            ABlock::ServerTool {
                                raw: b.clone(),
                                args: String::new(),
                            }
                        }
                        _ => ABlock::Text(String::new()),
                    });
                }
                "content_block_delta" => {
                    let idx = v["index"].as_u64().unwrap_or(0) as usize;
                    let d = &v["delta"];
                    match (self.blocks.get_mut(idx), d["type"].as_str()) {
                        (Some(ABlock::Text(t)), Some("text_delta")) => {
                            if let Some(s) = d["text"].as_str() {
                                if !s.is_empty() {
                                    t.push_str(s);
                                    if !sink(&StreamDelta::Text(s.to_string())) {
                                        return Err(ProviderError::Interrupted);
                                    }
                                }
                            }
                        }
                        // signature_delta 不是推理正文，落到下面的 _ 跳过。
                        (Some(ABlock::Thinking(t)), Some("thinking_delta")) => {
                            if let Some(s) = d["thinking"].as_str() {
                                if !s.is_empty() {
                                    t.push_str(s);
                                    if !sink(&StreamDelta::Thinking(s.to_string())) {
                                        return Err(ProviderError::Interrupted);
                                    }
                                }
                            }
                        }
                        (Some(ABlock::ToolUse { args, .. }), Some("input_json_delta"))
                        | (Some(ABlock::ServerTool { args, .. }), Some("input_json_delta")) => {
                            if let Some(p) = d["partial_json"].as_str() {
                                args.push_str(p);
                            }
                        }
                        _ => {}
                    }
                }
                "message_delta" => {
                    if let Some(s) = v["delta"]["stop_reason"].as_str() {
                        self.stop = Some(s.into());
                    }
                    self.usage
                        .observe(&v["usage"], "input_tokens", "output_tokens");
                }
                "message_stop" => return Ok(true),
                // Anthropic 流内 error 事件（如 overloaded_error）：完整性优先直传
                "error" => {
                    return Err(ProviderError::Transport(format!(
                        "sse error event: {}",
                        v["error"]["message"].as_str().unwrap_or("unknown")
                    )))
                }
                // ping / content_block_stop / message_start 余项 / 未知事件：跳过
                _ => {}
            }
            Ok(false)
        }

        /// 流终折叠：tool_use 的 args 缓冲 parse 成 input（空={}，坏 JSON=Transport）。
        pub fn finish(self) -> Result<ChatResponse, ProviderError> {
            let mut content = Vec::new();
            for b in self.blocks {
                match b {
                    ABlock::Text(t) if !t.is_empty() => {
                        content.push(ContentBlock::Text { text: t })
                    }
                    ABlock::Thinking(t) if !t.is_empty() => {
                        content.push(ContentBlock::Thinking { text: t })
                    }
                    ABlock::ToolUse { id, name, args } => {
                        let input: Value =
                            serde_json::from_str(if args.is_empty() { "{}" } else { &args })
                                .map_err(|e| {
                                    ProviderError::Transport(format!("tool input json: {e}"))
                                })?;
                        content.push(ContentBlock::ToolUse { id, name, input });
                    }
                    // 票 04：server 工具块——delta 累积的 input 回填 raw 再透传
                    ABlock::ServerTool { mut raw, args } => {
                        if !args.is_empty() {
                            if let Ok(input) = serde_json::from_str::<Value>(&args) {
                                raw["input"] = input;
                            }
                        }
                        content.push(ContentBlock::Opaque { raw });
                    }
                    _ => {}
                }
            }
            let stop = match self.stop.as_deref() {
                Some("tool_use") => StopReason::ToolUse,
                Some("max_tokens") => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            };
            Ok(ChatResponse {
                content,
                stop,
                usage: self.usage,
            })
        }
    }
}

/// 供应商类型：决定请求路径、鉴权头与消息形状。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ProviderKind {
    /// Anthropic /v1/messages（x-api-key + anthropic-version）。
    Anthropic,
    /// OpenAI 兼容 /chat/completions（Bearer）——OpenAI/DeepSeek/Moonshot/OpenRouter 等。
    OpenAi,
    /// TypeSafe System One（Bearer，`POST /v1/systemone`）。只做封闭选择，不能当聊天模型。
    /// 项目经理的决策槽用它；没配则同一次选择仍走主对话模型。
    Jev,
}

/// 真实 HTTP 供应商：同步 ureq 叶子调用，key 在每次调用时从 creds 现取。
pub struct HttpProvider {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key_name: String,
    creds: Arc<dyn crate::credentials::CredentialStore>,
    agent: ureq::Agent,
    meta: ModelMeta,
}

impl HttpProvider {
    pub fn new(
        kind: ProviderKind,
        base_url: String,
        model: String,
        key_name: String,
        creds: Arc<dyn crate::credentials::CredentialStore>,
        meta: ModelMeta,
    ) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(180)))
            .build()
            .into();
        Self {
            kind,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            key_name,
            creds,
            agent,
            meta,
        }
    }

    fn apply_openai_output_limit(&self, body: &mut Value) {
        // Reliability 13 / OpenAI Chat Completions contract: max_tokens is
        // incompatible with reasoning models; max_completion_tokens also
        // includes reasoning tokens. Keep max_tokens for legacy-compatible
        // third-party endpoints instead of silently dropping the bound.
        let family = self.model.split('-').next().unwrap_or_default();
        let reasoning = matches!(family, "o1" | "o3" | "o4")
            || self
                .model
                .strip_prefix("gpt-")
                .and_then(|s| s.split(['-', '.']).next())
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|major| major >= 5);
        let key = if reasoning {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        if let Some(max) = self.output_token_limit() {
            body[key] = serde_json::json!(max);
        }
    }

    fn map_err(e: ureq::Error) -> ProviderError {
        match e {
            // 4xx（除 429 限流）是配置/权限问题——不可重试直报
            ureq::Error::StatusCode(c) if (400..500).contains(&c) && c != 429 => {
                ProviderError::Refused(format!("HTTP {c}"))
            }
            other => ProviderError::Transport(other.to_string()),
        }
    }

    fn api_key(&self) -> Result<String, ProviderError> {
        self.creds
            .get(&self.key_name)
            .map_err(|e| ProviderError::Transport(e.to_string()))?
            .ok_or_else(|| ProviderError::MissingCredential(self.key_name.clone()))
    }

    /// 响应是否 SSE 流（回退判据：不流式的兼容端点直接回 JSON 全量）。
    fn is_sse(resp: &ureq::http::Response<ureq::Body>) -> bool {
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|t| t.contains("text/event-stream"))
            .unwrap_or(false)
    }
}

/// 非流式端点回退（票 02）：整段文本一发——delta 通道语义不变，
/// 只少「逐字」节奏（spec：不回退是错的，兼容端点不支持流式）。
fn emit_fallback(
    resp: ChatResponse,
    sink: &mut StreamSink<'_>,
) -> Result<ChatResponse, ProviderError> {
    if !emit_content_deltas(&resp.content, usize::MAX, sink) {
        return Err(ProviderError::Interrupted.with_usage(&resp.usage));
    }
    Ok(resp)
}

impl ModelProvider for HttpProvider {
    fn output_token_limit(&self) -> Option<u64> {
        (self.kind != ProviderKind::Jev).then_some(self.meta.max_output.unwrap_or(8_192).max(1))
    }
    fn billing_model(&self) -> Option<&str> {
        Some(&self.model)
    }
    fn model_meta(&self) -> ModelMeta {
        self.meta.clone()
    }

    /// 票 04：槽 caps 含 `web` 才给 Anthropic 挂 web_search_20250305
    /// （max_uses 有界防搜索循环烧额度）。OpenAI 形状返回空——能力
    /// 缺席是 ADR 0058-1 的预期不对称，不自建爬虫顶替。
    fn server_tools(&self, model_slot: &str) -> Vec<Value> {
        if self.kind != ProviderKind::Anthropic {
            return vec![];
        }
        if !crate::provider_config::caps_for_slot(model_slot).contains("web") {
            return vec![];
        }
        vec![serde_json::json!({
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 5,
        })]
    }

    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let key = self.api_key()?;
        match self.kind {
            ProviderKind::Anthropic => {
                let url = format!("{}/v1/messages", self.base_url);
                // 票 02：max_tokens 收编进模型元数据，8192 只是缺省旧值。
                let body = anthropic_shape::to_request(
                    req,
                    &self.model,
                    self.output_token_limit().unwrap_or(8_192),
                    &self.server_tools(&req.model_slot),
                );
                let mut resp = self
                    .agent
                    .post(&url)
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01")
                    .send_json(&body)
                    .map_err(Self::map_err)?;
                let v: Value = resp.body_mut().read_json().map_err(Self::map_err)?;
                anthropic_shape::from_response(&v)
            }
            ProviderKind::OpenAi => {
                let url = format!("{}/chat/completions", self.base_url);
                let mut body = openai_shape::to_request(req);
                body["model"] = serde_json::json!(self.model);
                // Reliability 13: the same advertised bound must be sent to
                // the provider; relying on its default makes reservations false.
                self.apply_openai_output_limit(&mut body);
                let mut resp = self
                    .agent
                    .post(&url)
                    .header("Authorization", &format!("Bearer {key}"))
                    .send_json(&body)
                    .map_err(Self::map_err)?;
                let v: Value = resp.body_mut().read_json().map_err(Self::map_err)?;
                openai_shape::from_response(&v)
            }
            ProviderKind::Jev => Err(ProviderError::Refused(
                "Jev only answers closed choices, not chat".into(),
            )),
        }
    }

    /// 真流式（票 02）：请求带 stream:true；响应是 event-stream 走
    /// SSE 折叠器逐块发 delta，否则回退整段单 delta（兼容端点不流式）。
    fn stream(
        &self,
        req: &ChatRequest,
        sink: &mut StreamSink<'_>,
    ) -> Result<ChatResponse, ProviderError> {
        let key = self.api_key()?;
        match self.kind {
            ProviderKind::Anthropic => {
                let url = format!("{}/v1/messages", self.base_url);
                // 票 02：max_tokens 收编进模型元数据，8192 只是缺省旧值。
                let mut body = anthropic_shape::to_request(
                    req,
                    &self.model,
                    self.output_token_limit().unwrap_or(8_192),
                    &self.server_tools(&req.model_slot),
                );
                body["stream"] = serde_json::json!(true);
                let mut resp = self
                    .agent
                    .post(&url)
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01")
                    .send_json(&body)
                    .map_err(Self::map_err)?;
                if !Self::is_sse(&resp) {
                    let v: Value = resp.body_mut().read_json().map_err(Self::map_err)?;
                    return emit_fallback(anthropic_shape::from_response(&v)?, sink);
                }
                let mut blocks =
                    SseBlocks::new(std::io::BufReader::new(resp.body_mut().as_reader()));
                let mut fold = anthropic_shape::SseFold::default();
                while let Some((ev, data)) =
                    blocks.next_block().map_err(|e| e.with_usage(&fold.usage))?
                {
                    if fold
                        .event(&ev, &data, sink)
                        .map_err(|e| e.with_usage(&fold.usage))?
                    {
                        break;
                    }
                }
                let usage = fold.usage.clone();
                fold.finish().map_err(|e| e.with_usage(&usage))
            }
            ProviderKind::OpenAi => {
                let url = format!("{}/chat/completions", self.base_url);
                let mut body = openai_shape::to_request(req);
                body["model"] = serde_json::json!(self.model);
                // Reliability 13: streaming uses the same enforced bound as complete().
                self.apply_openai_output_limit(&mut body);
                body["stream"] = serde_json::json!(true);
                // 末块带 usage（OpenAI 系端点通用支持；不识别的端点忽略字段）
                body["stream_options"] = serde_json::json!({"include_usage": true});
                let mut resp = self
                    .agent
                    .post(&url)
                    .header("Authorization", &format!("Bearer {key}"))
                    .send_json(&body)
                    .map_err(Self::map_err)?;
                if !Self::is_sse(&resp) {
                    let v: Value = resp.body_mut().read_json().map_err(Self::map_err)?;
                    return emit_fallback(openai_shape::from_response(&v)?, sink);
                }
                let mut blocks =
                    SseBlocks::new(std::io::BufReader::new(resp.body_mut().as_reader()));
                let mut fold = openai_shape::SseFold::default();
                while let Some((ev, data)) =
                    blocks.next_block().map_err(|e| e.with_usage(&fold.usage))?
                {
                    let _ = ev; // OpenAI 无 event 行，只有 data
                    if fold
                        .data(&data, sink)
                        .map_err(|e| e.with_usage(&fold.usage))?
                    {
                        break;
                    }
                }
                let usage = fold.usage.clone();
                fold.finish().map_err(|e| e.with_usage(&usage))
            }
            ProviderKind::Jev => Err(ProviderError::Refused(
                "Jev only answers closed choices, not chat".into(),
            )),
        }
    }

    fn uses_decision_api(&self) -> bool {
        self.kind == ProviderKind::Jev
    }

    fn decide(&self, state: &str, options: &[(&str, &str)]) -> Result<ChatResponse, ProviderError> {
        if self.kind != ProviderKind::Jev {
            return Err(ProviderError::Refused(
                "decision API is only for Jev".into(),
            ));
        }
        let key = self.api_key()?;
        jev_choice(
            &self.agent,
            &self.base_url,
            &self.model,
            &key,
            state,
            options,
        )
    }
}

/// `https://api.typesafe.ai` 与已经带 `/v1` 的基址都落到 `/v1/systemone`。
pub fn systemone_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/v1/systemone") {
        b.to_string()
    } else if b.ends_with("/v1") {
        format!("{b}/systemone")
    } else {
        format!("{b}/v1/systemone")
    }
}

/// TypeSafe System One 的一道选择题。选项键必须原样回到 `choice`，
/// 调用方再用花名册逐字比对。不走聊天形状。
fn jev_choice(
    agent: &ureq::Agent,
    base: &str,
    model: &str,
    key: &str,
    state: &str,
    options: &[(&str, &str)],
) -> Result<ChatResponse, ProviderError> {
    let mut criteria = serde_json::Map::new();
    for (name, note) in options {
        criteria.insert((*name).to_string(), Value::String((*note).to_string()));
    }
    // 执行判定和派活共用选择题接口。选项里出现「执行」就是判定，不能沿用派活说明。
    let judgment = options.iter().any(|(name, _)| *name == "执行");
    let instructions = if judgment {
        "这是执行判定。证据和提案已经落盘。只选执行、驳回或交给负责人。不要改写提案，不要重算分数。拿不准就交给负责人。"
    } else {
        "下一手派给谁。可以派给当前阶段激活名单以外的人。没有人该接就选「先不派活」。只选一个。"
    };
    let body = serde_json::json!({
        "state": state,
        "model": model,
        "questions": {
            "route": {
                "type": "choice",
                "instructions": instructions,
                "criteria": criteria,
            }
        }
    });
    let url = systemone_url(base);
    let mut resp = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {key}"))
        .send_json(&body)
        .map_err(HttpProvider::map_err)?;
    let v: Value = resp.body_mut().read_json().map_err(HttpProvider::map_err)?;
    let choice = v["answers"]["route"]["choice"]
        .as_str()
        .ok_or_else(|| ProviderError::Transport("jev response missing choice".into()))?
        .to_string();
    let usage = Usage::from_value(&v["usage"], "input_tokens", "output_tokens");
    Ok(ChatResponse {
        content: vec![ContentBlock::Text { text: choice }],
        stop: StopReason::EndTurn,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::CredentialStore;
    use serde_json::json;

    fn text(s: &str) -> ChatResponse {
        ChatResponse {
            content: vec![ContentBlock::Text { text: s.into() }],
            stop: StopReason::EndTurn,
            usage: Usage::default(),
        }
    }

    fn empty_req() -> ChatRequest {
        ChatRequest {
            model_slot: "claude-sonnet".into(),
            messages: vec![],
            tools: vec![],
        }
    }

    #[test]
    fn scripted_provider_replays_in_order_and_records_calls() {
        let p = ScriptedProvider::new(vec![text("第一"), text("第二")]);
        let req = empty_req();
        let r1 = p.complete(&req).unwrap();
        let r2 = p.complete(&req).unwrap();
        assert_eq!(
            r1.content[0],
            ContentBlock::Text {
                text: "第一".into()
            }
        );
        assert_eq!(
            r2.content[0],
            ContentBlock::Text {
                text: "第二".into()
            }
        );
        assert_eq!(p.recorded().len(), 2);
        assert_eq!(p.recorded()[0].model_slot, "claude-sonnet");
    }

    #[test]
    fn exhausted_script_errors() {
        let p = ScriptedProvider::new(vec![]);
        assert!(matches!(
            p.complete(&empty_req()),
            Err(ProviderError::ScriptExhausted)
        ));
    }

    /// 票 01：分块回放——delta 序拼回完整文本，最终响应与 complete 一致。
    #[test]
    fn scripted_stream_chunks_and_matches_complete() {
        let p = ScriptedProvider::chunked(vec![text("你好世界abcde")], 3);
        let mut deltas: Vec<String> = Vec::new();
        let resp = p
            .stream(&empty_req(), &mut |d| {
                let StreamDelta::Text(t) = d else {
                    panic!("unexpected thinking delta");
                };
                deltas.push(t.clone());
                true
            })
            .unwrap();
        // "你好世" / "界ab" / "cde" —— 3 字符一块（多字节不劈开）
        assert_eq!(deltas, vec!["你好世", "界ab", "cde"]);
        assert_eq!(deltas.concat(), "你好世界abcde");
        assert!(matches!(resp.content[0], ContentBlock::Text { .. }));
    }

    /// 票 01：默认实现 = complete + 单段全文 delta（HttpProvider 未覆写前走这条）。
    #[test]
    fn default_stream_emits_whole_text_once() {
        let p = ScriptedProvider::new(vec![text("一整段")]);
        let mut count = 0;
        let mut got = String::new();
        p.stream(&empty_req(), &mut |d| {
            count += 1;
            let StreamDelta::Text(t) = d else {
                panic!("unexpected thinking delta");
            };
            got.push_str(t);
            true
        })
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(got, "一整段");
    }

    #[test]
    fn openai_shape_roundtrips_tool_calls() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "fs_read".into(),
                    input: json!({"path": "a.md"}),
                }],
            }],
            tools: vec![ToolDef {
                name: "fs_read".into(),
                description: "read a file".into(),
                input_schema: json!({"type":"object"}),
            }],
        };
        // 输入不变断言（aisuite 教训：anthropic provider 曾 pop(0) 原地删
        // 调用方 system 消息——converter 吃 &ChatRequest，这条钉死契约）。
        let before = (
            serde_json::to_value(&req.messages).unwrap(),
            serde_json::to_value(&req.tools).unwrap(),
        );
        let out = openai_shape::to_request(&req);
        assert_eq!(
            out["messages"][0]["tool_calls"][0]["function"]["name"],
            "fs_read"
        );
        assert_eq!(out["tools"][0]["function"]["name"], "fs_read");
        assert_eq!(
            before,
            (
                serde_json::to_value(&req.messages).unwrap(),
                serde_json::to_value(&req.tools).unwrap()
            ),
            "converter 不得修改输入"
        );

        let resp = json!({
            "choices": [{"message": {"tool_calls": [{
                "id": "t1", "type": "function",
                "function": {"name": "fs_read", "arguments": "{\"path\":\"a.md\"}"}
            }]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        });
        let r = openai_shape::from_response(&resp).unwrap();
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!(r.usage.prompt_tokens, 10);
        assert!(matches!(&r.content[0], ContentBlock::ToolUse { name, .. } if name == "fs_read"));
    }

    /// Anthropic 形状：system 抽顶、tool_result 归 user、stop_reason/usage 映射。
    #[test]
    fn anthropic_shape_maps_roles_and_tools() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: vec![ContentBlock::Text { text: "sys".into() }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: "hi".into() }],
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: "文件内容".into(),
                        is_error: false,
                        images: vec![],
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: "fs_read".into(),
                description: "read".into(),
                input_schema: json!({"type":"object"}),
            }],
        };
        let before = (
            serde_json::to_value(&req.messages).unwrap(),
            serde_json::to_value(&req.tools).unwrap(),
        );
        let out = anthropic_shape::to_request(&req, "claude-test", 8192, &[]);
        assert_eq!(out["model"], "claude-test");
        assert_eq!(out["max_tokens"], 8192);
        assert_eq!(out["system"], "sys");
        assert_eq!(out["messages"].as_array().unwrap().len(), 2); // system 不进 messages
        assert_eq!(out["messages"][1]["role"], "user");
        assert_eq!(out["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(out["tools"][0]["name"], "fs_read");
        assert_eq!(
            before,
            (
                serde_json::to_value(&req.messages).unwrap(),
                serde_json::to_value(&req.tools).unwrap()
            ),
            "converter 不得修改输入"
        );

        let resp = json!({
            "content": [
                {"type":"text","text":"看一下"},
                {"type":"tool_use","id":"t1","name":"fs_read","input":{"path":"a.md"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 6}
        });
        let r = anthropic_shape::from_response(&resp).unwrap();
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!(r.usage.prompt_tokens, 20);
        assert!(matches!(&r.content[1], ContentBlock::ToolUse { name, .. } if name == "fs_read"));
    }

    /// HttpProvider：无 key → MissingCredential（不发请求）。
    #[test]
    fn http_provider_missing_key_fails_closed() {
        let p = HttpProvider::new(
            ProviderKind::OpenAi,
            "http://127.0.0.1:1".into(),
            "m".into(),
            "model/chat".into(),
            Arc::new(crate::credentials::MemoryStore::default()),
            ModelMeta::default(),
        );
        let err = p.complete(&empty_req()).unwrap_err();
        assert!(matches!(err, ProviderError::MissingCredential(_)));
    }

    // ---------- 票 02：SSE ----------

    /// 预录 OpenAI SSE：文本 delta 序 + tool_calls 片段拼装 + usage 末块。
    #[test]
    fn openai_sse_folds_text_tools_usage() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"世界\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"fs_read\",\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a.md\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":9}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut fold = openai_shape::SseFold::default();
        let mut got: Vec<String> = Vec::new();
        let mut blocks = SseBlocks::new(std::io::BufReader::new(sse.as_bytes()));
        while let Some((_, data)) = blocks.next_block().unwrap() {
            if fold
                .data(&data, &mut |d| {
                    let StreamDelta::Text(t) = d else {
                        panic!("unexpected thinking delta");
                    };
                    got.push(t.clone());
                    true
                })
                .unwrap()
            {
                break;
            }
        }
        let resp = fold.finish().unwrap();
        assert_eq!(got, vec!["你好", "世界"]);
        assert_eq!(resp.stop, StopReason::ToolUse);
        assert_eq!(resp.usage.prompt_tokens, 11);
        match &resp.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "fs_read");
                assert_eq!(input["path"], "a.md");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    /// 预录 Anthropic SSE：text_delta + input_json_delta 分片拼装 +
    /// stop_reason/usage 从 message_delta 来。
    #[test]
    fn anthropic_sse_folds_partial_json() {
        let sse = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":21}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"先读\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t9\",\"name\":\"fs_read\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"b.md\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":14}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let mut fold = anthropic_shape::SseFold::default();
        let mut got: Vec<String> = Vec::new();
        let mut blocks = SseBlocks::new(std::io::BufReader::new(sse.as_bytes()));
        while let Some((ev, data)) = blocks.next_block().unwrap() {
            if fold
                .event(&ev, &data, &mut |d| {
                    let StreamDelta::Text(t) = d else {
                        panic!("unexpected thinking delta");
                    };
                    got.push(t.clone());
                    true
                })
                .unwrap()
            {
                break;
            }
        }
        let resp = fold.finish().unwrap();
        assert_eq!(got, vec!["先读"]);
        assert_eq!(resp.stop, StopReason::ToolUse);
        assert_eq!(
            (resp.usage.prompt_tokens, resp.usage.completion_tokens),
            (21, 14)
        );
        match &resp.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!((id.as_str(), name.as_str()), ("t9", "fs_read"));
                assert_eq!(input["path"], "b.md");
            }
            other => panic!("expected tool_use, got {other:?}"),
        }
    }

    /// hands-free 票 06：OpenAI 明文 reasoning_content 进思考增量；
    /// reasoning 为对象时不编一段文本。
    #[test]
    fn openai_sse_folds_reasoning_text_only() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先想\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning\":{\"summary\":\"不要收\"}}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"再答\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut fold = openai_shape::SseFold::default();
        let mut thinking = Vec::new();
        let mut text = Vec::new();
        let mut blocks = SseBlocks::new(std::io::BufReader::new(sse.as_bytes()));
        while let Some((_, data)) = blocks.next_block().unwrap() {
            if fold
                .data(&data, &mut |d| {
                    match d {
                        StreamDelta::Thinking(t) => thinking.push(t.clone()),
                        StreamDelta::Text(t) => text.push(t.clone()),
                    }
                    true
                })
                .unwrap()
            {
                break;
            }
        }
        assert_eq!(thinking, vec!["先想"]);
        assert_eq!(text, vec!["再答"]);
        let resp = fold.finish().unwrap();
        assert!(matches!(&resp.content[0], ContentBlock::Thinking { text } if text == "先想"));
        assert!(matches!(&resp.content[1], ContentBlock::Text { text } if text == "再答"));
    }

    /// hands-free 票 06：Anthropic thinking_delta 进思考；redacted_thinking 无明文，不编造。
    #[test]
    fn anthropic_sse_folds_thinking_ignores_redacted() {
        let sse = concat!(
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"因为\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"cipher\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"答\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let mut fold = anthropic_shape::SseFold::default();
        let mut thinking = Vec::new();
        let mut text = Vec::new();
        let mut blocks = SseBlocks::new(std::io::BufReader::new(sse.as_bytes()));
        while let Some((ev, data)) = blocks.next_block().unwrap() {
            if fold
                .event(&ev, &data, &mut |d| {
                    match d {
                        StreamDelta::Thinking(t) => thinking.push(t.clone()),
                        StreamDelta::Text(t) => text.push(t.clone()),
                    }
                    true
                })
                .unwrap()
            {
                break;
            }
        }
        assert_eq!(thinking, vec!["因为"]);
        assert_eq!(text, vec!["答"]);
        let resp = fold.finish().unwrap();
        assert_eq!(resp.content.len(), 2, "密文思考不得变成一块可见推理");
        assert!(matches!(&resp.content[0], ContentBlock::Thinking { text } if text == "因为"));
        assert!(matches!(&resp.content[1], ContentBlock::Text { text } if text == "答"));
        let dumped = format!("{resp:?}");
        assert!(!dumped.contains("cipher"));
    }

    /// 畸形行/注释/心跳跳过不崩；sink 叫停 → Interrupted。
    #[test]
    fn sse_skips_noise_and_aborts_on_sink() {
        let sse = concat!(
            ": heartbeat\n",
            "garbage line without colon\n",
            "retry: 3000\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n",
        );
        let mut fold = openai_shape::SseFold::default();
        let mut n = 0;
        let mut blocks = SseBlocks::new(std::io::BufReader::new(sse.as_bytes()));
        let mut aborted = false;
        while let Some((_, data)) = blocks.next_block().unwrap() {
            match fold.data(&data, &mut |_| {
                n += 1;
                n < 2 // 第二个 delta 叫停
            }) {
                Err(ProviderError::Interrupted) => {
                    aborted = true;
                    break;
                }
                other => {
                    other.unwrap();
                }
            }
        }
        assert!(aborted);
        assert_eq!(n, 2);
    }

    #[test]
    fn request_budget_native_tool_billing_stays_unknown_after_stream_error() {
        let body = concat!(
            "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":1000,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"s1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
            "event: message_delta\ndata: {\"usage\":{\"output_tokens\":25}}\n\n",
            "event: error\ndata: {\"error\":{\"type\":\"overloaded_error\"}}\n\n",
        );
        let base = serve_once(200, "text/event-stream", body);
        let provider = http_provider(ProviderKind::Anthropic, &base);
        let error = provider.stream(&empty_req(), &mut |_| true).unwrap_err();
        assert!(matches!(error.cause(), ProviderError::Transport(_)));
        let usage = error
            .usage()
            .expect("received usage survives stream errors");
        assert_eq!(usage.prompt_tokens, 1000);
        assert_eq!(usage.completion_tokens, 25);
        assert!(
            usage.unpriced,
            "two-rate table cannot price native web search"
        );
    }

    #[test]
    fn request_budget_output_limit_reaches_each_http_transport() {
        for (kind, model, key) in [
            (ProviderKind::Anthropic, "claude-test", "max_tokens"),
            (ProviderKind::OpenAi, "legacy-compatible", "max_tokens"),
            (ProviderKind::OpenAi, "o3", "max_completion_tokens"),
            (ProviderKind::OpenAi, "gpt-5", "max_completion_tokens"),
        ] {
            for streaming in [false, true] {
                for limit in [None, Some(1234)] {
                    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                    let base = format!("http://{}", listener.local_addr().unwrap());
                    let peer = std::thread::spawn(move || {
                        use std::io::{BufRead, Read, Write};
                        let (mut socket, _) = listener.accept().unwrap();
                        socket
                            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                            .unwrap();
                        let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
                        let mut size = 0;
                        loop {
                            let mut line = String::new();
                            reader.read_line(&mut line).unwrap();
                            if let Some(value) =
                                line.to_ascii_lowercase().strip_prefix("content-length:")
                            {
                                size = value.trim().parse::<usize>().unwrap();
                            }
                            if line.trim().is_empty() {
                                break;
                            }
                        }
                        let mut bytes = vec![0; size];
                        reader.read_exact(&mut bytes).unwrap();
                        let body: Value = serde_json::from_slice(&bytes).unwrap();
                        let response = r#"{"content":[],"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"stop_reason":"end_turn"}"#;
                        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                        body
                    });
                    let mut provider = http_provider_with_meta(
                        kind.clone(),
                        &base,
                        ModelMeta {
                            context_window: None,
                            max_output: limit,
                        },
                    );
                    provider.model = model.into();
                    if streaming {
                        provider.stream(&empty_req(), &mut |_| true).unwrap();
                    } else {
                        provider.complete(&empty_req()).unwrap();
                    }
                    let sent = peer.join().unwrap();
                    assert_eq!(
                        sent[key],
                        limit.unwrap_or(8192),
                        "{model} stream={streaming}"
                    );
                    if key == "max_completion_tokens" {
                        assert!(sent.get("max_tokens").is_none());
                    }
                }
            }
        }
    }

    /// 一次性 canned HTTP 端点：读完整请求后回固定响应，返回 base_url。
    fn serve_once(status: u16, content_type: &'static str, body: &'static str) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{BufRead, Read, Write};
            let (mut s, _) = l.accept().unwrap();
            let mut br = std::io::BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            let mut clen = 0usize;
            loop {
                line.clear();
                br.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    clen = v.trim().parse().unwrap_or(0);
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            let mut req = vec![0u8; clen];
            br.read_exact(&mut req).unwrap();
            let head = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(body.as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    #[test]
    fn systemone_url_accepts_bare_and_v1_bases() {
        assert_eq!(
            systemone_url("https://api.typesafe.ai"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert_eq!(
            systemone_url("https://api.typesafe.ai/v1/"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert_eq!(
            systemone_url("https://api.typesafe.ai/v1/systemone"),
            "https://api.typesafe.ai/v1/systemone"
        );
    }

    #[test]
    fn jev_decide_posts_choice_and_returns_the_option_verbatim() {
        let base = serve_once(
            200,
            "application/json",
            r#"{"model":"jev-1.13.0","answers":{"route":{"type":"choice","choice":"后端"}},"usage":{"input_tokens":4,"output_tokens":1}}"#,
        );
        let p = http_provider(ProviderKind::Jev, &base);
        let resp = p
            .decide(
                "负责人说：开始执行",
                &[("后端", "花名册角色"), ("先不派活", "先不唤醒任何人")],
            )
            .unwrap();
        assert_eq!(choice_text_of(&resp), "后端");
        assert_eq!(resp.usage.prompt_tokens, 4);
    }

    fn choice_text_of(resp: &ChatResponse) -> String {
        resp.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn http_provider(kind: ProviderKind, base: &str) -> HttpProvider {
        http_provider_with_meta(kind, base, ModelMeta::default())
    }

    fn http_provider_with_meta(kind: ProviderKind, base: &str, meta: ModelMeta) -> HttpProvider {
        let creds = Arc::new(crate::credentials::MemoryStore::default());
        creds.set("k", "test-key").unwrap();
        HttpProvider::new(kind, base.into(), "m".into(), "k".into(), creds, meta)
    }

    /// HttpProvider::stream 全链路：真 HTTP + SSE 响应 → 增量 delta + 终值响应。
    #[test]
    fn http_stream_reads_sse_end_to_end() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"一\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"二\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        );
        let base = serve_once(200, "text/event-stream", sse);
        let p = http_provider(ProviderKind::OpenAi, &base);
        let mut got: Vec<String> = Vec::new();
        let resp = p
            .stream(&empty_req(), &mut |d| {
                let StreamDelta::Text(t) = d else {
                    panic!("unexpected thinking delta");
                };
                got.push(t.clone());
                true
            })
            .unwrap();
        assert_eq!(got, vec!["一", "二"]);
        assert_eq!(resp.usage.completion_tokens, 2);
        assert_eq!(resp.stop, StopReason::EndTurn);
    }

    // ---------- 票 02：图片内容块两形状 ----------

    fn img_result() -> ContentBlock {
        ContentBlock::ToolResult {
            tool_use_id: "tu1".into(),
            content: "{\"path\":\"a.png\"}".into(),
            is_error: false,
            images: vec![ImageData {
                media_type: "image/png".into(),
                data: "aGk=".into(),
            }],
        }
    }

    #[test]
    fn openai_tool_result_images_become_user_message() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![Message {
                role: Role::Tool,
                content: vec![img_result()],
            }],
            tools: vec![],
        };
        let out = openai_shape::to_request(&req);
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "tool msg + 补图 user msg");
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "tu1");
        assert_eq!(msgs[1]["role"], "user");
        let part = &msgs[1]["content"][1];
        assert_eq!(part["type"], "image_url");
        assert_eq!(part["image_url"]["url"], "data:image/png;base64,aGk=");
    }

    #[test]
    fn anthropic_tool_result_keeps_images_inline() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![Message {
                role: Role::Tool,
                content: vec![img_result()],
            }],
            tools: vec![],
        };
        let out = anthropic_shape::to_request(&req, "claude", 8192, &[]);
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1, "Anthropic 不拆消息");
        let tr = &msgs[0]["content"][0];
        assert_eq!(tr["type"], "tool_result");
        assert_eq!(tr["content"][1]["type"], "image");
        assert_eq!(tr["content"][1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn inline_image_maps_both_shapes() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![Message {
                role: Role::User,
                content: vec![
                    ContentBlock::Text {
                        text: "看图".into(),
                    },
                    ContentBlock::Image {
                        media_type: "image/png".into(),
                        data: "aGk=".into(),
                    },
                ],
            }],
            tools: vec![],
        };
        let o = openai_shape::to_request(&req);
        assert_eq!(o["messages"][0]["content"][1]["type"], "image_url");
        let a = anthropic_shape::to_request(&req, "claude", 8192, &[]);
        assert_eq!(a["messages"][0]["content"][1]["type"], "image");
        // serde 往返
        let blk = ContentBlock::Image {
            media_type: "image/png".into(),
            data: "x".into(),
        };
        let rt: ContentBlock = serde_json::from_str(&serde_json::to_string(&blk).unwrap()).unwrap();
        assert_eq!(rt, blk);
    }

    /// 非流式端点回退：响应是 JSON 非 event-stream → 单 delta 全量。
    #[test]
    fn http_stream_falls_back_on_plain_json() {
        let body = "{\"choices\":[{\"message\":{\"content\":\"整段\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}";
        let base = serve_once(200, "application/json", body);
        let p = http_provider(ProviderKind::OpenAi, &base);
        let mut got: Vec<String> = Vec::new();
        let resp = p
            .stream(&empty_req(), &mut |d| {
                let StreamDelta::Text(t) = d else {
                    panic!("unexpected thinking delta");
                };
                got.push(t.clone());
                true
            })
            .unwrap();
        assert_eq!(got, vec!["整段"]);
        assert_eq!(resp.stop, StopReason::EndTurn);
    }
}

#[cfg(test)]
mod server_tool_tests {
    use super::*;

    fn anthropic_provider() -> HttpProvider {
        HttpProvider::new(
            ProviderKind::Anthropic,
            "http://unused".into(),
            "claude-sonnet-4".into(),
            "k".into(),
            std::sync::Arc::new(crate::credentials::MemoryStore::default()),
            ModelMeta::default(),
        )
    }

    #[test]
    fn infer_caps_marks_claude_web_not_openai() {
        assert!(crate::provider_config::infer_caps("claude-sonnet-4").contains(&"web".to_string()));
        assert!(!crate::provider_config::infer_caps("gpt-4o").contains(&"web".to_string()));
    }

    #[test]
    fn server_tools_gated_by_slot_caps() {
        // 进程级环境变量互斥：provider_config 测试也碰 HEXAGON_PROVIDERS_PATH，
        // 并行会互踩（2026-09 曾打出 defs.len()=0 的偶发红）。
        let _env = crate::provider_config::PROVIDERS_ENV_LOCK.lock().unwrap();
        let p = anthropic_provider();
        // 无 providers.json → caps 空 → 不挂 server tool（fail-closed）
        assert!(p.server_tools("default").is_empty());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(
            "HEXAGON_PROVIDERS_PATH",
            dir.path().join("providers.json").to_str().unwrap(),
        );
        // 绑一个 claude 模型 → infer_caps 给 web
        let doc = crate::provider_config::ProviderDoc {
            providers: vec![crate::provider_config::ProviderDef {
                id: "anth".into(),
                name: "anth".into(),
                kind: ProviderKind::Anthropic,
                base_url: "http://x".into(),
                models: vec![],
                enabled: true,
            }],
            slots: [(
                "default".to_string(),
                crate::provider_config::SlotBinding {
                    provider_id: "anth".into(),
                    model: "claude-sonnet-4".into(),
                },
            )]
            .into_iter()
            .collect(),
            search: None,
        };
        std::fs::write(
            dir.path().join("providers.json"),
            serde_json::to_string(&doc).unwrap(),
        )
        .unwrap();
        let defs = p.server_tools("default");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["type"], "web_search_20250305");
        assert_eq!(defs[0]["name"], "web_search");
        assert!(defs[0]["max_uses"].as_u64().unwrap() <= 10);
        // OpenAI 形状恒缺席（ADR 0058-1 不对称）
        let openai = HttpProvider::new(
            ProviderKind::OpenAi,
            "http://unused".into(),
            "gpt-4o".into(),
            "k".into(),
            std::sync::Arc::new(crate::credentials::MemoryStore::default()),
            ModelMeta::default(),
        );
        assert!(openai.server_tools("default").is_empty());
        std::env::remove_var("HEXAGON_PROVIDERS_PATH");
    }

    #[test]
    fn anthropic_request_merges_server_tools() {
        let req = ChatRequest {
            model_slot: "default".into(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "hi".into() }],
            }],
            tools: vec![ToolDef {
                name: "fs_read".into(),
                description: "d".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }],
        };
        let st = vec![
            serde_json::json!({"type":"web_search_20250305","name":"web_search","max_uses":5}),
        ];
        let out = anthropic_shape::to_request(&req, "claude", 8192, &st);
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[1]["type"], "web_search_20250305");
        // 无 server tools 时数组形态不变
        let out2 = anthropic_shape::to_request(&req, "claude", 8192, &[]);
        assert_eq!(out2["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn response_folds_server_blocks_to_opaque() {
        let resp = serde_json::json!({
            "content": [
                {"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"rust 1.9"}},
                {"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[{"type":"web_search_result","title":"R","url":"https://x"}]},
                {"type":"text","text":"found it"}
            ],
            "stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":2}
        });
        let r = anthropic_shape::from_response(&resp).unwrap();
        assert_eq!(r.content.len(), 3);
        assert!(
            matches!(&r.content[0], ContentBlock::Opaque { raw } if raw["type"] == "server_tool_use")
        );
        assert!(
            matches!(&r.content[1], ContentBlock::Opaque { raw } if raw["type"] == "web_search_tool_result")
        );
        assert!(matches!(&r.content[2], ContentBlock::Text { text } if text == "found it"));
        assert_eq!(r.usage.completion_tokens, 2);
    }

    #[test]
    fn sse_folds_server_blocks() {
        let mut f = anthropic_shape::SseFold::default();
        let mut sink = |_d: &StreamDelta| true;
        f.event("content_block_start", r#"{"index":0,"content_block":{"type":"server_tool_use","id":"s1","name":"web_search","input":{}}}"#, &mut sink).unwrap();
        f.event("content_block_delta", r#"{"index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"rust\"}"}}"#, &mut sink).unwrap();
        f.event("content_block_start", r#"{"index":1,"content_block":{"type":"web_search_tool_result","tool_use_id":"s1","content":[]}}"#, &mut sink).unwrap();
        f.event("message_stop", "{}", &mut sink).unwrap();
        let r = f.finish().unwrap();
        assert_eq!(r.content.len(), 2);
        if let ContentBlock::Opaque { raw } = &r.content[0] {
            assert_eq!(raw["input"]["query"], "rust", "delta 累积的 input 应回填");
        } else {
            panic!()
        }
    }

    #[test]
    fn openai_shape_serializes_opaque_as_text() {
        let req = ChatRequest {
            model_slot: "m".into(),
            messages: vec![Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Opaque {
                    raw: serde_json::json!({"type":"server_tool_use","name":"web_search"}),
                }],
            }],
            tools: vec![],
        };
        let out = openai_shape::to_request(&req);
        assert_eq!(
            out["messages"][0]["content"][0]["text"],
            "[server_tool_use]"
        );
    }

    /// 手写 web_search 调用不撞供应商原生通道。行为变更说明（code-search
    /// 票 03）：web_search 起是真实本地工具，防碰撞的实现从「不进注册表」
    /// 改为「web 能力槽在场时 defs 不列出 + exec 拒绝」——断言跟着改：
    /// 无 web caps 的 ctx 照常列出（本地槽是合法路径），web caps 的 ctx
    /// 里清单与执行双不见。
    #[test]
    fn handwritten_web_search_is_unknown_tool() {
        let reg = crate::tools::Registry::builtin();
        assert!(reg.get("web_search").is_some(), "本地搜索槽在册");
        let plain = crate::tools::ToolContext::default();
        assert!(reg
            .defs_for_ctx(Some(&plain))
            .iter()
            .any(|d| d.name == "web_search"));
        let native = crate::tools::ToolContext {
            caps: ["web".to_string()].into_iter().collect(),
            ..Default::default()
        };
        assert!(!reg
            .defs_for_ctx(Some(&native))
            .iter()
            .any(|d| d.name == "web_search"));
        let err = crate::tools::Tool::exec(
            &crate::websearch::WebSearch,
            &crate::db::Db::open_in_memory().unwrap(),
            &serde_json::json!({"query":"x"}),
            &native,
        )
        .unwrap_err();
        assert!(err.to_string().contains("native web search"));
    }
    proptest::proptest! {
        #[test]
        fn unmodeled_billing_dimensions_never_look_fully_priced(count in 1u64..1_000_000) {
            let mut usage = Usage::from_value(&serde_json::json!({"input_tokens":10,"cache_creation_input_tokens":count}), "input_tokens", "output_tokens");
            usage.observe(&serde_json::json!({"output_tokens":5}), "input_tokens", "output_tokens");
            proptest::prop_assert!(usage.unpriced);
            proptest::prop_assert_eq!(usage.prompt_tokens,10);
            proptest::prop_assert_eq!(usage.completion_tokens,5);
            let standard = Usage::from_value(&serde_json::json!({"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"service_tier":"standard"}), "input_tokens", "output_tokens");
            proptest::prop_assert!(!standard.unpriced);
        }
    }
}
