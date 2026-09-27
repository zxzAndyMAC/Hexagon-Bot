use super::*;

#[test]
fn evaluation_report_preserves_unstarted_formal_coverage_as_insufficient() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = host
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Formal)
        .unwrap();
    let report = host.evaluation_benefit_report(&batch.id).unwrap();
    assert_eq!(report.version, 1);
    assert_eq!(report.batch_fingerprint, batch.fingerprint);
    let formal = report
        .groups
        .iter()
        .find(|g| g.kind == crate::evaluation::ReportGroupKind::Formal)
        .unwrap();
    assert_eq!(formal.runs.len(), 48);
    assert_eq!(formal.categories.len(), 4);
    for category in &formal.categories {
        assert_eq!(category.fast.planned, 6);
        assert_eq!(category.full.planned, 6);
        assert_eq!(category.fast.started, 0);
        assert_eq!(category.full.started, 0);
        assert_eq!(
            category.recommendation,
            crate::evaluation::BenefitRecommendation::InsufficientEvidence
        );
        assert_eq!(category.fast.cost_per_success_mc, None);
        assert_eq!(category.fast.human_median_ms, None);
    }
    assert!(formal
        .runs
        .iter()
        .all(|r| r.run_id.is_none() && r.independent_passed.is_none()));
    assert_eq!(
        serde_json::to_value(host.evaluation_plan(&plan.id).unwrap()).unwrap(),
        serde_json::to_value(plan).unwrap()
    );
    let text = report.markdown();
    assert!(text.contains("48"));
    assert!(text.contains("Cost / success"));
    assert!(text.contains("Human median"));
    assert!(text.contains("Runtime fingerprint"));
    assert!(text.contains("Pair details"));
}

fn measurements() -> Vec<crate::evaluation::BenefitRun> {
    use crate::evaluation::{BenefitRun, EvaluationArm, PlannedState, SafetyVerdict};
    let mut rows = Vec::new();
    for task in ["bug-a", "bug-b"] {
        for repetition in 1..=3 {
            for arm in [EvaluationArm::Fast, EvaluationArm::Full] {
                let full = arm == EvaluationArm::Full;
                rows.push(BenefitRun {
                    plan_id: "manual-fixture".into(),
                    position: rows.len(),
                    task_id: task.into(),
                    category: "bug".into(),
                    repetition,
                    arm,
                    state: PlannedState::Completed,
                    run_id: Some(format!("fixture-{}", rows.len())),
                    evidence_kind: Some("live_model".into()),
                    flow_completed: Some(full),
                    independent_passed: Some(true),
                    owner_exception: Some(false),
                    formal_success: true,
                    safety: Some(SafetyVerdict::Passed),
                    human_ms: Some(if full { 80 } else { 100 }),
                    active_ms: Some(if full { 1250 } else { 1000 }),
                    known_mc: Some(if full { 150 } else { 100 }),
                    unknown_mc: Some(0),
                    in_flight_mc: Some(0),
                    requests: Some(1),
                    outcome: None,
                    reasons: vec![],
                });
            }
        }
    }
    rows
}

#[test]
fn evaluation_report_joint_thresholds_match_manual_twenty_fifty_twentyfive_percent() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let summary = host.summarize_evaluation_measurements_fixture(&measurements());
    assert_eq!(summary.fast.cost_per_success_mc, Some(100.0));
    assert_eq!(summary.full.cost_per_success_mc, Some(150.0));
    assert_eq!(summary.fast.human_median_ms, Some(100.0));
    assert_eq!(summary.full.human_median_ms, Some(80.0));
    assert_eq!(summary.full.active_median_ms, Some(1250.0));
    assert_eq!(summary.paired_samples, 6);
    assert_eq!(
        summary.recommendation,
        crate::evaluation::BenefitRecommendation::ExploratoryFull
    );
}

#[test]
fn evaluation_report_keeps_failed_costs_and_partial_known_costs() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut rows = measurements();
    for row in &mut rows[..2] {
        row.state = crate::evaluation::PlannedState::Failed;
        row.formal_success = false;
        row.independent_passed = Some(false);
    }
    let summary = host.summarize_evaluation_measurements_fixture(&rows);
    assert_eq!(summary.fast.started, 6);
    assert_eq!(summary.fast.successes, 5);
    assert_eq!(summary.fast.known_mc, Some(600));
    assert_eq!(summary.full.known_mc, Some(900));
    assert_eq!(summary.fast.cost_per_success_mc, Some(120.0));
    assert_eq!(summary.full.cost_per_success_mc, Some(180.0));
    assert_eq!(summary.paired_samples, 5);
    assert_eq!(
        summary.pairs[0].exclusions,
        vec!["both_sides_not_successful"]
    );
    rows[0].known_mc = None;
    let unknown = host.summarize_evaluation_measurements_fixture(&rows);
    assert_eq!(
        unknown.fast.known_mc,
        Some(500),
        "missing one receipt must not erase the other five known costs"
    );
    assert!(!unknown.fast.costs_complete);
    assert_eq!(unknown.fast.cost_per_success_mc, None);
    assert_eq!(
        unknown.recommendation,
        crate::evaluation::BenefitRecommendation::InsufficientEvidence
    );
}

