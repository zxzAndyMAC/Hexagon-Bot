use super::*;

#[test]
fn evaluation_formal_plan_is_balanced_persistent_and_unstarted() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = wb
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Formal)
        .unwrap();
    assert_eq!(plan.entries.len(), 48);
    assert_eq!(
        plan.entries
            .iter()
            .filter(|e| e.position % 2 == 0 && e.arm == crate::evaluation::EvaluationArm::Fast)
            .count(),
        12
    );
    assert!(plan.entries.iter().all(|e| e.run_id.is_none()));
    let again = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Formal)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let restored = wb.evaluation_plan(&plan.id).unwrap();
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
    let stopped = wb.stop_evaluation_plan(&plan.id).unwrap();
    assert!(stopped
        .entries
        .iter()
        .all(|e| e.state == crate::evaluation::PlannedState::NotRun
            && e.run_id.is_none()
            && e.reason.as_deref() == Some("owner_stopped")));
    assert!(wb.evaluate_next_debug(&plan.id, &[]).is_err());
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    assert_eq!(
        serde_json::to_value(stopped).unwrap(),
        serde_json::to_value(wb.evaluation_plan(&plan.id).unwrap()).unwrap()
    );
}

#[test]
fn evaluation_pair_uses_separate_workers_and_real_stage_dispatch() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    let mut initial = request.full_pack.stages[0].clone();
    initial.name = "prepare".into();
    initial.stamp_point = false;
    request.full_pack.stages.insert(0, initial);
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    let task = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == plan.entries[0].task_id)
        .unwrap()
        .task;
    let mut outcomes = Vec::new();
    for entry in &plan.entries[..2] {
        let mut activations = vec![crate::evaluation::DebugActivation {
            request_baseline_merge: false,
            role: request.fast_role.clone(),
            writes: task.reference.clone(),
        }];
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            activations.push(crate::evaluation::DebugActivation {
                request_baseline_merge: false,
                role: request.fast_role.clone(),
                writes: Default::default(),
            });
        }
        let result = wb.evaluate_next_debug(&plan.id, &activations).unwrap();
        assert_eq!(result.state, "completed", "{:?}", result.error);
        assert!(result.independent_passed, "{:?}", result.acceptance);
        assert_eq!(result.evidence_kind, "scripted_debug");
        if entry.arm == crate::evaluation::EvaluationArm::Full {
            assert!(result.flow_completed);
            let worker =
                Workbench::open_evaluation_host(std::path::Path::new(&result.workspace)).unwrap();
            let stages: i64 = worker
                .db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM stage_runs WHERE state='done'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(stages, 2);
        }
        assert!(!std::path::Path::new(&result.workspace)
            .join(".hexagon/private-first-arm.txt")
            .exists());
        std::fs::write(
            std::path::Path::new(&result.workspace).join(".hexagon/private-first-arm.txt"),
            "must not cross arm boundary",
        )
        .unwrap();
        outcomes.push(result);
    }
    assert_ne!(outcomes[0].workspace, outcomes[1].workspace);
    assert_ne!(outcomes[0].id, outcomes[1].id);
    assert_eq!(outcomes[0].task_fingerprint, outcomes[1].task_fingerprint);
    let after = wb.evaluation_plan(&plan.id).unwrap();
    assert_eq!(
        after.entries.iter().filter(|e| e.run_id.is_some()).count(),
        2
    );
    assert_eq!(
        after
            .entries
            .iter()
            .filter(|e| e.state == crate::evaluation::PlannedState::Planned)
            .count(),
        6
    );
}
