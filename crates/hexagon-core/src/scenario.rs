//! 场景 DSL：「假供应商脚本 + 用户动作序列 + 事件断言」的声明式场景。
//!
//! 票 28 验收套件的载体。场景是可 JSON 反序列化的数据；断言直接消费轨迹事件。
//!
//! ```json
//! {
//!   "roles": ["产品策划", "后端"],
//!   "pack": { ... PackDef ... },
//!   "scripts": {"default": [{"text": "..."}, {"tool_calls": [{"name":"fs_write","input":{...}}]}]},
//!   "steps": [
//!     {"do": "open_stage", "seq": 0},
//!     {"do": "run_all_active"},
//!     {"do": "assert_event", "kind": "artifact_delivered"},
//!     {"do": "assert_stage", "seq": 0, "state": "done"}
//!   ]
//! }
//! ```

use crate::api::{ApiError, Workbench};
use crate::orchestra::PackDef;
use crate::provider::{ChatResponse, ContentBlock, ScriptedProvider, StopReason};
use crate::trace::EventKind;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub roles: Vec<String>,
    pub pack: Option<PackDef>,
    /// 模型槽 → 脚本。缺省槽名 "default"。
    #[serde(default)]
    pub scripts: HashMap<String, Vec<ScriptedStep>>,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ScriptedStep {
    Text { text: String },
    Calls { tool_calls: Vec<CallSpec> },
    Resp { response: Box<RespSpec> },
}

#[derive(Debug, Deserialize)]
pub struct CallSpec {
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Deserialize)]
pub struct RespSpec {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<CallSpec>,
    #[serde(default)]
    pub stop: Option<String>,
}

