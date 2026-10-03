//! Quality behavior through the existing Workbench evidence/acceptance seam.
use super::*;

fn fixture(contracts: &[(&str, &str)]) -> (tempfile::TempDir, Workbench) {
    let checks: Vec<_> = contracts.iter().map(|(_, cmd)| *cmd).collect();
    let quality_checks: serde_json::Map<String, serde_json::Value> = contracts
        .iter()
        .map(|(kind, cmd)| (kind.to_string(), json!(cmd)))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"quality fixture","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":checks,"quality_checks":quality_checks,"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    (dir, wb)
}

#[test]
fn changed_ui_requires_real_tests_accessibility_and_performance_evidence() {
    let (dir, wb) = fixture(&[]);
    std::fs::create_dir(dir.path().join("ui")).unwrap();
    std::fs::write(
        dir.path().join("ui/page.tsx"),
        "export const Page = () => <button>Run</button>",
    )
    .unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    for key in [
        "quality:tests",
        "quality:accessibility",
        "quality:performance",
    ] {
        assert!(
            evidence
                .missing
                .iter()
                .any(|m| m == &format!("check:{key}")),
            "missing {key}: {evidence:?}"
        );
    }
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
}

#[test]
fn declared_quality_runners_produce_real_evidence_and_retest_regressions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tests.sh"), "exit 0\n").unwrap();
    std::fs::write(dir.path().join("accessibility.sh"), "exit 0\n").unwrap();
    std::fs::write(
        dir.path().join("performance.sh"),
        "sleep \"$(cat duration)\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("duration"), "0.01").unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"quality fixture","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":["sh tests.sh","sh accessibility.sh","sh performance.sh"],"quality_checks":{"tests":"sh tests.sh","accessibility":"sh accessibility.sh","performance":"sh performance.sh"},"stamp_point":true}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    let page = dir.path().join("page.tsx");
    std::fs::write(&page, "export const Page = () => <button>Run</button>").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Passed,
        "{evidence:?}"
    );
    assert_eq!(
        performance.quality.as_ref().unwrap().reason,
        "baseline_established"
    );
    assert!(performance
        .quality
        .as_ref()
        .unwrap()
        .baseline_event_id
        .is_none());
    eprintln!("quality baseline evidence: {:?}", performance.quality);
    let baseline_event = performance.event_id;

    std::fs::write(&page, "export const Page = () => <button>Slow</button>").unwrap();
    std::fs::write(dir.path().join("duration"), "2.0").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Failed,
        "{evidence:?}"
    );
    assert_eq!(performance.quality.as_ref().unwrap().reason, "regressed");
    eprintln!("quality regression evidence: {:?}", performance.quality);
    assert_eq!(
        performance.quality.as_ref().unwrap().baseline_event_id,
        baseline_event
    );
    assert!(evidence
        .missing
        .contains(&"check:quality:performance".into()));

    std::fs::write(&page, "export const Page = () => <button>Fixed</button>").unwrap();
    std::fs::write(dir.path().join("duration"), "0.001").unwrap();
    wb.run_checks().unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let performance = evidence
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(
        performance.state,
        orchestra::CheckState::Passed,
        "{evidence:?}"
    );
    assert_eq!(performance.quality.as_ref().unwrap().reason, "passed");
    eprintln!("quality repair evidence: {:?}", performance.quality);
    assert!(evidence.missing.is_empty(), "{evidence:?}");

    // A changed benchmark definition cannot inherit the old comparison pass.
    std::fs::write(
        dir.path().join("performance.sh"),
        "sleep \"$(cat duration)\"\n# changed workload definition\n",
    )
    .unwrap();
    wb.run_checks().unwrap();
    let changed = wb.stage_evidence().unwrap().unwrap();
    let performance = changed
        .checks
        .iter()
        .find(|c| c.cmd == "quality:performance")
        .unwrap();
    assert_eq!(performance.state, orchestra::CheckState::Failed);
    assert_eq!(
        performance.quality.as_ref().unwrap().reason,
        "measurement_changed"
    );
}

#[test]
fn automatic_checkpoint_coalesces_unchanged_attempts_and_retests_changed_code() {
    let (dir, wb) = fixture(&[("tests", "sh tests.sh")]);
    std::fs::write(dir.path().join("tests.sh"), "exit 0\n").unwrap();
    let file = dir.path().join("math.rs");
    std::fs::write(&file, "fn sum() {}\n").unwrap();
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::AwaitingStamp { .. }
    ));
    let first = wb.stage_evidence().unwrap().unwrap();
    let first_id = first
        .checks
        .iter()
        .find(|c| c.cmd == "quality:tests")
        .unwrap()
        .event_id;
    wb.advance().unwrap();
    let same = wb.stage_evidence().unwrap().unwrap();
    assert_eq!(
        first_id,
        same.checks
            .iter()
            .find(|c| c.cmd == "quality:tests")
            .unwrap()
            .event_id
    );
    std::fs::write(&file, "fn sum() { let _ = 1; }\n").unwrap();
    wb.advance().unwrap();
    let changed = wb.stage_evidence().unwrap().unwrap();
    assert_ne!(
        first_id,
        changed
            .checks
            .iter()
            .find(|c| c.cmd == "quality:tests")
            .unwrap()
            .event_id
    );
}

