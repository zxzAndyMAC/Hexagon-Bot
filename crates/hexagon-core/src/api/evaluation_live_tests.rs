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
fn evaluation_preflight_prices_included_usage_details_without_blocking_second_request() {
    // 2026-09-28 preflight-1 regression: exercise the real response parser and
    // paid authority together; unknown extras must still stop the second call.
    struct DetailedProbe {
        unknown_extra: bool,
    }
    impl ModelProvider for DetailedProbe {
        fn billing_model(&self) -> Option<&str> {
            Some("fixture-model-v1")
        }
        fn model_meta(&self) -> ModelMeta {
            ProbeProvider { wrong_model: false }.model_meta()
        }
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            let mut response = ProbeProvider { wrong_model: false }.complete(request)?;
            let mut raw = json!({
                "model":"fixture-model-v1", "choices":[{"message":{"content":""},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":16,"completion_tokens":10,"total_tokens":26,
                    "prompt_cache_hit_tokens":5,"prompt_cache_miss_tokens":11,
                    "prompt_tokens_details":{"cached_tokens":5},
                    "completion_tokens_details":{"reasoning_tokens":4}}
            });
            if self.unknown_extra {
                raw["usage"]["server_tool_use"] = json!({"web_search_requests":1});
            }
            response.usage = crate::provider::openai_shape::from_response(&raw)?.usage;
            Ok(response)
        }
    }
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
    attest_price(&wb, &batch);
    wb.register_provider(
        "default",
        Arc::new(DetailedProbe {
            unknown_extra: false,
        }),
    );
    let receipt = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(receipt.state, "passed");
    let budget = wb.evaluation_budget().unwrap();
    assert_eq!(budget.requests, 2);
    assert_eq!(budget.known_mc, 2); // ceil((16 + 10) * 10 / 1000) per request
    assert_eq!(budget.unknown_mc, 0);
    assert!(!budget.blocked);
    wb.register_provider(
        "default",
        Arc::new(DetailedProbe {
            unknown_extra: true,
        }),
    );
    let failed = wb.preflight_evaluation(&batch.id).unwrap();
    assert_eq!(failed.state, "failed");
    let budget = wb.evaluation_budget().unwrap();
    assert_eq!(
        budget.requests, 3,
        "unknown extra stops the second exchange before dispatch"
    );
    assert!(budget.unknown_mc > 0);
    assert!(budget.blocked);
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

#[test]
fn evaluation_permission_resume_delivers_task_guidance_and_result_after_reopen() {
    assert_evaluation_permission_resume(true);
}

#[test]
fn evaluation_permission_resume_keeps_denials_after_reopen() {
    assert_evaluation_permission_resume(false);
}

fn assert_evaluation_permission_resume(allow: bool) {
    // 2026-09-28 live pilot: scripted response offsets hid a fresh, contextless
    // turn after each permission. Observe the real provider boundary instead.
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    let mut request = priced_request();
    for case in request.corpora.iter_mut().flat_map(|c| &mut c.cases) {
        case.task.allowed_paths.push("resume-marker.txt".into());
        case.task
            .files
            .insert("resume-marker.txt".into(), String::new());
    }
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    attest_price(&wb, &batch);
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    let requirements = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == plan.entries[0].task_id)
        .unwrap()
        .task
        .requirements;
    let text = |s: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: s.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    let provider = Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(vec![
        text("Verify the local interpreter once"),
        ChatResponse {
            content: vec![ContentBlock::ToolUse {
                id: "verify-once".into(),
                name: "bash".into(),
                input: json!({"cmd":"printf resume-proof >> resume-marker.txt; sleep 0.2; /bin/echo interpreter-ready"}),
            }],
            stop: StopReason::ToolUse,
            usage: Default::default(),
        },
        text(r#"{"verdict":"unsure","reason":"fixture requests an owner decision"}"#),
        ChatResponse {
            content: vec![ContentBlock::ToolUse {
                id: "verify-second".into(),
                name: "bash".into(),
                input: json!({"cmd":"/bin/echo second-ready"}),
            }],
            stop: StopReason::ToolUse,
            usage: Default::default(),
        },
        text(r#"{"verdict":"unsure","reason":"fixture requests an owner decision"}"#),
        text("Ready for review"),
    ])));
    wb.register_provider("default", provider.clone());
    let run = wb.evaluate_next_live(&plan.id).unwrap();
    assert_eq!(run.state, "waiting_human", "{:?}", run.error);
    let permission = wb
        .evaluation_pending(&run.id)
        .unwrap()
        .into_iter()
        .find(|c| c.kind == "permission")
        .expect("local command needs permission");
    let h = wb
        .begin_evaluation_attention(&run.id, eval::EvaluationActor::Scripted)
        .unwrap();
    wb.add_evaluation_guidance(&h, "Use this interpreter result; do not search for Conda.")
        .unwrap();
    wb.end_evaluation_attention(&h, eval::AttentionEnd::Away)
        .unwrap();
    drop(wb);
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", provider.clone());
    let h = wb
        .begin_evaluation_attention(&run.id, eval::EvaluationActor::Scripted)
        .unwrap();
    let resumed = wb
        .submit_evaluation_decision(
            &h,
            eval::EvaluationDecision::Permission {
                question_id: permission.id.clone(),
                allow,
            },
        )
        .unwrap();
    assert_eq!(resumed.state, "waiting_human", "{:?}", resumed.error);
    let before_duplicate = provider.0.recorded().len();
    assert!(wb
        .submit_evaluation_decision(
            &h,
            eval::EvaluationDecision::Permission {
                question_id: permission.id,
                allow,
            }
        )
        .is_err());
    assert_eq!(provider.0.recorded().len(), before_duplicate);
    let second = wb
        .evaluation_pending(&run.id)
        .unwrap()
        .into_iter()
        .find(|c| c.kind == "permission")
        .expect("second permission pause");
    let h = wb
        .begin_evaluation_attention(&run.id, eval::EvaluationActor::Scripted)
        .unwrap();
    wb.add_evaluation_guidance(&h, "Keep both observed results and finish this task.")
        .unwrap();
    let resumed = wb
        .submit_evaluation_decision(
            &h,
            eval::EvaluationDecision::Permission {
                question_id: second.id,
                allow,
            },
        )
        .unwrap();
    assert_eq!(resumed.state, "waiting_human", "{:?}", resumed.error);
    let calls = provider.0.recorded();
    let sent = serde_json::to_string(&calls.last().unwrap().messages).unwrap();
    let instruction = calls
        .last()
        .unwrap()
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::User))
        .flat_map(|m| &m.content)
        .find_map(|c| match c {
            ContentBlock::Text { text } => serde_json::from_str::<serde_json::Value>(text)
                .ok()?
                .get("instruction")?
                .as_str()
                .map(str::to_owned),
            _ => None,
        });
    assert_eq!(
        instruction.as_deref(),
        Some(requirements.as_str()),
        "original task lost"
    );
    assert!(
        sent.contains("Use this interpreter result; do not search for Conda."),
        "owner guidance lost"
    );
    assert!(sent.contains("Keep both observed results and finish this task."));
    let tool_results: Vec<_> = calls
        .last()
        .unwrap()
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::Tool))
        .flat_map(|m| &m.content)
        .collect();
    assert!(tool_results.iter().any(|c| matches!(c,
            ContentBlock::ToolResult { content, is_error, .. }
            if if allow { !is_error && content.contains("interpreter-ready") } else { *is_error && content.contains("denied") }
        )), "decision result missing: {tool_results:?}");
    assert_eq!(tool_results.len(), 2, "both permission receipts retained");
    if allow {
        assert!(
            tool_results.iter().any(|c| matches!(c,
                ContentBlock::ToolResult { content, is_error: false, .. }
                if content.contains("second-ready")
            )),
            "second approved result lost"
        );
    } else {
        assert!(
            tool_results.iter().all(|c| matches!(c,
                ContentBlock::ToolResult { content, is_error: true, .. }
                if content.contains("denied")
            )),
            "denial must not become success"
        );
    }
    let marker = Path::new(&run.workspace).join("resume-marker.txt");
    if allow {
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "resume-proof");
    } else {
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "");
    }
    wb.stop_evaluation_plan(&plan.id).unwrap();
}