impl ScriptedStep {
    fn to_response(&self, i: usize) -> ChatResponse {
        match self {
            Self::Text { text } => ChatResponse {
                content: vec![ContentBlock::Text { text: text.clone() }],
                stop: StopReason::EndTurn,
                usage: Default::default(),
            },
            Self::Calls { tool_calls } => ChatResponse {
                content: tool_calls
                    .iter()
                    .enumerate()
                    .map(|(j, c)| ContentBlock::ToolUse {
                        id: format!("s{i}_{j}"),
                        name: c.name.clone(),
                        input: c.input.clone(),
                    })
                    .collect(),
                stop: StopReason::ToolUse,
                usage: Default::default(),
            },
            Self::Resp { response } => {
                let mut content = Vec::new();
                if let Some(t) = &response.text {
                    content.push(ContentBlock::Text { text: t.clone() });
                }
                for (j, c) in response.tool_calls.iter().enumerate() {
                    content.push(ContentBlock::ToolUse {
                        id: format!("s{i}_{j}"),
                        name: c.name.clone(),
                        input: c.input.clone(),
                    });
                }
                ChatResponse {
                    content,
                    stop: match response.stop.as_deref() {
                        Some("tool_use") => StopReason::ToolUse,
                        Some("max_tokens") => StopReason::MaxTokens,
                        _ => {
                            if response.tool_calls.is_empty() {
                                StopReason::EndTurn
                            } else {
                                StopReason::ToolUse
                            }
                        }
                    },
                    usage: Default::default(),
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum StepDef {
    OpenStage {
        seq: usize,
    },
    RunTurn {
        role: String,
        input: Option<String>,
    },
    RunAllActive {
        input: Option<String>,
    },
    OwnerMessage {
        text: String,
    },
    Advance,
    RunChecks,
    Stamp,
    Rewind {
        to_seq: usize,
    },
    Skip,
    Pause,
    Resume,
    SleepAll,
    /// 裁决队列里第一个 kind=permission 的待决问题
    AnswerPermission {
        allow: bool,
        remember: Option<String>,
        scope: Option<String>,
    },
    // ---- 断言 ----
    AssertEvent {
        kind: EventKind,
        contains: Option<Value>,
    },
    AssertNoEvent {
        kind: EventKind,
    },
    AssertStage {
        seq: i64,
        state: String,
    },
    AssertArtifact {
        path: String,
        kind: Option<String>,
        status: Option<String>,
    },
    AssertAgentStatus {
        role: String,
        status: String,
    },
}

#[derive(Debug)]
pub struct ScenarioReport {
    pub events_len: usize,
}

pub fn run_scenario(dir: &std::path::Path, sc: &Scenario) -> Result<ScenarioReport, ApiError> {
    let roles: Vec<&str> = sc.roles.iter().map(|s| s.as_str()).collect();
    let mut wb = Workbench::for_test(dir, &roles, sc.pack.clone())?;
    for (slot, steps) in &sc.scripts {
        let resps: Vec<ChatResponse> = steps
            .iter()
            .enumerate()
            .map(|(i, s)| s.to_response(i))
            .collect();
        wb.register_provider(slot, Arc::new(ScriptedProvider::new(resps)));
    }
    for (i, step) in sc.steps.iter().enumerate() {
        run_step(&wb, step).map_err(|e| {
            eprintln!("step {i} failed: {step:?} → {e}");
            e
        })?;
    }
    Ok(ScenarioReport {
        events_len: wb.events(None)?.len(),
    })
}

fn run_step(wb: &Workbench, step: &StepDef) -> Result<(), ApiError> {
    match step {
        StepDef::OpenStage { seq } => {
            wb.open_stage(*seq)?;
        }
        StepDef::RunTurn { role, input } => {
            wb.run_turn(role, input.as_deref().unwrap_or(""))?;
        }
        StepDef::RunAllActive { input } => {
            wb.run_all_active(input.as_deref().unwrap_or(""))?;
        }
        StepDef::OwnerMessage { text } => {
            wb.send_message(text)?;
        }
        StepDef::Advance => {
            wb.advance()?;
        }
        StepDef::RunChecks => {
            wb.run_checks()?;
        }
        StepDef::Stamp => {
            wb.stamp()?;
        }
        StepDef::Rewind { to_seq } => {
            wb.rewind(*to_seq)?;
        }
        StepDef::Skip => {
            wb.skip()?;
        }
        StepDef::Pause => wb.pause()?,
        StepDef::Resume => wb.resume()?,
        StepDef::SleepAll => wb.sleep_all()?,
        StepDef::AnswerPermission {
            allow,
            remember,
            scope,
        } => {
            let qid = wb
                .first_pending_question("permission")?
                .ok_or_else(|| ApiError::NoRole("no queued permission question".into()))?;
            wb.answer_permission(
                &qid,
                *allow,
                remember.as_deref(),
                scope.as_deref().unwrap_or("activation"),
            )?;
        }
        StepDef::AssertEvent { kind, contains } => {
            let evs = wb.events(Some(&[*kind]))?;
            let hit = match contains {
                None => !evs.is_empty(),
                Some(sub) => evs.iter().any(|e| json_contains(&e.payload, sub)),
            };
            assert!(hit, "expected event {kind:?} matching {contains:?}");
        }
        StepDef::AssertNoEvent { kind } => {
            let evs = wb.events(Some(&[*kind]))?;
            assert!(evs.is_empty(), "unexpected event {kind:?} present");
        }
        StepDef::AssertStage { seq, state } => {
            let stages = wb.stage_status()?;
            let hit = stages
                .iter()
                .any(|s| s["seq"] == *seq && s["state"] == *state);
            assert!(
                hit,
                "expected stage seq={seq} state={state}, got {stages:?}"
            );
        }
        StepDef::AssertArtifact { path, kind, status } => {
            let arts = wb.artifacts()?;
            let hit = arts.iter().any(|a| {
                a["path"] == *path
                    && kind.as_ref().is_none_or(|k| a["kind"] == *k)
                    && status.as_ref().is_none_or(|s| a["status"] == *s)
            });
            assert!(
                hit,
                "expected artifact {path} kind={kind:?} status={status:?}"
            );
        }
        StepDef::AssertAgentStatus { role, status } => {
            let team = wb.team()?;
            let hit = team
                .iter()
                .any(|m| m["role"] == *role && m["status"] == *status);
            assert!(hit, "expected agent {role} status={status}");
        }
    }
    Ok(())
}

/// JSON 子集匹配：sub 的每个键值都要在 sup 里（递归到对象）。
fn json_contains(sup: &Value, sub: &Value) -> bool {
    match (sup, sub) {
        (Value::Object(a), Value::Object(b)) => b
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|av| json_contains(av, v))),
        _ => sup == sub,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_deliver_and_assert() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":["规格"]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"artifact_write","input":{
                    "path":"specs/prd.md",
                    "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}}]},
                {"text": "done"}
            ]},
            "steps": [
                {"do":"owner_message","text":"开工 @产品策划"},
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"assert_event","kind":"artifact_delivered","contains":{"path":"specs/prd.md"}},
                {"do":"assert_agent_status","role":"产品策划","status":"active"},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"done"}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();
    }

    #[test]
    fn scenario_permission_ask_path() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["后端"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"实现","roles":["后端"],"due":[]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"bash","input":{"cmd":"npm install zod"}}]},
                {"text": "装好了"}
            ]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"assert_event","kind":"permission_asked"},
                {"do":"answer_permission","allow":true,"remember":"npm install *","scope":"project"},
                {"do":"assert_event","kind":"permission_allowed"},
                {"do":"assert_event","kind":"permission_shape_remembered","contains":{"shape":"npm install *"}},
                {"do":"assert_event","kind":"tool_result"}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();
    }

    #[test]
    fn scenario_deny_and_missing_role_skip() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划"],           // 无 UI → 阶段 1 跳过
            "pack": {"name":"t","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":[]},
                {"name":"界面","roles":["UI"],"due":["界面稿"]}
            ]},
            "scripts": {"default": [{"text": "ok"}]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"done"},
                {"do":"assert_stage","seq":1,"state":"skipped"},
                {"do":"assert_event","kind":"stage_skipped"},
                {"do":"assert_no_event","kind":"permission_asked"}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();
    }
}