#[test]
fn missing_quality_evidence_needs_explicit_versioned_owner_exception() {
    let (dir, wb) = fixture(&[]);
    // Migration compatibility fixture: old active projects have no before-work inventory.
    wb.db
        .conn()
        .execute("DELETE FROM quality_baselines", [])
        .unwrap();
    std::fs::write(dir.path().join("logic.rs"), "fn work() {}\n").unwrap();
    let evidence = wb.stage_evidence().unwrap().unwrap();
    let fingerprint = evidence.fingerprint.unwrap();
    let selected: Vec<_> = evidence
        .exceptions
        .iter()
        .map(|c| c.requirement.clone())
        .collect();
    assert!(!selected.is_empty());
    let question = wb
        .request_acceptance_exception(&fingerprint)
        .unwrap()
        .question_id;
    assert!(wb
        .accept_delivery_exception(&question, &fingerprint, &selected, " ")
        .is_err());
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::Incomplete { .. }
    ));
    wb.accept_delivery_exception(
        &question,
        &fingerprint,
        &selected,
        "Owner accepts missing runner for this prototype",
    )
    .unwrap();
    let accepted = wb.stage_evidence().unwrap().unwrap();
    assert!(accepted
        .checks
        .iter()
        .all(|c| c.state == orchestra::CheckState::Missing));
    assert!(accepted.missing.is_empty());
    assert!(matches!(
        wb.advance().unwrap(),
        orchestra::StageAction::AwaitingStamp { .. }
    ));
    std::fs::write(dir.path().join("logic.rs"), "fn work() { todo!() }\n").unwrap();
    assert!(!wb.stage_evidence().unwrap().unwrap().missing.is_empty());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 16, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/quality-gates.txt"))), ..Default::default() })]
    #[test]
    fn quality_scope_and_missing_evidence_cannot_be_overridden_by_agent_claims(index in 0usize..4, claim in "[a-zA-Z0-9 ]{0,120}") {
        let (dir, wb) = fixture(&[]);
        let (path, body) = [("security-notes.md", "Notes"), ("logic.rs", "fn work() {}"), ("page.tsx", "<button>Run</button>"), ("auth.rs", "fn check() {}")][index];
        std::fs::write(dir.path().join(path), body).unwrap();
        wb.db.append_event(&wb.project_id, EventKind::AgentMessage, json!({"all_checks_passed":true,"claim":claim}), None, None).unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        let has = |key: &str| evidence.checks.iter().any(|c| c.cmd == key);
        proptest::prop_assert_eq!(has("quality:tests"), index != 0);
        proptest::prop_assert_eq!(has("quality:accessibility"), index == 2);
        proptest::prop_assert_eq!(has("quality:performance"), index == 2);
        proptest::prop_assert_eq!(has("quality:security"), index == 3);
        proptest::prop_assert!(evidence.checks.iter().all(|c| c.state == orchestra::CheckState::Missing));
        proptest::prop_assert_eq!(evidence.missing.is_empty(), index == 0);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config { cases: 4, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("proptest-regressions/quality-performance.txt"))), ..Default::default() })]
    #[test]
    fn unsuccessful_performance_runs_never_establish_a_passing_baseline(exit in 1u8..64) {
        let (dir, wb) = fixture(&[("performance", "sh performance.sh")]);
        std::fs::write(dir.path().join("page.tsx"), "<button>Run</button>").unwrap();
        std::fs::write(dir.path().join("performance.sh"), format!("exit {exit}\n")).unwrap();
        wb.run_checks().unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        let performance = evidence.checks.iter().find(|c| c.cmd == "quality:performance").unwrap();
        proptest::prop_assert_eq!(&performance.state, &orchestra::CheckState::Failed);
        proptest::prop_assert_eq!(performance.exit_code, Some(i32::from(exit)));
        proptest::prop_assert_eq!(&performance.quality.as_ref().unwrap().reason, "execution_failed");
        proptest::prop_assert!(evidence.missing.contains(&"check:quality:performance".into()));
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(8))]
    #[test]
    fn unrelated_command_words_never_declare_a_quality_contract(word in proptest::sample::select(vec!["tests", "performance", "accessibility", "security"])) {
        let dir = tempfile::tempdir().unwrap();
        let command = format!("printf '%s' {word}");
        let pack: PackDef = serde_json::from_value(json!({"name":"untrusted labels","version":1,"stages":[{"name":"delivery","roles":["dev"],"due":[],"checks":[command],"stamp_point":true}]})).unwrap();
        let wb = Workbench::for_test(dir.path(), &["dev"], Some(pack)).unwrap();
        wb.open_stage(0).unwrap();
        std::fs::write(dir.path().join("page.tsx"), "export const page = 1").unwrap();
        wb.run_checks().unwrap();
        let evidence = wb.stage_evidence().unwrap().unwrap();
        for category in ["tests", "performance", "accessibility"] {
            proptest::prop_assert!(evidence.missing.iter().any(|key| key == &format!("check:quality:{category}")), "{evidence:?}");
        }
    }
}