#[test]
fn evaluation_permission_resume_excludes_previous_activation_and_other_instance() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    let mut request = priced_request();
    let stage = request.full_pack.stages[0].clone();
    request.full_pack.stages = ["old-own-stage", "other-instance-stage", "current-stage"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let mut next = stage.clone();
            next.name = name.into();
            next.stamp_point = i == 2;
            if i == 1 {
                next.roles = vec!["前端".into()];
            }
            next
        })
        .collect();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    attest_price(&wb, &batch);
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    let plan = wb
        .plan_evaluation(&batch.id, eval::PlanKind::Pilot)
        .unwrap();
    let text = |s: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: s.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    let bash = |id: &str, cmd: &str| ChatResponse {
        content: vec![ContentBlock::ToolUse {
            id: id.into(),
            name: "bash".into(),
            input: json!({"cmd":cmd}),
        }],
        stop: StopReason::ToolUse,
        usage: Default::default(),
    };
    if plan.entries[0].arm == eval::EvaluationArm::Fast {
        wb.register_provider(
            "default",
            Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(vec![
                text("Plan"),
                text("No changes for the unrelated fast arm"),
            ]))),
        );
        let fast = wb.evaluate_next_live(&plan.id).unwrap();
        wb.stop_evaluation_run(&fast.id).unwrap();
    }
    let unsure = r#"{"verdict":"unsure","reason":"fixture requests an owner decision"}"#;
    let provider = Arc::new(TaskProvider(crate::provider::ScriptedProvider::new(vec![
        text("Plan old stage"),
        bash("old", "/bin/echo old-activation-receipt"),
        text(unsure),
        text("Old stage complete"),
        text("Plan other instance"),
        bash("other", "printf other-instance-receipt"),
        text("Other stage complete"),
        text("Plan current stage"),
        bash("current", "/bin/echo current-activation-receipt"),
        text(unsure),
        text("Current stage complete"),
    ])));
    wb.register_provider("default", provider.clone());
    let run = wb.evaluate_next_live(&plan.id).unwrap();
    for guidance in ["old-stage-owner-guidance", "current-stage-owner-guidance"] {
        let permission = wb
            .evaluation_pending(&run.id)
            .unwrap()
            .into_iter()
            .find(|c| c.kind == "permission")
            .expect("stage waits for permission");
        let h = wb
            .begin_evaluation_attention(&run.id, eval::EvaluationActor::Scripted)
            .unwrap();
        wb.add_evaluation_guidance(&h, guidance).unwrap();
        let resumed = wb
            .submit_evaluation_decision(
                &h,
                eval::EvaluationDecision::Permission {
                    question_id: permission.id,
                    allow: false,
                },
            )
            .unwrap();
        assert_eq!(resumed.state, "waiting_human", "{:?}", resumed.error);
    }
    let calls = provider.0.recorded();
    let sent = serde_json::to_string(&calls.last().unwrap().messages).unwrap();
    assert!(sent.contains("current-activation-receipt"));
    assert!(sent.contains("current-stage-owner-guidance"));
    for unrelated in [
        "old-activation-receipt",
        "other-instance-receipt",
        "old-stage-owner-guidance",
    ] {
        assert!(
            !sent.contains(unrelated),
            "unrelated history leaked: {unrelated}"
        );
    }
    wb.stop_evaluation_plan(&plan.id).unwrap();
}
