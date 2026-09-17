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
use std::sync::Mutex;

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
}
