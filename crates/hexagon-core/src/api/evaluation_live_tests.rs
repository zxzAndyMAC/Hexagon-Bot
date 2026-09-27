use super::*;
use crate::evaluation as eval;
use crate::provider::{
    ChatRequest, ChatResponse, ContentBlock, Message, ModelMeta, ProviderError, Role, StopReason,
    Usage,
};

pub(super) fn fixture_configuration() -> crate::provider_config::FixtureDocument {
    use crate::provider_config::*;
    fixture_document(ProviderDoc {
        providers: vec![ProviderDef {
            id: "fixture".into(),
            name: "Provider boundary fixture".into(),
            kind: crate::provider::ProviderKind::OpenAi,
            base_url: "https://fixture.invalid/v1".into(),
            enabled: true,
            models: vec![ModelEntry {
                id: "fixture-model-v1".into(),
                context_window: Some(100000),
                max_output: Some(4096),
                caps: vec!["tools".into()],
                ..Default::default()
            }],
        }],
        slots: std::collections::HashMap::from([(
            "default".into(),
            SlotBinding {
                provider_id: "fixture".into(),
                model: "fixture-model-v1".into(),
            },
        )]),
        ..Default::default()
    })
}

pub(super) fn priced_request() -> eval::FreezeRequest {
    let mut request = super::evaluation_config_tests::request();
    request.prices.insert(
        "default".into(),
        eval::PriceSource {
            provider_id: "fixture".into(),
            model: "fixture-model-v1".into(),
            prompt_per_1k_mc: 10,
            completion_per_1k_mc: 10,
            source_url: "https://fixture.invalid/pricing".into(),
            checked_at: "2026-09-27".into(),
            billing_scope: "text_input_output_only".into(),
        },
    );
    request
}
pub(super) fn attest_price(wb: &Workbench, batch: &eval::EvaluationBatch) {
    let price = &batch.request.prices["default"];
    wb.record_evaluation_verification(
        &batch.id,
        &eval::VerificationObservation {
            dimension: eval::VerificationDimension::Price,
            outcome: eval::ObservedOutcome::ReportedPass,
            batch_fingerprint: batch.fingerprint.clone(),
            source_url: price.source_url.clone(),
            checked_at: price.checked_at.clone(),
        },
    )
    .unwrap();
}

pub(super) struct ProbeProvider {
    pub(super) wrong_model: bool,
}
impl ModelProvider for ProbeProvider {
    fn billing_model(&self) -> Option<&str> {
        Some("fixture-model-v1")
    }
    fn model_meta(&self) -> ModelMeta {
        ModelMeta {
            context_window: Some(100000),
            max_output: Some(4096),
        }
    }
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let usage = Usage {
            observed_model: Some(
                if self.wrong_model {
                    "different-model"
                } else {
                    "fixture-model-v1"
                }
                .into(),
            ),
            prompt_tokens: 10,
            completion_tokens: 10,
            prompt_reported: true,
            completion_reported: true,
            ..Default::default()
        };
        if let Some(tool) = request.tools.first() {
            Ok(ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: "probe-call".into(),
                    name: tool.name.clone(),
                    input: json!({"nonce":tool.input_schema["properties"]["nonce"]["enum"][0]}),
                }],
                stop: StopReason::ToolUse,
                usage,
            })
        } else {
            let nonce = request
                .messages
                .iter()
                .rev()
                .find_map(|m| match m {
                    Message {
                        role: Role::Tool,
                        content,
                    } => content.iter().find_map(|c| {
                        if let ContentBlock::ToolResult { content, .. } = c {
                            Some(content.clone())
                        } else {
                            None
                        }
                    }),
                    _ => None,
                })
                .unwrap();
            Ok(ChatResponse {
                content: vec![ContentBlock::Text { text: nonce }],
                stop: StopReason::EndTurn,
                usage,
            })
        }
    }
}