#[test]
fn evaluation_report_regeneration_preserves_originals_and_observation_history() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let batch = host
        .freeze_evaluation(&super::evaluation_config_tests::request(), None)
        .unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    host.arm_evaluation_service_failure_fixture(503);
    let run = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    host.stop_evaluation_plan(&plan.id).unwrap();
    host.supplement_evaluation_pair(&plan.id, 0).unwrap();
    let original = serde_json::to_value(host.evaluation_plan(&plan.id).unwrap()).unwrap();
    let history = serde_json::to_value(host.evaluation_outcome_history(&run.id).unwrap()).unwrap();
    let budget = serde_json::to_value(host.evaluation_budget_debug().unwrap()).unwrap();
    for _ in 0..2 {
        let report = host.evaluation_benefit_report(&batch.id).unwrap();
        let pilot = report
            .groups
            .iter()
            .find(|g| g.kind == crate::evaluation::ReportGroupKind::Pilot)
            .unwrap();
        let supplement = report
            .groups
            .iter()
            .find(|g| g.kind == crate::evaluation::ReportGroupKind::Supplement)
            .unwrap();
        assert_eq!(pilot.runs.len(), 8);
        assert_eq!(pilot.runs[0].known_mc, Some(150));
        assert_eq!(supplement.runs.len(), 2);
        assert!(supplement.runs.iter().all(|r| r.run_id.is_none()));
    }
    assert_eq!(
        serde_json::to_value(host.evaluation_outcome_history(&run.id).unwrap()).unwrap(),
        history
    );
    assert_eq!(
        serde_json::to_value(host.evaluation_budget_debug().unwrap()).unwrap(),
        budget
    );
    assert_eq!(
        serde_json::to_value(host.evaluation_plan(&plan.id).unwrap()).unwrap(),
        original
    );
    assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
    // D10: check both a same-task opposite arm and a different task.
    for position in [1, 2] {
        host.move_evaluation_report_binding_fixture(&plan.id, &run.id, position)
            .unwrap();
        let damaged = host.evaluation_benefit_report(&batch.id).unwrap();
        let pilot = damaged
            .groups
            .iter()
            .find(|g| g.kind == crate::evaluation::ReportGroupKind::Pilot)
            .unwrap();
        assert!(pilot.runs[position as usize]
            .reasons
            .contains(&"run_binding_corrupt".into()));
        assert!(!pilot.runs[position as usize].formal_success);
    }
}

#[test]
fn evaluation_report_stable_threshold_failure_and_variable_pairs_are_distinct() {
    use crate::evaluation::{BenefitRecommendation as Verdict, EvaluationArm};
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut rows = measurements();
    for row in &mut rows {
        if row.arm == EvaluationArm::Full {
            row.known_mc = Some(151);
        }
    }
    let stable = host.summarize_evaluation_measurements_fixture(&rows);
    assert_eq!(stable.recommendation, Verdict::NoDemonstratedAdvantage);
    assert!(stable
        .reasons
        .contains(&"cost_increase_over_fifty_percent".into()));
    rows[1].known_mc = Some(150);
    let fluctuating = host.summarize_evaluation_measurements_fixture(&rows);
    assert_eq!(fluctuating.recommendation, Verdict::InsufficientEvidence);
    assert!(fluctuating
        .reasons
        .contains(&"cost_direction_or_threshold_varies".into()));
}

#[test]
fn evaluation_report_missing_zero_and_overflow_cannot_produce_advantage() {
    use crate::evaluation::{BenefitRecommendation as Verdict, SafetyVerdict};
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    for mutation in 0..7 {
        let mut rows = measurements();
        match mutation {
            0 => rows[0].human_ms = None,
            1 => rows[0].human_ms = Some(0),
            2 => rows[0].unknown_mc = Some(1),
            3 => rows[0].safety = Some(SafetyVerdict::Unknown),
            4 => {
                rows.pop();
            }
            5 => {
                for row in &mut rows {
                    row.formal_success = false;
                }
            }
            _ => rows[0].known_mc = Some(u64::MAX),
        }
        assert_eq!(
            host.summarize_evaluation_measurements_fixture(&rows)
                .recommendation,
            Verdict::InsufficientEvidence,
            "mutation {mutation}"
        );
    }
}

#[test]
fn evaluation_report_failed_pair_cost_reversal_remains_visible() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut rows = measurements();
    for row in &mut rows[..2] {
        row.formal_success = false;
        row.independent_passed = Some(false);
        row.state = crate::evaluation::PlannedState::Failed;
    }
    rows[0].known_mc = Some(10_000);
    rows[1].known_mc = Some(1);
    let result = host.summarize_evaluation_measurements_fixture(&rows);
    assert_eq!(result.paired_samples, 5);
    assert_eq!(
        result.recommendation,
        crate::evaluation::BenefitRecommendation::InsufficientEvidence
    );
    assert!(result
        .reasons
        .contains(&"cost_direction_or_threshold_varies".into()));
}

proptest::proptest! {
    #[test]
    fn evaluation_report_removed_evidence_never_grants_advantage(index in 0usize..12, field in 0u8..5) {
        let home = tempfile::tempdir().unwrap();
        let host = Workbench::open_evaluation_host(home.path()).unwrap();
        let mut rows = measurements();
        match field {
            0 => rows[index].human_ms = None,
            1 => rows[index].active_ms = None,
            2 => rows[index].known_mc = None,
            3 => rows[index].safety = None,
            _ => rows[index].evidence_kind = None,
        }
        proptest::prop_assert_eq!(host.summarize_evaluation_measurements_fixture(&rows).recommendation, crate::evaluation::BenefitRecommendation::InsufficientEvidence);
    }
}
