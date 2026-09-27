use super::*;

#[test]
fn evaluation_generation_gets_only_development_material_and_cannot_reset_exposure() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    let heldout = request.corpora[0]
        .cases
        .iter()
        .find(|c| c.split == "heldout")
        .unwrap()
        .task
        .id
        .clone();
    request.corpora[0]
        .cases
        .iter_mut()
        .find(|c| c.task.id == heldout)
        .unwrap()
        .task
        .dependencies
        .push("standard-library-only".into());
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let context = wb.prepare_evaluation_generation(&batch.id).unwrap();
    assert_eq!(context.task_ids.len(), 12);
    assert!(!context.task_ids.contains(&heldout));
    assert!(!context.tainted);
    assert!(context.eligible_for_generation);
    let materials =
        std::fs::read_to_string(std::path::Path::new(&context.workspace).join("development.json"))
            .unwrap();
    assert!(!materials.contains(&heldout));
    assert!(!materials.contains("expected_stdout"));
    assert!(!materials.contains("validation_files"));
    assert!(!materials.contains("\"reference\""));
    assert!(!materials.contains("\"wrong\""));
    let tainted = wb.reveal_evaluation_task(&context.id, &heldout).unwrap();
    assert!(tainted.tainted);
    assert!(!tainted.eligible_for_generation);
    let case = request.corpora[0]
        .cases
        .iter_mut()
        .find(|c| c.task.id == heldout)
        .unwrap();
    case.task.id = "renamed-heldout".into();
    case.task.revision = "changed-metadata".into();
    case.task.dependencies.reverse();
    let renamed = wb.freeze_evaluation(&request, Some(&batch.id)).unwrap();
    assert!(renamed
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::HeldoutRetired));
    let plan = wb
        .plan_evaluation(&renamed.id, crate::evaluation::PlanKind::Formal)
        .unwrap();
    assert!(wb.evaluate_next_debug(&plan.id, &[]).is_err());
    assert!(wb
        .evaluation_plan(&plan.id)
        .unwrap()
        .entries
        .iter()
        .all(|e| e.run_id.is_none()));
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    assert!(wb.evaluation_generation(&context.id).unwrap().tainted);
    assert!(wb
        .evaluation_batch(&renamed.id)
        .unwrap()
        .blocks
        .contains(&crate::evaluation::AdmissionBlock::HeldoutRetired));
}

#[test]
fn evaluation_hidden_material_is_blocked_at_tools_process_and_message_boundaries() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let report = wb.check_evaluation_isolation().unwrap();
    assert!(report.passed, "{report:#?}");
    assert!(report.public_requirements_readable);
    assert!(report.filesystem_denied);
    assert!(report.terminal_denied);
    assert!(report.child_process_denied);
    assert!(report.messages_clean);
    assert!(report.host_material_unchanged);
    drop(wb);
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(wb.evaluation_isolation_check(&report.id).unwrap()).unwrap()
    );
}
