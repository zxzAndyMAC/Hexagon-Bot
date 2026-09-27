use super::*;

#[test]
fn evaluation_supplement_does_not_retry_ordinary_failure_or_replace_originals() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = host
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.evaluate_next_debug(&plan.id, &[]).unwrap();
    host.evaluate_next_debug(&plan.id, &[]).unwrap();
    let original = host.evaluation_plan(&plan.id).unwrap();
    assert!(host.supplement_evaluation_pair(&plan.id, 0).is_err());
    assert_eq!(
        serde_json::to_value(host.evaluation_plan(&plan.id).unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}

#[test]
fn evaluation_supplement_is_one_whole_new_pair_with_original_costs_retained() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    host.arm_evaluation_service_failure_fixture(503);
    let first = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    assert_eq!(first.state, "failed");
    let second = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    let before = host.evaluation_plan(&plan.id).unwrap();
    let fees = host.evaluation_budget_debug().unwrap();
    assert_eq!(fees.known_mc, 150);
    let extra = host.supplement_evaluation_pair(&plan.id, 0).unwrap();
    assert_eq!(extra.entries.len(), 2);
    assert_eq!(extra.batch_fingerprint, plan.batch_fingerprint);
    assert_eq!(extra.supplement.as_ref().unwrap().plan_id, plan.id);
    assert!(host.supplement_evaluation_pair(&plan.id, 0).is_err());
    assert!(host.supplement_evaluation_pair(&extra.id, 0).is_err());
    let a = host.evaluate_next_debug(&extra.id, &[]).unwrap();
    let b = host.evaluate_next_debug(&extra.id, &[]).unwrap();
    let roots = std::collections::BTreeSet::from([
        &first.workspace,
        &second.workspace,
        &a.workspace,
        &b.workspace,
    ]);
    assert_eq!(roots.len(), 4);
    assert_eq!(
        serde_json::to_value(host.evaluation_plan(&plan.id).unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let after = host.evaluation_budget_debug().unwrap();
    assert_eq!(after.known_mc, 150);
    assert_eq!(after.runs.len(), 4);
    let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
    assert_eq!(reopened.evaluation_supplements(&plan.id).unwrap().len(), 1);
    assert!(reopened.supplement_evaluation_pair(&plan.id, 0).is_err());
}

#[test]
fn evaluation_supplement_interleaved_request_cannot_erase_terminal_failure() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = host
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.arm_interleaved_evaluation_service_failure_fixture();
    let run = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    assert_eq!(run.state, "failed");
    assert_eq!(
        host.evaluation_plan(&plan.id).unwrap().entries[0].state,
        crate::evaluation::PlannedState::Failed
    );
    host.stop_evaluation_plan(&plan.id).unwrap();
    let supplement = host.supplement_evaluation_pair(&plan.id, 0).unwrap();
    assert_eq!(supplement.supplement.unwrap().run_id, run.id);
}

#[test]
fn evaluation_supplement_missing_receipt_keeps_failure_without_granting_retry() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = host
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.arm_unbound_evaluation_service_failure_fixture();
    let run = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    assert_eq!(run.state, "failed");
    assert_eq!(
        host.evaluation_plan(&plan.id).unwrap().entries[0].state,
        crate::evaluation::PlannedState::Failed
    );
    host.stop_evaluation_plan(&plan.id).unwrap();
    assert!(host.supplement_evaluation_pair(&plan.id, 0).is_err());
    assert!(host.evaluation_supplements(&plan.id).unwrap().is_empty());
    assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
}
