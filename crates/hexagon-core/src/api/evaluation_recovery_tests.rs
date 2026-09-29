use super::*;

#[test]
fn evaluation_recovery_empty_host_does_not_invent_evidence() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let report = host.inspect_evaluation_recovery().unwrap();
    assert!(report.entries.is_empty());
    let again = Workbench::open_evaluation_host(home.path())
        .unwrap()
        .inspect_evaluation_recovery()
        .unwrap();
    assert!(again.entries.is_empty());
}

#[test]
fn evaluation_recovery_preserves_waiting_run_without_replay() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    let run = host
        .evaluate_next_with_owner_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: request.fast_role,
                writes: Default::default(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    let before = host.evaluation_budget_debug().unwrap();
    drop(host);
    for _ in 0..2 {
        let host = Workbench::open_evaluation_host(home.path()).unwrap();
        let recovery = host.inspect_evaluation_recovery().unwrap();
        let entry = recovery
            .entries
            .iter()
            .find(|e| e.run_id == run.id)
            .unwrap();
        assert_eq!(entry.state, crate::evaluation::RecoveryState::WaitingHuman);
        assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
        let after = host.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, before.requests);
        assert_eq!(after.unknown_mc, before.unknown_mc);
    }
}

#[test]
fn evaluation_recovery_never_reclaims_a_live_driver_or_replays_a_dead_one() {
    let (home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    let driver = host.hold_evaluation_driver_fixture(&run.id).unwrap();
    let peer = Workbench::open_evaluation_host(home.path()).unwrap();
    let before = peer.evaluation_budget_debug().unwrap();
    assert_eq!(
        peer.inspect_evaluation_recovery().unwrap().entries[0].state,
        crate::evaluation::RecoveryState::Running
    );
    assert!(peer.reconcile_evaluation_run(&run.id).is_err());
    drop(driver);
    assert_eq!(
        peer.inspect_evaluation_recovery().unwrap().entries[0].state,
        crate::evaluation::RecoveryState::NeedsReconciliation
    );
    for _ in 0..2 {
        peer.reconcile_evaluation_run(&run.id).unwrap();
        assert_eq!(peer.evaluation_result(&run.id).unwrap().state, "incomplete");
        let after = peer.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, before.requests);
        assert_eq!(after.known_mc, before.known_mc);
        assert_eq!(after.unknown_mc, before.unknown_mc);
    }
}

#[test]
fn evaluation_recovery_lost_attention_stays_unknown_and_releases_owner_slot() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    let run = host
        .evaluate_next_with_owner_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: request.fast_role,
                writes: Default::default(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    let attention = host
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    assert!(
        host.reconcile_evaluation_run(&run.id).is_err(),
        "a live owner handle must not be reclaimed"
    );
    drop(attention);
    assert_eq!(
        host.inspect_evaluation_recovery().unwrap().entries[0].state,
        crate::evaluation::RecoveryState::NeedsReconciliation
    );
    host.reconcile_evaluation_run(&run.id).unwrap();
    let timing = host.evaluation_timing(&run.id).unwrap();
    assert!(!timing.timing_complete);
    assert_eq!(timing.intervals[0].duration_ms, None);
    assert_eq!(
        timing.intervals[0].end_reason.as_deref(),
        Some("owner_handle_lost")
    );
    assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
    let next = host
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    host.end_evaluation_attention(&next, crate::evaluation::AttentionEnd::Away)
        .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "api::evaluation_recovery_tests::evaluation_recovery_crash_child",
            "--nocapture",
        ])
        .env("HEXAGON_TEST_EVALUATION_CRASH_ROOT", home.path())
        .env("HEXAGON_TEST_EVALUATION_CRASH_POINT", "attention")
        .env("HEXAGON_TEST_EVALUATION_CRASH_RUN", &run.id)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    host.reconcile_evaluation_run(&run.id).unwrap();
    let timing = host.evaluation_timing(&run.id).unwrap();
    let last = timing.intervals.last().unwrap();
    assert_eq!(last.duration_ms, None);
    assert_eq!(last.end_reason.as_deref(), Some("owner_handle_lost"));
    assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
}

