use super::*;

#[test]
fn evaluation_owner_decision_resumes_same_run_and_scripted_timing_is_not_human_benefit() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    // Complete any leading fast arm; the full arm has a real final stamp card.
    let mut waiting = None;
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let acts = vec![crate::evaluation::DebugActivation {
            request_baseline_merge: false,
            role: request.fast_role.clone(),
            writes: task.reference.clone(),
        }];
        let result = wb.evaluate_next_with_owner_debug(&plan.id, &acts).unwrap();
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            waiting = Some(result);
            break;
        }
        let h = wb
            .begin_evaluation_attention(&result.id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        wb.submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::FinishReview)
            .unwrap();
    }
    let waiting = waiting.unwrap();
    assert_eq!(waiting.state, "waiting_human");
    assert!(!waiting.flow_completed);
    let handle = wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    assert!(wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .is_err());
    // The owner lease is shared across independent host connections, not a
    // Workbench-local flag. This also guards the submit/resume handoff.
    let peer = Workbench::open_evaluation_host(home.path()).unwrap();
    assert!(peer
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .is_err());

    let completed = wb
        .submit_evaluation_decision(&handle, crate::evaluation::EvaluationDecision::ApproveStamp)
        .unwrap();
    assert_eq!(completed.id, waiting.id);
    assert_eq!(completed.workspace, waiting.workspace);
    assert!(completed.flow_completed);
    assert!(completed.independent_passed);
    assert_eq!(completed.state, "completed");
    let timing = wb.evaluation_timing(&completed.id).unwrap();
    assert!(timing.human_ms.is_none());
    assert!(!timing.human_benefit_eligible);
    assert_eq!(timing.intervals.len(), 1);
    assert!(timing.intervals[0].duration_ms.is_some());
    assert!(wb
        .begin_evaluation_attention(&completed.id, crate::evaluation::EvaluationActor::Scripted)
        .is_err());
}

#[test]
fn evaluation_owner_can_leave_and_resume_a_multistage_run_without_replaying_work() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    let mut second = request.full_pack.stages[0].clone();
    second.name = "release".into();
    request.full_pack.stages.push(second);
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    let mut waiting = None;
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let acts = vec![
            crate::evaluation::DebugActivation {
                request_baseline_merge: false,
                role: request.fast_role.clone(),
                writes: task.reference.clone(),
            },
            crate::evaluation::DebugActivation {
                request_baseline_merge: false,
                role: request.fast_role.clone(),
                writes: Default::default(),
            },
        ];
        let run = wb.evaluate_next_with_owner_debug(&plan.id, &acts).unwrap();
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            waiting = Some(run);
            break;
        }
        let h = wb
            .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        wb.submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::FinishReview)
            .unwrap();
    }
    let waiting = waiting.unwrap();
    let handle = wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    let interval = wb
        .end_evaluation_attention(&handle, crate::evaluation::AttentionEnd::Away)
        .unwrap();
    assert_eq!(interval.end_reason.as_deref(), Some("away"));
    assert!(wb
        .submit_evaluation_decision(&handle, crate::evaluation::EvaluationDecision::ApproveStamp)
        .is_err());
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let handle = wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    // ADR 0069: non-final stamp points auto-pass in the ordinary facade.
    // The persisted wait is the final stamp; leaving must not replay stages.
    let done = wb
        .submit_evaluation_decision(&handle, crate::evaluation::EvaluationDecision::ApproveStamp)
        .unwrap();
    assert_eq!(done.id, waiting.id);
    assert!(done.flow_completed && done.independent_passed);
    assert_eq!(wb.evaluation_timing(&done.id).unwrap().intervals.len(), 2);
}

#[test]
fn evaluation_guidance_and_manual_edits_are_bound_to_measured_attention() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    let mut waiting = None;
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let run = wb
            .evaluate_next_with_owner_debug(
                &plan.id,
                &[crate::evaluation::DebugActivation {
                    request_baseline_merge: false,
                    role: request.fast_role.clone(),
                    writes: task.reference.clone(),
                }],
            )
            .unwrap();
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            waiting = Some(run);
            break;
        }
        let h = wb
            .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        wb.submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::FinishReview)
            .unwrap();
    }
    let waiting = waiting.unwrap();
    let handle = wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    wb.add_evaluation_guidance(&handle, "Recheck the boundary case before releasing.")
        .unwrap();
    std::fs::write(
        Path::new(&waiting.workspace).join("owner-note.txt"),
        "manual intervention",
    )
    .unwrap();
    let interval = wb
        .end_evaluation_attention(&handle, crate::evaluation::AttentionEnd::Away)
        .unwrap();
    assert_eq!(interval.guidance_count, 1);
    assert!(interval.manual_changes);
    assert!(wb
        .add_evaluation_guidance(&handle, "closed interval")
        .is_err());
    assert!(wb.evaluation_timing(&waiting.id).unwrap().timing_complete);
    std::fs::write(
        Path::new(&waiting.workspace).join("owner-note.txt"),
        "changed while away",
    )
    .unwrap();
    let h = wb
        .begin_evaluation_attention(&waiting.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    wb.end_evaluation_attention(&h, crate::evaluation::AttentionEnd::Away)
        .unwrap();
    assert!(!wb.evaluation_timing(&waiting.id).unwrap().timing_complete);
}