#[test]
fn evaluation_paid_preflight_requires_attestation_and_binds_actual_receipts() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
    assert!(wb.preflight_evaluation(&batch.id).is_err());
    assert_eq!(wb.evaluation_budget().unwrap().requests, 0);
    attest_price(&wb, &batch);
    assert!(!wb.evaluation_batch(&batch.id).unwrap().ready);
    let receipt = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(receipt.state, "passed", "{:?}", receipt.reason);
    assert_eq!(receipt.evidence_kind, "provider_boundary_fixture");
    assert_eq!(receipt.request_ids.len(), 2);
    assert!(wb.evaluation_batch(&batch.id).unwrap().ready);
    let budget = wb.evaluation_budget().unwrap();
    assert_eq!(budget.requests, 2);
    assert_eq!(budget.confirmed_requests, 2);
    assert_eq!(budget.unknown_mc, 0);
    assert!(budget.known_mc > 0);
    assert_eq!(budget.reserved_mc, 0);
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    wb.enable_evaluation_live(&plan.id).unwrap();
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    assert!(wb.evaluation_batch(&batch.id).unwrap().ready);
    assert_eq!(
        wb.evaluation_preflight(&receipt.id).unwrap().request_ids,
        receipt.request_ids
    );
}

#[test]
fn evaluation_paid_preflight_model_mismatch_preserves_unknown_money() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: true }));
    let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
    attest_price(&wb, &batch);
    let receipt = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(receipt.state, "failed");
    assert!(!wb.evaluation_batch(&batch.id).unwrap().ready);
    let budget = wb.evaluation_budget().unwrap();
    assert_eq!(budget.requests, 1);
    assert!(budget.blocked);
    assert!(budget.unknown_mc > 0);
}

pub(super) struct TaskProvider(pub(super) crate::provider::ScriptedProvider);
impl ModelProvider for TaskProvider {
    fn billing_model(&self) -> Option<&str> {
        Some("fixture-model-v1")
    }
    fn model_meta(&self) -> ModelMeta {
        ModelMeta {
            context_window: Some(100000),
            max_output: Some(4096),
        }
    }
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut response = self.0.complete(request)?;
        response.usage = Usage {
            observed_model: Some("fixture-model-v1".into()),
            prompt_reported: true,
            completion_reported: true,
            prompt_tokens: 10,
            completion_tokens: 10,
            ..Default::default()
        };
        Ok(response)
    }
}
#[test]
fn evaluation_live_driver_uses_metered_transport_and_waits_for_real_owner_decision() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    let request = priced_request();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    assert!(wb.evaluate_next_live(&plan.id).is_err());
    assert!(wb
        .evaluation_plan(&plan.id)
        .unwrap()
        .entries
        .iter()
        .all(|e| e.run_id.is_none()));
    attest_price(&wb, &batch);
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    let task = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == plan.entries[0].task_id)
        .unwrap()
        .task;
    let text = |s: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: s.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    let mut replies = vec![text("Inspect declared task files")];
    for (path, body) in &task.reference {
        for (name, input) in [
            ("fs_read", json!({"path":path})),
            ("fs_write", json!({"path":path,"content":body})),
        ] {
            replies.push(ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: format!("call-{}", replies.len()),
                    name: name.into(),
                    input,
                }],
                stop: StopReason::ToolUse,
                usage: Default::default(),
            });
        }
    }
    replies.push(text("Task ready for owner review"));
    wb.register_provider(
        "default",
        Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(
            replies,
        ))),
    );
    let run = wb.evaluate_next_live(&plan.id).unwrap();
    assert_eq!(run.state, "waiting_human", "{:?}", run.error);
    assert_eq!(run.evidence_kind, "provider_boundary_fixture");
    let requests = wb.evaluation_budget().unwrap().requests;
    assert!(requests > 2);
    assert!(wb.evaluate_next_live(&plan.id).is_err());
    assert_eq!(wb.evaluation_budget().unwrap().requests, requests);
    let attention = wb
        .begin_evaluation_attention(&run.id, eval::EvaluationActor::Scripted)
        .unwrap();
    let decision = if plan.entries[0].arm == eval::EvaluationArm::Full {
        eval::EvaluationDecision::ApproveStamp
    } else {
        eval::EvaluationDecision::FinishReview
    };
    let completed = wb.submit_evaluation_decision(&attention, decision).unwrap();
    assert_eq!(completed.id, run.id);
    assert_eq!(completed.state, "completed");
    assert!(completed.independent_passed);
    assert!(
        !wb.evaluation_timing(&run.id)
            .unwrap()
            .human_benefit_eligible
    );
    assert_eq!(wb.evaluation_budget().unwrap().unknown_mc, 0);
    assert_eq!(wb.evaluation_budget().unwrap().requests, requests);
}

