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
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub stop: StopReason,
    pub usage: Usage,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("scripted provider: script exhausted")]
    ScriptExhausted,
    #[error("transport: {0}")]
    Transport(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("missing credential: {0}")]
    MissingCredential(String),
}

pub trait ModelProvider: Send + Sync {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError>;
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
}

impl ScriptedProvider {
    pub fn new(script: Vec<ChatResponse>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            calls: Mutex::new(Vec::new()),
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
}

/// OpenAI 兼容形状的请求/响应映射（纯函数，不碰网络——传输层归具体供应商实现）。
pub mod openai_shape {
    use super::*;

    pub fn to_request(req: &ChatRequest) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = serde_json::to_value(&m.role).unwrap();
                let mut content_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for b in &m.content {
                    match b {
                        ContentBlock::Text { text } => {
                            content_parts.push(serde_json::json!({"type":"text","text":text}))
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
                            ..
                        } => {
                            return serde_json::json!({
                                "role": "tool", "tool_call_id": tool_use_id, "content": content
                            });
                        }
                    }
                }
                let mut msg = serde_json::json!({"role": role});
                if !content_parts.is_empty() {
                    msg["content"] = serde_json::json!(content_parts);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::json!(tool_calls);
                }
                msg
            })
            .collect();
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
            usage: Usage {
                prompt_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                completion_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            },
        })
    }
}

/// Anthropic Messages 形状的请求/响应映射（纯函数，同 openai_shape 的接缝）。
pub mod anthropic_shape {
    use super::*;

    /// system 消息抽顶字段；ToolResult 归 user 消息；其余角色/块近直译。
    pub fn to_request(req: &ChatRequest, model: &str, max_tokens: u64) -> Value {
        let mut system_parts = Vec::new();
        let mut messages = Vec::new();
        for m in &req.messages {
            let blocks: Vec<Value> = m
                .content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => {
                        serde_json::json!({"type":"text","text":text})
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        serde_json::json!({"type":"tool_use","id":id,"name":name,"input":input})
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        serde_json::json!({
                            "type":"tool_result","tool_use_id":tool_use_id,
                            "content":[{"type":"text","text":content}],
                            "is_error":is_error,
                        })
                    }
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
        let mut content = Vec::new();
        if let Some(blocks) = v["content"].as_array() {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => content.push(ContentBlock::Text {
                        text: b["text"].as_str().unwrap_or_default().to_string(),
                    }),
                    Some("tool_use") => content.push(ContentBlock::ToolUse {
                        id: b["id"].as_str().unwrap_or_default().to_string(),
                        name: b["name"].as_str().unwrap_or_default().to_string(),
                        input: b["input"].clone(),
                    }),
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
            usage: Usage {
                prompt_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
                completion_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
            },
        })
    }
}

/// 供应商类型：决定请求路径、鉴权头与消息形状。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Anthropic /v1/messages（x-api-key + anthropic-version）。
    Anthropic,
    /// OpenAI 兼容 /chat/completions（Bearer）——OpenAI/DeepSeek/Moonshot/OpenRouter 等。
    OpenAi,
}

/// 真实 HTTP 供应商：同步 ureq 叶子调用，key 在每次调用时从 creds 现取。
pub struct HttpProvider {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key_name: String,
    creds: Arc<dyn crate::credentials::CredentialStore>,
    agent: ureq::Agent,
}

impl HttpProvider {
    pub fn new(
        kind: ProviderKind,
        base_url: String,
        model: String,
        key_name: String,
        creds: Arc<dyn crate::credentials::CredentialStore>,
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
}

impl ModelProvider for HttpProvider {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let key = self
            .creds
            .get(&self.key_name)
            .map_err(|e| ProviderError::Transport(e.to_string()))?
            .ok_or_else(|| ProviderError::MissingCredential(self.key_name.clone()))?;
        match self.kind {
            ProviderKind::Anthropic => {
                let url = format!("{}/v1/messages", self.base_url);
                let body = anthropic_shape::to_request(req, &self.model, 8192);
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
                let mut resp = self
                    .agent
                    .post(&url)
                    .header("Authorization", &format!("Bearer {key}"))
                    .send_json(&body)
                    .map_err(Self::map_err)?;
                let v: Value = resp.body_mut().read_json().map_err(Self::map_err)?;
                openai_shape::from_response(&v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let out = openai_shape::to_request(&req);
        assert_eq!(
            out["messages"][0]["tool_calls"][0]["function"]["name"],
            "fs_read"
        );
        assert_eq!(out["tools"][0]["function"]["name"], "fs_read");

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
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: "fs_read".into(),
                description: "read".into(),
                input_schema: json!({"type":"object"}),
            }],
        };
        let out = anthropic_shape::to_request(&req, "claude-test", 8192);
        assert_eq!(out["model"], "claude-test");
        assert_eq!(out["max_tokens"], 8192);
        assert_eq!(out["system"], "sys");
        assert_eq!(out["messages"].as_array().unwrap().len(), 2); // system 不进 messages
        assert_eq!(out["messages"][1]["role"], "user");
        assert_eq!(out["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(out["tools"][0]["name"], "fs_read");

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
        );
        let err = p.complete(&empty_req()).unwrap_err();
        assert!(matches!(err, ProviderError::MissingCredential(_)));
    }
}
