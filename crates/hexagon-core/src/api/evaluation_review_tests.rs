use super::evaluation_live_tests::{
    attest_price, fixture_configuration, priced_request, ProbeProvider,
};
use super::*;
use crate::evaluation as eval;
use crate::provider::{
    ChatRequest, ChatResponse, ContentBlock, ModelMeta, ProviderError, StopReason, Usage,
};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ReviewProvider {
    calls: AtomicUsize,
    verdict: Option<&'static str>,
    pause: bool,
}
impl ModelProvider for ReviewProvider {
    fn billing_model(&self) -> Option<&str> {
        Some("fixture-model-v1")
    }
    fn model_meta(&self) -> ModelMeta {
        ProbeProvider { wrong_model: false }.model_meta()
    }
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let call = |name: &str, input: Value| ContentBlock::ToolUse {
            id: format!("review-{n}"),
            name: name.into(),
            input,
        };
        let step = if self.pause && n >= 6 { n - 2 } else { n };
        let content = if self.pause && n == 4 {
            call("bash", json!({"cmd":"/bin/echo review-context"}))
        } else if self.pause && n == 5 {
            ContentBlock::Text {
                text: r#"{"verdict":"unsure","reason":"fixture permission"}"#.into(),
            }
        } else {
            match step {
                1 => call(
                    "artifact_write",
                    json!({"path":"docs/note.md","kind":"结构说明","content":"Contract to independently review."}),
                ),
                4 => {
                    if self.pause {
                        let messages = serde_json::to_string(&request.messages).unwrap();
                        assert!(
                            messages.contains("Independently review"),
                            "resume lost review assignment"
                        );
                        assert!(
                            messages.contains("Resume the independent review"),
                            "resume lost guidance"
                        );
                        let results = request.messages.iter().flat_map(|m| &m.content).filter(|b| matches!(b, ContentBlock::ToolResult { content, is_error: false, .. } if content.contains("review-context"))).count();
                        assert_eq!(results, 1, "approved command result must occur once");
                    }
                    call("artifact_read", json!({"path":"docs/note.md"}))
                }
                5 if self.verdict.is_some() => {
                    let body = request
                        .messages
                        .iter()
                        .rev()
                        .flat_map(|m| &m.content)
                        .find_map(|b| match b {
                            ContentBlock::ToolResult { content, .. } => Some(content),
                            _ => None,
                        })
                        .unwrap();
                    let target: Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
                    call(
                        "artifact_write",
                        json!({"path":"docs/review.md","content":format!("---\nkind: 复审意见\nauthor: 架构师\ntarget: docs/note.md\nverdict: {}\ntarget_evidence: {}\n---\nIndependent review passed.", self.verdict.unwrap(), target["review_target"])}),
                    )
                }
                _ => ContentBlock::Text {
                    text: "Complete current assigned role.".into(),
                },
            }
        };
        let stop = if matches!(content, ContentBlock::ToolUse { .. }) {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };
        Ok(ChatResponse {
            content: vec![content],
            stop,
            usage: Usage {
                observed_model: Some("fixture-model-v1".into()),
                prompt_reported: true,
                completion_reported: true,
                prompt_tokens: 10,
                completion_tokens: 10,
                ..Default::default()
            },
        })
    }
}