#[test]
fn evaluation_preflight_detects_modified_host_receipt_on_read() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
    attest_price(&wb, &batch);
    let receipt = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(receipt.state, "passed");
    eval::live::corrupt_receipt_for_test(&wb.db, &receipt.id);
    assert!(wb.evaluation_preflight(&receipt.id).is_err());
    let next = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(next.state, "passed");
    let report = wb.inspect_evaluation_recovery().unwrap();
    assert_eq!(report.entries.len(), 2);
    assert_eq!(
        report
            .entries
            .iter()
            .find(|r| r.run_id == receipt.id)
            .unwrap()
            .state,
        eval::RecoveryState::Corrupt
    );
    assert_eq!(
        report
            .entries
            .iter()
            .find(|r| r.run_id == next.id)
            .unwrap()
            .state,
        eval::RecoveryState::Ended
    );
}

#[test]
fn evaluation_preflight_lost_activity_is_reconciled_without_redispatch() {
    for point in ["reserved", "returned"] {
        let _config = fixture_configuration();
        let home = tempfile::tempdir().unwrap();
        let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
        wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
        let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
        attest_price(&wb, &batch);
        eval::live::arm_activity_crash(point);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || wb.preflight_evaluation(&batch.id)
        ))
        .is_err());
        let before = wb.evaluation_budget().unwrap();
        let report = wb.inspect_evaluation_recovery().unwrap();
        assert_eq!(report.entries.len(), 1);
        assert_eq!(
            report.entries[0].state,
            eval::RecoveryState::NeedsReconciliation
        );
        let id = &report.entries[0].run_id;
        assert_eq!(
            wb.reconcile_evaluation_run(id).unwrap().state,
            eval::RecoveryState::Ended
        );
        assert_eq!(wb.evaluation_preflight(id).unwrap().state, "interrupted");
        assert!(!wb.evaluation_batch(&batch.id).unwrap().ready);
        let after = wb.evaluation_budget().unwrap();
        assert_eq!(after.requests, before.requests);
        assert_eq!(after.known_mc, before.known_mc);
        assert_eq!(after.unknown_mc, before.unknown_mc);
        assert_eq!(after.reserved_mc, 0);
        assert_eq!(
            wb.reconcile_evaluation_run(id).unwrap().state,
            eval::RecoveryState::Ended
        );
    }
}

pub(super) fn task_provider(task: &eval::EvaluationTask) -> Arc<dyn ModelProvider> {
    let text = |s: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: s.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    let mut responses = vec![text("Inspect declared task inputs")];
    for (path, body) in &task.reference {
        for (name, input) in [
            ("fs_read", json!({"path":path})),
            ("fs_write", json!({"path":path,"content":body})),
        ] {
            responses.push(ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: format!("call-{}", responses.len()),
                    name: name.into(),
                    input,
                }],
                stop: StopReason::ToolUse,
                usage: Default::default(),
            });
        }
    }
    responses.push(text("Task ready for review"));
    Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(
        responses,
    )))
}