#[test]
fn evaluation_recovery_crash_child() {
    let Some(root) = std::env::var_os("HEXAGON_TEST_EVALUATION_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let point = std::env::var("HEXAGON_TEST_EVALUATION_CRASH_POINT").unwrap();
    let run_id = std::env::var("HEXAGON_TEST_EVALUATION_CRASH_RUN").unwrap();
    let host = Workbench::open_evaluation_host(&root).unwrap();
    if point == "claimed" || point == "result" {
        host.arm_evaluation_crash_fixture(&point).unwrap();
        let _ = host.evaluate_next_debug(&run_id, &[]);
        panic!("claim checkpoint must exit");
    }
    if point == "attention" {
        let _attention = host
            .begin_evaluation_attention(&run_id, crate::evaluation::EvaluationActor::Scripted)
            .unwrap();
        std::process::exit(86);
    }
    let run = host.evaluation_result(&run_id).unwrap();
    let _owner = host.hold_evaluation_driver_fixture(&run_id).unwrap();
    struct Receipt(std::path::PathBuf, bool);
    impl crate::provider::ModelProvider for Receipt {
        fn is_scripted(&self) -> bool {
            true
        }
        fn complete(
            &self,
            _: &crate::provider::ChatRequest,
        ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
            std::fs::write(self.0.join("request-reached-peer"), "one request").unwrap();
            // 2026-09-29 acceptance: the parent kills this process while the
            // provider owns the request, before any response or receipt exists.
            if self.1 {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            Ok(crate::provider::ChatResponse {
                content: vec![crate::provider::ContentBlock::Text {
                    text: "response".into(),
                }],
                stop: crate::provider::StopReason::EndTurn,
                usage: crate::provider::Usage {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    prompt_reported: true,
                    completion_reported: true,
                    ..Default::default()
                },
            })
        }
    }
    let mut worker = Workbench::open_evaluation_host(Path::new(&run.workspace)).unwrap();
    worker.register_provider("default", Arc::new(Receipt(root, point == "inflight")));
    if point != "inflight" {
        worker.arm_evaluation_crash_fixture(&point).unwrap();
    }
    let _ = worker.draft_role_def("a0", "crash receipt boundary fixture");
    panic!("crash boundary must exit");
}

#[test]
#[cfg(unix)]
fn evaluation_recovery_sigkill_inflight_preserves_unknown_without_replay() {
    use std::os::unix::process::ExitStatusExt;
    let (home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    let before = host.evaluation_budget_debug().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "api::evaluation_recovery_tests::evaluation_recovery_crash_child",
            "--nocapture",
        ])
        .env("HEXAGON_TEST_EVALUATION_CRASH_ROOT", home.path())
        .env("HEXAGON_TEST_EVALUATION_CRASH_POINT", "inflight")
        .env("HEXAGON_TEST_EVALUATION_CRASH_RUN", &run.id)
        .spawn()
        .unwrap();
    let marker = home.path().join("request-reached-peer");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Reap even on timeout so a failed acceptance never leaves a hanging child.
    let reached = marker.exists();
    let killed = child.kill();
    let status = child.wait().unwrap();
    assert!(reached, "provider was never reached before the deadline");
    killed.unwrap();
    assert_eq!(status.signal(), Some(9));
    drop(host);
    for _ in 0..2 {
        let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
        reopened.reconcile_evaluation_run(&run.id).unwrap();
        let after = reopened.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, before.requests + 1);
        assert_eq!(after.known_mc, before.known_mc);
        assert_eq!(after.unknown_mc, before.unknown_mc + 3000);
        assert_eq!(after.in_flight_mc, 0);
        assert!(after.blocked);
        assert_eq!(
            reopened.evaluation_result(&run.id).unwrap().state,
            "incomplete"
        );
        assert!(
            !reopened
                .evaluation_control(&run.id)
                .unwrap()
                .cleanup_confirmed
        );
    }
}

#[test]
fn evaluation_recovery_process_exit_keeps_reservations_and_receipts_once() {
    for point in ["reserved", "returned", "persisted"] {
        let (home, host, run) = super::evaluation_budget_tests::waiting_budget(
            80,
            500000,
            super::evaluation_budget_tests::fixture_price(),
        );
        let before = host.evaluation_budget_debug().unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "api::evaluation_recovery_tests::evaluation_recovery_crash_child",
                "--nocapture",
            ])
            .env("HEXAGON_TEST_EVALUATION_CRASH_ROOT", home.path())
            .env("HEXAGON_TEST_EVALUATION_CRASH_POINT", point)
            .env("HEXAGON_TEST_EVALUATION_CRASH_RUN", &run.id)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(
            home.path().join("request-reached-peer").exists(),
            point != "reserved"
        );
        if point == "reserved" {
            let path = Path::new(&run.workspace).join(".hexagon/evaluation-budget.json");
            let binding = std::fs::read(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            assert_eq!(
                host.inspect_evaluation_recovery().unwrap().entries[0].state,
                crate::evaluation::RecoveryState::Corrupt
            );
            assert!(host.reconcile_evaluation_run(&run.id).is_err());
            assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
            std::fs::write(path, binding).unwrap();
        }
        for _ in 0..2 {
            let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
            reopened.reconcile_evaluation_run(&run.id).unwrap();
            let after = reopened.evaluation_budget_debug().unwrap();
            assert_eq!(after.requests, before.requests + 1);
            assert_eq!(
                after.in_flight_mc, 0,
                "lost owner must become unknown, not stay falsely in flight"
            );
            if point == "persisted" {
                assert_eq!(after.known_mc, before.known_mc + 150);
                assert_eq!(after.unknown_mc, before.unknown_mc);
            } else {
                assert_eq!(after.known_mc, before.known_mc);
                assert_eq!(after.unknown_mc, before.unknown_mc + 3000);
            }
            let result = reopened.evaluation_result(&run.id).unwrap();
            assert_eq!(result.state, "incomplete");
            assert!(!result.flow_completed);
            assert!(
                !reopened
                    .evaluation_control(&run.id)
                    .unwrap()
                    .cleanup_confirmed
            );
            assert_eq!(reopened.evaluation_timing(&run.id).unwrap().active_ms, None);
        }
    }
}