fn run_review(verdict: Option<&'static str>, pause: bool) {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = priced_request();
    request.full_pack = serde_json::from_value(json!({"name":"review handoff","version":1,"stages":[{"name":"structure","roles":["UX"],"due":["结构说明"],"reviews":[{"artifact_kind":"结构说明","reviewer":"架构师"}]}]})).unwrap();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    attest_price(&wb, &batch);
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    if plan.entries[0].arm == eval::EvaluationArm::Fast {
        wb.register_provider(
            "default",
            Arc::new(ReviewProvider {
                calls: AtomicUsize::new(100),
                verdict,
                pause: false,
            }),
        );
        let fast = wb.evaluate_next_live(&plan.id).unwrap();
        wb.stop_evaluation_run(&fast.id).unwrap();
    }
    let provider = Arc::new(ReviewProvider {
        calls: AtomicUsize::new(0),
        verdict,
        pause,
    });
    wb.register_provider("default", provider.clone());
    let mut result = wb.evaluate_next_live(&plan.id).unwrap_or_else(|error| {
        panic!(
            "review dispatch failed: {error}; admission recheck={:?}",
            wb.check_evaluation_configuration(&batch.id, &request)
                .map(|current| current.blocks)
        )
    });
    if pause {
        assert_eq!(result.state, "waiting_human");
        let question = wb
            .evaluation_pending(&result.id)
            .unwrap()
            .into_iter()
            .find(|q| q.kind == "permission")
            .unwrap();
        let h = wb
            .begin_evaluation_attention(&result.id, eval::EvaluationActor::Scripted)
            .unwrap();
        wb.add_evaluation_guidance(
            &h,
            "Resume the independent review using the recorded result.",
        )
        .unwrap();
        wb.end_evaluation_attention(&h, eval::AttentionEnd::Away)
            .unwrap();
        drop(wb);
        wb = Workbench::open_evaluation_host(home.path()).unwrap();
        wb.register_provider("default", provider.clone());
        let h = wb
            .begin_evaluation_attention(&result.id, eval::EvaluationActor::Scripted)
            .unwrap();
        result = wb
            .submit_evaluation_decision(
                &h,
                eval::EvaluationDecision::Permission {
                    question_id: question.id.clone(),
                    allow: true,
                },
            )
            .unwrap();
        let requests = wb.evaluation_budget().unwrap().requests;
        assert!(wb
            .submit_evaluation_decision(
                &h,
                eval::EvaluationDecision::Permission {
                    question_id: question.id,
                    allow: true
                }
            )
            .is_err());
        assert_eq!(wb.evaluation_budget().unwrap().requests, requests);
    }
    let worker = Workbench::open_evaluation_host(std::path::Path::new(&result.workspace)).unwrap();
    assert_eq!(
        result.flow_completed,
        verdict == Some("pass"),
        "result={result:?}, evidence={:?}",
        worker.stage_evidence().unwrap()
    );
    if verdict != Some("pass") {
        assert_eq!(result.state, "incomplete");
        assert!(worker
            .stage_evidence()
            .unwrap()
            .unwrap()
            .missing
            .iter()
            .any(|m| m.starts_with("review:")));
    }
    if !pause && verdict == Some("pass") {
        // Ticket 24: successful native artifact operations need bound scope
        // observations; completing a review alone cannot manufacture safety.
        let observed = wb.inspect_evaluation_outcome(&result.id).unwrap();
        assert_eq!(observed.safety, eval::SafetyVerdict::Passed, "{observed:?}");
    }
    wb.stop_evaluation_plan(&plan.id).unwrap();
}

#[test]
fn evaluation_full_flow_dispatches_required_independent_review() {
    run_review(Some("pass"), false);
}

#[test]
fn evaluation_full_flow_rejected_review_cannot_advance() {
    run_review(Some("reject"), false);
}

#[test]
fn evaluation_full_flow_missing_review_cannot_advance() {
    run_review(None, false);
}

#[test]
fn evaluation_full_flow_review_permission_survives_reopen() {
    run_review(Some("pass"), true);
}

#[test]
fn evaluation_full_flow_assigns_each_review_kind_once_after_production() {
    use super::evaluation_live_tests::TaskProvider;
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = priced_request();
    request.full_pack = serde_json::from_value(json!({"name":"review assignments","version":1,"stages":[{"name":"delivery","roles":["UX","架构师"],"due":["结构说明","接口说明","代码"],"reviews":[{"artifact_kind":"结构说明","reviewer":"架构师"},{"artifact_kind":"代码","reviewer":"后端技术负责人"},{"artifact_kind":"接口说明","reviewer":"架构师"}]}]})).unwrap();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    attest_price(&wb, &batch);
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    let empty = || ChatResponse {
        content: vec![ContentBlock::Text {
            text: "No delivery or review submitted.".into(),
        }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    if plan.entries[0].arm == eval::EvaluationArm::Fast {
        wb.register_provider(
            "default",
            Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(vec![
                empty(),
                empty(),
            ]))),
        );
        let fast = wb.evaluate_next_live(&plan.id).unwrap();
        wb.stop_evaluation_run(&fast.id).unwrap();
    }
    let provider = Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(
        (0..8).map(|_| empty()).collect(),
    )));
    wb.register_provider("default", provider.clone());
    let run = wb.evaluate_next_live(&plan.id).unwrap();
    assert_eq!(
        run.state, "incomplete",
        "missing deliveries/reviews still block"
    );
    let instructions: Vec<String> = provider
        .0
        .recorded()
        .iter()
        .filter(|r| !r.tools.is_empty())
        .map(|r| {
            r.messages
                .iter()
                .filter(|m| m.role == crate::provider::Role::User)
                .flat_map(|m| &m.content)
                .find_map(|b| match b {
                    ContentBlock::Text { text } => serde_json::from_str::<Value>(text)
                        .ok()?
                        .get("instruction")?
                        .as_str()
                        .map(str::to_owned),
                    _ => None,
                })
                .unwrap()
        })
        .collect();
    // Two production activations precede two reviewer activations. The architect
    // contributes first, then receives both declared kinds in a single review.
    assert_eq!(instructions.len(), 4);
    assert_eq!(instructions[0], instructions[1]);
    let first: Value =
        serde_json::from_str(&instructions[2]).unwrap_or_else(|e| panic!("{e}: {instructions:?}"));
    let second: Value = serde_json::from_str(&instructions[3]).unwrap();
    assert_eq!(first["review_kinds"], json!(["结构说明", "接口说明"]));
    assert_eq!(second["review_kinds"], json!(["代码"]));
    wb.stop_evaluation_plan(&plan.id).unwrap();
}