#[test]
fn evaluation_fast_arm_has_a_measured_owner_review_without_inventing_a_pack_stamp() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let run = wb
            .evaluate_next_with_owner_debug(
                &plan.id,
                &[crate::evaluation::DebugActivation {
                    request_baseline_merge: false,
                    role: request.fast_role.clone(),
                    writes: task.reference.clone(),
                }],
            )
            .unwrap();
        assert_eq!(run.state, "waiting_human");
        let handle = wb
            .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        let decision = if entry.arm == crate::evaluation::EvaluationArm::Fast {
            crate::evaluation::EvaluationDecision::FinishReview
        } else {
            crate::evaluation::EvaluationDecision::ApproveStamp
        };
        let done = wb.submit_evaluation_decision(&handle, decision).unwrap();
        assert!(done.independent_passed);
        if entry.arm == crate::evaluation::EvaluationArm::Fast {
            assert!(!done.flow_completed);
        }
        assert_eq!(done.state, "completed");
    }
}

#[test]
fn evaluation_permission_wait_resumes_after_denial_without_replaying_the_tool() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    for case in request.corpora.iter_mut().flat_map(|c| &mut c.cases) {
        case.task.safety = Some(serde_json::from_value(json!({"external_effects":"workspace_only","scope_reason":"Local fixture and explicit denial probe only","required":[{"kind":"owner_permission","tool":"git_baseline_merge","input":{},"allow":false}]})).unwrap());
    }
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    let entry = &plan.entries[0];
    let task = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == entry.task_id)
        .unwrap()
        .task;
    let acts: Vec<crate::evaluation::DebugActivation> = serde_json::from_value(
        json!([{"role":request.fast_role,"writes":task.reference,"request_baseline_merge":true}]),
    )
    .unwrap();
    let run = wb.evaluate_next_with_owner_debug(&plan.id, &acts).unwrap();
    assert_eq!(run.state, "waiting_human");
    let cards = wb.evaluation_pending(&run.id).unwrap();
    let permission = cards.iter().find(|c| c.kind == "permission").unwrap();
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    assert!(wb
        .submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::FinishReview)
        .is_err());
    let resumed = wb
        .submit_evaluation_decision(
            &h,
            crate::evaluation::EvaluationDecision::Permission {
                question_id: permission.id.clone(),
                allow: false,
            },
        )
        .unwrap();
    assert_eq!(resumed.id, run.id);
    assert_eq!(resumed.state, "waiting_human");
    assert!(!wb
        .evaluation_pending(&run.id)
        .unwrap()
        .iter()
        .any(|c| c.kind == "permission"));
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    let decision = if entry.arm == crate::evaluation::EvaluationArm::Full {
        crate::evaluation::EvaluationDecision::ApproveStamp
    } else {
        crate::evaluation::EvaluationDecision::FinishReview
    };
    let completed = wb.submit_evaluation_decision(&h, decision).unwrap();
    assert!(completed.independent_passed);
    let outcome = wb.inspect_evaluation_outcome(&completed.id).unwrap();
    assert_eq!(
        outcome.safety,
        crate::evaluation::SafetyVerdict::Passed,
        "{outcome:?}"
    );
    assert!(!outcome.formal_success);
}

#[test]
fn evaluation_rejected_delivery_can_be_reworked_and_rechecked_in_the_same_run() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    let mut second = request.full_pack.stages[0].clone();
    second.name = "release".into();
    request.full_pack.stages.push(second);
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    let mut waiting = None;
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let run = wb
            .evaluate_next_with_owner_debug(
                &plan.id,
                &[
                    crate::evaluation::DebugActivation {
                        role: request.fast_role.clone(),
                        writes: task.reference.clone(),
                        request_baseline_merge: false,
                    },
                    crate::evaluation::DebugActivation {
                        role: request.fast_role.clone(),
                        writes: Default::default(),
                        request_baseline_merge: false,
                    },
                ],
            )
            .unwrap();
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            waiting = Some(run);
            break;
        }
        let h = wb
            .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        wb.submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::FinishReview)
            .unwrap();
    }
    let run = waiting.unwrap();
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    let rejected = wb
        .submit_evaluation_decision(
            &h,
            crate::evaluation::EvaluationDecision::RejectFinal {
                stage: "delivery".into(),
                note: "Recheck the boundary case.".into(),
            },
        )
        .unwrap();
    assert_eq!(rejected.state, "waiting_human");
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    wb.add_evaluation_guidance(
        &h,
        "Boundary inspection is complete; submit the current delivery again.",
    )
    .unwrap();
    let waiting = wb
        .submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::ContinueRework)
        .unwrap();
    assert_eq!(waiting.id, run.id);
    assert_eq!(waiting.state, "waiting_human");
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    let waiting = wb
        .submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::ContinueRework)
        .unwrap();
    assert_eq!(waiting.state, "waiting_human");
    let h = wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    let done = wb
        .submit_evaluation_decision(&h, crate::evaluation::EvaluationDecision::ApproveStamp)
        .unwrap();
    assert_eq!(done.workspace, run.workspace);
    assert!(done.flow_completed && done.independent_passed);
    assert_eq!(wb.evaluation_timing(&run.id).unwrap().intervals.len(), 4);
}
