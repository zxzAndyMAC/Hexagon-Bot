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
use crate::trace::{Event, EventKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub roles: Vec<String>,
    pub pack: Option<PackDef>,
    /// 模型槽 → 脚本。缺省槽名 "default"。
    #[serde(default)]
    pub scripts: HashMap<String, Vec<ScriptedStep>>,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptedStep {
    Text { text: String },
    Calls { tool_calls: Vec<CallSpec> },
    Resp { response: Box<RespSpec> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSpec {
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 负责人跳过某次声明复审（留 review_skipped 事件）。
    SkipReview {
        artifact_kind: String,
    },
    /// 以 role 身份对 path 产物提交复审（pass/reject）。
    SubmitReview {
        role: String,
        artifact: String,
        verdict: String,
        body: Option<String>,
    },
    // ---- 断言 ----
    AssertEvent {
        kind: EventKind,
        contains: Option<Value>,
    },
    /// 黄金轨迹：期望的 kind 序列是实际事件流的有序子序列（允许穿插其他事件）。
    AssertEventSeq {
        kinds: Vec<EventKind>,
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

/// 回放用跑法（票 06）：报告带事件流与用量账,调用方做指标抽取。
/// violations = 不变量伴随件在收尾时的违规数（票 07 挂接点：回放
/// 证据自带完整性校验,坏轨迹不能冒充证据）。
pub struct CapturedRun {
    pub events: Vec<Event>,
    pub usage: crate::usage::UsageSummary,
    pub violations: usize,
}

pub fn run_scenario(dir: &std::path::Path, sc: &Scenario) -> Result<ScenarioReport, ApiError> {
    // 与 capture 同一条执行路径——曾经各写一份,两边 step 报错格式
    // 还漂移过(eprintln vs ApiError),review 后收成单源。
    let cap = run_scenario_capture(dir, sc)?;
    Ok(ScenarioReport {
        events_len: cap.events.len(),
    })
}

/// 与 run_scenario 同路,但返回事件流+用量——回放/验收的证据出口。
/// 收尾前先跑不变量伴随件：违规事件也进流,违规数单独带出。
pub fn run_scenario_capture(dir: &std::path::Path, sc: &Scenario) -> Result<CapturedRun, ApiError> {
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
        run_step(&wb, step).map_err(|e| ApiError::NoRole(format!("step {i} {step:?}: {e}")))?;
    }
    let violations = crate::invariant::check_and_log(&wb.db, &wb.project_id)?;
    Ok(CapturedRun {
        events: wb.db.events(&wb.project_id, None)?,
        usage: crate::usage::project_summary(&wb.db, &wb.project_id)?,
        violations,
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
            // 与壳层 send_message 同路由：控制面落库+即时指令，余下交 wb 分发
            let (_id, cmd) = crate::commands::send_via_control(&wb.db, &wb.project_id, text)
                .map_err(|e| ApiError::BadInput(e.to_string()))?;
            if let Some(c) = cmd {
                wb.dispatch_command(&c)?;
            }
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
        StepDef::Pause => crate::orchestra::pause(&wb.db, &wb.project_id)?,
        StepDef::Resume => crate::orchestra::resume(&wb.db, &wb.project_id)?,
        StepDef::SleepAll => crate::orchestra::sleep_all(&wb.db, &wb.project_id)?,
        StepDef::AnswerPermission {
            allow,
            remember,
            scope,
        } => {
            let qid = crate::cards::first_queued(
                &wb.db,
                &wb.project_id,
                crate::cards::CardKind::Permission,
            )?
            .ok_or_else(|| ApiError::NoRole("no queued permission question".into()))?;
            wb.answer_permission(
                &qid,
                *allow,
                remember.as_deref(),
                scope.as_deref().unwrap_or("activation"),
            )?;
        }
        StepDef::SkipReview { artifact_kind } => {
            wb.skip_review(artifact_kind)?;
        }
        StepDef::SubmitReview {
            role,
            artifact,
            verdict,
            body,
        } => {
            let aid = wb.agent_by_role(role)?;
            let run = wb.active_run()?.map(|r| r.id);
            let ctx = wb.ctx_for(&aid, run);
            let art_id: String =
                crate::artifacts::query(&wb.db, &wb.project_id, None, None, None, None)?
                    .iter()
                    .find(|a| a.path == *artifact)
                    .map(|a| a.id.clone())
                    .ok_or_else(|| ApiError::NoRole(format!("no artifact {artifact}")))?;
            let v = match verdict.as_str() {
                "pass" | "通过" => crate::review::Verdict::Pass,
                _ => crate::review::Verdict::Reject,
            };
            crate::review::submit_review(
                &wb.db,
                &ctx,
                &art_id,
                v,
                body.as_deref().unwrap_or("scenario review"),
            )
            .map_err(|e| ApiError::NoRole(e.to_string()))?;
        }
        StepDef::AssertEvent { kind, contains } => {
            let evs = wb.db.events(&wb.project_id, Some(&[*kind]))?;
            let hit = match contains {
                None => !evs.is_empty(),
                Some(sub) => evs.iter().any(|e| json_contains(&e.payload, sub)),
            };
            assert!(hit, "expected event {kind:?} matching {contains:?}");
        }
        StepDef::AssertNoEvent { kind } => {
            let evs = wb.db.events(&wb.project_id, Some(&[*kind]))?;
            assert!(evs.is_empty(), "unexpected event {kind:?} present");
        }
        StepDef::AssertEventSeq { kinds } => {
            let evs = wb.db.events(&wb.project_id, None)?;
            let mut it = evs.iter().map(|e| e.kind);
            for want in kinds {
                assert!(
                    it.any(|k| k == *want),
                    "golden trace: missing {want:?} in event stream"
                );
            }
        }
        StepDef::AssertStage { seq, state } => {
            let stages = crate::orchestra::stage_status(&wb.db, &wb.project_id)?;
            let hit = stages.iter().any(|s| s.seq == *seq && s.state == *state);
            assert!(
                hit,
                "expected stage seq={seq} state={state}, got {stages:?}"
            );
        }
        StepDef::AssertArtifact { path, kind, status } => {
            let arts = crate::artifacts::query(&wb.db, &wb.project_id, None, None, None, None)?;
            let hit = arts.iter().any(|a| {
                a.path == *path
                    && kind.as_ref().is_none_or(|k| a.kind == *k)
                    && status.as_ref().is_none_or(|s| a.status == *s)
            });
            assert!(
                hit,
                "expected artifact {path} kind={kind:?} status={status:?}"
            );
        }
        StepDef::AssertAgentStatus { role, status } => {
            let team = crate::orchestra::team(&wb.db, &wb.project_id)?;
            let hit = team.iter().any(|m| m.role == *role && m.status == *status);
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

    /// 黄金轨迹（票 28）：盖章点流程全走一遍，断言事件序列本身。
    #[test]
    fn golden_trace_stamp_gate_flow() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划", "架构师"],
            "pack": {"name":"g","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":["规格"],
                 "checks":["true"],"stamp_point":true,
                 "reviews":[{"artifact_kind":"规格","reviewer":"架构师"}]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"artifact_write","input":{
                    "path":"specs/prd.md",
                    "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}}]},
                {"text": "done"}
            ]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"submit_review","role":"架构师","artifact":"specs/prd.md","verdict":"pass"},
                {"do":"run_checks"},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"waiting_stamp"},
                {"do":"stamp"},
                {"do":"assert_stage","seq":0,"state":"done"},
                {"do":"assert_event_seq","kinds":[
                    "stage_started","agent_activated","artifact_delivered",
                    "review_passed","test_ran","stamped","team_slept"]}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();
    }

    /// US25：复审驳回只产生驳回事件与本阶段返工语义——上游阶段不退回。
    #[test]
    fn us25_review_reject_does_not_rewind() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划", "架构师", "后端"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":["规格"]},
                {"name":"实现","roles":["后端"],"due":[]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"artifact_write","input":{
                    "path":"specs/prd.md",
                    "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}}]},
                {"text": "done"}
            ]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"done"},
                {"do":"open_stage","seq":1},
                {"do":"submit_review","role":"架构师","artifact":"specs/prd.md","verdict":"reject","body":"目标不清"},
                {"do":"assert_event","kind":"review_rejected"},
                {"do":"assert_no_event","kind":"stage_rewound"},
                {"do":"assert_stage","seq":0,"state":"done"}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();
    }

    /// US26：跳过复审留痕——review_skipped 进轨迹且阶段评估视作满足；
    /// 已通过的复审不可跳（章后无后门）。
    #[test]
    fn us26_skip_review_leaves_trace_and_satisfies() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划", "架构师"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":["规格"],
                 "reviews":[{"artifact_kind":"规格","reviewer":"架构师"}]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"artifact_write","input":{
                    "path":"specs/prd.md",
                    "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}}]},
                {"text": "done"}
            ]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"active"},   // 复审未过挡评估
                {"do":"skip_review","artifact_kind":"规格"},
                {"do":"assert_event","kind":"review_skipped","contains":{"artifact_kind":"规格","reviewer":"架构师","by":"owner"}},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"done"}
            ]
        }))
        .unwrap();
        run_scenario(dir.path(), &sc).unwrap();

        // 已通过不可跳
        let dir2 = tempfile::tempdir().unwrap();
        let sc2: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["产品策划", "架构师"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"规格","roles":["产品策划"],"due":["规格"],
                 "reviews":[{"artifact_kind":"规格","reviewer":"架构师"}]}
            ]},
            "scripts": {"default": [
                {"tool_calls": [{"name":"artifact_write","input":{
                    "path":"specs/prd.md",
                    "content":"---\nkind: 规格\nauthor: a0\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx"}}]},
                {"text": "done"}
            ]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"submit_review","role":"架构师","artifact":"specs/prd.md","verdict":"pass"}
            ]
        }))
        .unwrap();
        // 手动补 skip：run_scenario 没有 expect-error 步，直接借 Workbench 断言
        let roles: Vec<&str> = sc2.roles.iter().map(|s| s.as_str()).collect();
        let mut wb = Workbench::for_test(dir2.path(), &roles, sc2.pack.clone()).unwrap();
        for (slot, steps) in &sc2.scripts {
            let resps: Vec<ChatResponse> = steps
                .iter()
                .enumerate()
                .map(|(i, s)| s.to_response(i))
                .collect();
            wb.register_provider(slot, Arc::new(ScriptedProvider::new(resps)));
        }
        for step in &sc2.steps {
            run_step(&wb, step).unwrap();
        }
        assert!(wb.skip_review("规格").is_err());
    }

    /// US33：检验红 → 阶段评估不过（合入盖章的默认挡路在 git::stamp_gate_blocks_merge）。
    #[test]
    fn us33_failed_check_blocks_eval() {
        let dir = tempfile::tempdir().unwrap();
        let sc: Scenario = serde_json::from_value(serde_json::json!({
            "roles": ["后端"],
            "pack": {"name":"t","version":1,"stages":[
                {"name":"实现","roles":["后端"],"due":[],"checks":["false"]}
            ]},
            "scripts": {"default": [{"text": "done"}]},
            "steps": [
                {"do":"open_stage","seq":0},
                {"do":"run_all_active"},
                {"do":"run_checks"},
                {"do":"assert_event","kind":"test_ran","contains":{"exit_code":1}},
                {"do":"advance"},
                {"do":"assert_stage","seq":0,"state":"active"},
                {"do":"assert_no_event","kind":"stage_finished"}
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