#[test]
fn evaluation_recovery_claim_exit_preserves_original_missing_run() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "api::evaluation_recovery_tests::evaluation_recovery_crash_child",
            "--nocapture",
        ])
        .env("HEXAGON_TEST_EVALUATION_CRASH_ROOT", home.path())
        .env("HEXAGON_TEST_EVALUATION_CRASH_POINT", "claimed")
        .env("HEXAGON_TEST_EVALUATION_CRASH_RUN", &plan.id)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let claimed = host.evaluation_plan(&plan.id).unwrap();
    let run = claimed.entries[0].run_id.as_ref().unwrap();
    assert!(host.evaluation_result(run).is_err());
    let before = host.evaluation_budget_debug().unwrap();
    for _ in 0..2 {
        let recovered = host.reconcile_evaluation_run(run).unwrap();
        assert_eq!(
            recovered.state,
            crate::evaluation::RecoveryState::MissingRun
        );
        assert!(
            host.evaluation_result(run).is_err(),
            "recovery must not fabricate a result"
        );
        assert_eq!(
            host.evaluation_plan(&plan.id).unwrap().entries[0].state,
            crate::evaluation::PlannedState::Incomplete
        );
        let after = host.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, 0);
        assert_eq!(after.reserved_mc, before.reserved_mc);
    }
}

#[test]
fn evaluation_recovery_rejects_corrupt_worker_binding_without_changing_result() {
    let (_home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    std::fs::write(
        Path::new(&run.workspace).join(".hexagon/evaluation-control.json"),
        b"{broken binding",
    )
    .unwrap();
    let observation = host.inspect_evaluation_recovery().unwrap();
    assert_eq!(
        observation.entries[0].state,
        crate::evaluation::RecoveryState::Corrupt
    );
    assert!(host.reconcile_evaluation_run(&run.id).is_err());
    assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
}

#[test]
fn evaluation_recovery_result_exit_rolls_back_all_terminal_facts() {
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
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "api::evaluation_recovery_tests::evaluation_recovery_crash_child",
            "--nocapture",
        ])
        .env("HEXAGON_TEST_EVALUATION_CRASH_ROOT", home.path())
        .env("HEXAGON_TEST_EVALUATION_CRASH_POINT", "result")
        .env("HEXAGON_TEST_EVALUATION_CRASH_RUN", &plan.id)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let entry = host.evaluation_plan(&plan.id).unwrap().entries[0].clone();
    let id = entry.run_id.unwrap();
    assert_eq!(entry.state, crate::evaluation::PlannedState::Started);
    assert_eq!(
        host.evaluation_result(&id).unwrap().state,
        "started",
        "uncommitted terminal evidence must roll back"
    );
    let before = host.evaluation_budget_debug().unwrap();
    for _ in 0..2 {
        host.reconcile_evaluation_run(&id).unwrap();
        assert_eq!(host.evaluation_result(&id).unwrap().state, "incomplete");
        assert_eq!(
            host.evaluation_plan(&plan.id).unwrap().entries[0].state,
            crate::evaluation::PlannedState::Incomplete
        );
        let after = host.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, before.requests);
        assert_eq!(after.unknown_mc, before.unknown_mc);
    }
}

#[test]
fn evaluation_recovery_repairs_legacy_terminal_links_without_rewriting_result() {
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
    let run = host.evaluate_next_debug(&plan.id, &[]).unwrap();
    let before = host.evaluation_budget_debug().unwrap();
    host.break_evaluation_terminal_links_fixture(&run.id)
        .unwrap();
    for _ in 0..2 {
        host.reconcile_evaluation_run(&run.id).unwrap();
        assert_eq!(host.evaluation_result(&run.id).unwrap(), run);
        assert_ne!(
            host.evaluation_plan(&plan.id).unwrap().entries[0].state,
            crate::evaluation::PlannedState::Started
        );
        assert_eq!(
            host.evaluation_control(&run.id).unwrap().state,
            crate::evaluation::EvaluationControlState::Ended
        );
        let after = host.evaluation_budget_debug().unwrap();
        assert_eq!(after.requests, before.requests);
        assert_eq!(after.known_mc, before.known_mc);
        assert_eq!(after.unknown_mc, before.unknown_mc);
    }
}
