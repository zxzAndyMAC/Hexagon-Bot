use super::*;
use serde_json::json;

#[test]
fn evaluation_debug_delivery_is_independently_checked_and_survives_reopen() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open(home.path(), "evaluation", &[], None).unwrap();
    let task = fixture();
    let result = wb
        .evaluate_debug(
            &task,
            &std::collections::BTreeMap::from([("answer.txt".into(), "correct".into())]),
        )
        .unwrap();
    assert!(result.independent_passed, "{result:#?}");
    assert!(!result.flow_completed);
    assert!(!result.owner_exception);
    assert_eq!(result.evidence_kind, "scripted_debug");
    assert!(!home.path().join("answer.txt").exists());
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&result.workspace).join("notes.txt")).unwrap(),
        "owner work"
    );
    drop(wb);
    let wb = Workbench::open(home.path(), "evaluation", &[], None).unwrap();
    let restored = wb.evaluation_result(&result.id).unwrap();
    assert_eq!(restored, result);
}

fn fixture() -> crate::evaluation::EvaluationTask {
    serde_json::from_value(json!({
        "id":"bug-01", "category":"bug", "source":"local fixture", "license":"MIT",
        "revision":"fixture-v1", "requirements":"Fix answer", "allowed_paths":["answer.txt"],
        "files":{"answer.txt":"wrong", "notes.txt":"owner work"},
        "validator":"accept.sh",
        "validation_files":{"accept.sh":"test \"$(cat answer.txt)\" = correct"},
        "reference":{"answer.txt":"correct"}, "wrong":{"answer.txt":"still wrong"}
    }))
    .unwrap()
}

#[test]
fn evaluation_completed_script_does_not_make_wrong_delivery_pass() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open(home.path(), "evaluation", &[], None).unwrap();
    let result = wb
        .evaluate_debug(&fixture(), &std::collections::BTreeMap::new())
        .unwrap();
    assert_eq!(result.state, "completed");
    assert!(!result.independent_passed);
    assert_eq!(result.acceptance.unwrap().exit_code, 1);
}

#[test]
fn evaluation_rejects_vacuous_acceptance_before_starting() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open(home.path(), "evaluation", &[], None).unwrap();
    let mut task = fixture();
    task.validation_files
        .insert("accept.sh".into(), "true".into());
    assert!(wb.evaluate_debug(&task, &task.reference).is_err());
    assert!(!home.path().join(".hexagon/evaluation-runs").exists());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn evaluation_cannot_materialize_traversal_or_host_state(name in "[a-z]{1,16}", prefix in proptest::sample::select(vec!["../", "/", ".hexagon/", ".git/", "src/../../"])) {
        let home = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(home.path(), &[], None).unwrap();
        let mut task = fixture();
        task.files.insert(format!("{prefix}{name}"), "overwrite".into());
        proptest::prop_assert!(wb.evaluate_debug(&task, &task.reference).is_err());
        proptest::prop_assert!(!home.path().join(".hexagon/evaluation-runs").exists());
    }
}

#[test]
fn evaluation_rejects_agent_writable_validator() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut task = fixture();
    task.allowed_paths.push("accept.sh".into());
    assert!(wb
        .evaluate_debug(
            &task,
            &std::collections::BTreeMap::from([("accept.sh".into(), "exit 0".into())])
        )
        .is_err());
}

#[cfg(unix)]
#[test]
fn evaluation_rejects_mode_changes_during_acceptance() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut task = fixture();
    task.validation_files.insert(
        "accept.sh".into(),
        "chmod +x answer.txt; test \"$(cat answer.txt)\" = correct".into(),
    );
    assert!(wb.evaluate_debug(&task, &task.reference).is_err());
}

#[test]
fn evaluation_bug_category_has_five_self_checked_cases() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let corpus =
        serde_json::from_str(include_str!("../../../../evaluation/corpus/bugs.json")).unwrap();
    let report = wb.check_evaluation_category(&corpus).unwrap();
    assert_eq!(report.cases.len(), 5);
    assert!(report.passed, "{report:#?}");
    assert_eq!(report.evidence_kind, "fixture_self_check");
    for case in report.cases {
        assert!(!case.initial.passed);
        assert!(case.reference.passed);
        assert!(!case.wrong.passed);
    }
}

#[test]
fn evaluation_category_rejects_duplicate_identity_or_wrong_split() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut corpus: crate::evaluation::EvaluationCorpus =
        serde_json::from_str(include_str!("../../../../evaluation/corpus/bugs.json")).unwrap();
    corpus.cases[1].task.id = corpus.cases[0].task.id.clone();
    assert!(wb.check_evaluation_category(&corpus).is_err());
    corpus.cases[1].task.id = "different".into();
    corpus.cases[0].split = "heldout".into();
    assert!(wb.check_evaluation_category(&corpus).is_err());
}

#[test]
fn evaluation_zero_exit_without_expected_result_is_not_success() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut value = serde_json::to_value(fixture()).unwrap();
    // Evaluation 02 review: delivered code could exit the validator before all
    // assertions. The host must require the actual declared result, not exit0.
    value["expected_stdout"] = json!("correct");
    value["validation_files"]["accept.sh"] = json!("cat answer.txt");
    let task = serde_json::from_value(value).unwrap();
    let result = wb
        .evaluate_debug(&task, &std::collections::BTreeMap::new())
        .unwrap();
    assert!(!result.independent_passed);
    assert_eq!(result.acceptance.unwrap().exit_code, 0);
}

#[test]
fn evaluation_bug_probe_rejects_early_exit_and_extra_output() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let corpus: crate::evaluation::EvaluationCorpus =
        serde_json::from_str(include_str!("../../../../evaluation/corpus/bugs.json")).unwrap();
    let mut task = corpus.cases[0].task.clone();
    for exit in ["import os; os._exit(0)", "raise SystemExit(0)"] {
        task.wrong.insert("pagination.py".into(), exit.into());
        let result = wb.evaluate_debug(&task, &task.wrong).unwrap();
        assert!(!result.independent_passed);
        assert_eq!(result.acceptance.unwrap().exit_code, 0);
    }
    let mut extra = task.reference.clone();
    extra
        .get_mut("pagination.py")
        .unwrap()
        .push_str("\nprint('extra unverified output')\n");
    let result = wb.evaluate_debug(&task, &extra).unwrap();
    assert!(!result.independent_passed);
}
