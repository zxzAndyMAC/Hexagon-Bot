//! Ticket11 draft: register only after ticket10 delivery, run RED before implementation.
use super::*;

#[test]
fn evaluation_budget_counts_actual_scripted_requests_in_a_shared_debug_round() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let paid_before = serde_json::to_value(wb.evaluation_budget().unwrap()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    wb.enable_evaluation_budget_debug(
        &plan.id,
        crate::evaluation::DebugPrice {
            prompt_per_1k_mc: 1000,
            completion_per_1k_mc: 1000,
            prompt_bound: 2000,
            output_bound: 1000,
        },
    )
    .unwrap();
    let task = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == plan.entries[0].task_id)
        .unwrap()
        .task;
    let run = wb
        .evaluate_next_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: request.fast_role.clone(),
                writes: task.reference.clone(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    let budget = wb.evaluation_budget_debug().unwrap();
    assert_eq!(budget.evidence_kind, "scripted_debug");
    assert!(
        budget.requests >= 2,
        "planning and the actual turn must both enter the shared ledger"
    );
    assert!(
        budget.unknown_mc > 0,
        "missing supplier usage retains its reservation"
    );
    assert!(budget.reserved_mc >= 2 * request.limits.run_mc - budget.known_mc);
    assert_eq!(
        budget.runs.len(),
        2,
        "both arms reserved before the first arm started"
    );
    assert!(budget
        .runs
        .iter()
        .any(|r| r.run_id.as_deref() == Some(&run.id)));
    drop(wb);
    let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
    let restored = reopened.evaluation_budget_debug().unwrap();
    assert_eq!(restored.requests, budget.requests);
    assert_eq!(restored.unknown_mc, budget.unknown_mc);
    // Scripted fixture charges never consume the actual approved paid round.
    assert_eq!(
        serde_json::to_value(reopened.evaluation_budget().unwrap()).unwrap(),
        paid_before
    );
}

pub(super) fn waiting_budget(
    request_limit: u32,
    run_mc: u64,
    price: crate::evaluation::DebugPrice,
) -> (
    tempfile::TempDir,
    Workbench,
    crate::evaluation::EvaluationResult,
) {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::control_only_request();
    request.limits.requests = request_limit;
    request.limits.run_mc = run_mc;
    let batch = wb.freeze_evaluation(&request, None).unwrap();
    let plan = wb
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    wb.enable_evaluation_budget_debug(&plan.id, price).unwrap();
    let run = wb
        .evaluate_next_with_owner_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: request.fast_role,
                writes: Default::default(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    // D09: native budget probes now need an explicitly active execution phase.
    let run = wb.resume_evaluation_fixture(&run.id).unwrap();
    (home, wb, run)
}
pub(super) fn fixture_price() -> crate::evaluation::DebugPrice {
    crate::evaluation::DebugPrice {
        prompt_per_1k_mc: 1000,
        completion_per_1k_mc: 1000,
        prompt_bound: 2000,
        output_bound: 1000,
    }
}
fn fixture_reply() -> crate::provider::ChatResponse {
    crate::provider::ChatResponse {
        content: vec![crate::provider::ContentBlock::Text {
            text: "fixture reply".into(),
        }],
        stop: crate::provider::StopReason::EndTurn,
        usage: Default::default(),
    }
}

#[test]
fn evaluation_budget_four_workers_share_eighty_request_ceiling() {
    let (_home, host, run) = waiting_budget(80, 500000, fixture_price());
    assert_eq!(run.state, "started");
    let initial = host.evaluation_budget_debug().unwrap().requests;
    let provider = Arc::new(crate::provider::ScriptedProvider::new(
        (0..100).map(|_| fixture_reply()).collect(),
    ));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let root = run.workspace.clone();
            let provider = provider.clone();
            std::thread::spawn(move || {
                let mut worker = Workbench::open_evaluation_host(Path::new(&root)).unwrap();
                worker.register_provider("default", provider);
                let mut admitted = 0;
                for _ in 0..80 {
                    if worker.draft_role_def("a0", "bounded fixture").is_err() {
                        break;
                    }
                    admitted += 1;
                }
                admitted
            })
        })
        .collect();
    let completed: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    // D09: at the limit, admitted replies can be interrupted before delivery.
    // Supplier dispatches, not successful API returns, prove no eighty-first IO.
    assert!(completed as u64 + initial <= 80);
    assert_eq!(
        provider.recorded().len() as u64 + initial,
        80,
        "no eighty-first IO"
    );
    let report = host.evaluation_budget_debug().unwrap();
    assert_eq!(report.requests, 80);
    assert_eq!(report.unknown_mc, 240000);
    assert_eq!(
        report.known_mc + report.reserved_mc + report.available_mc,
        report.limit_mc
    );
}
#[test]
fn evaluation_budget_money_exhaustion_keeps_unknown_reservations_on_reopen() {
    let (home, host, run) = waiting_budget(80, 6500, fixture_price());
    assert_eq!(run.state, "started");
    let mut worker = Workbench::open_evaluation_host(Path::new(&run.workspace)).unwrap();
    let provider = Arc::new(crate::provider::ScriptedProvider::new(
        vec![fixture_reply()],
    ));
    worker.register_provider("default", provider.clone());
    assert!(worker.draft_role_def("a0", "must refuse").is_err());
    assert_eq!(provider.recorded().len(), 0);
    let before = host.evaluation_budget_debug().unwrap();
    assert_eq!(before.requests, 2);
    assert_eq!(before.unknown_mc, 6000);
    drop(host);
    let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
    let after = reopened.evaluation_budget_debug().unwrap();
    assert_eq!(after.requests, before.requests);
    assert_eq!(after.available_mc, before.available_mc);
    assert_eq!(after.unknown_mc, before.unknown_mc);
}

#[test]
fn evaluation_budget_debug_refuses_real_provider_but_ordinary_unknown_price_continues() {
    struct NetworkLike(std::sync::atomic::AtomicUsize);
    impl crate::provider::ModelProvider for NetworkLike {
        fn complete(
            &self,
            _: &crate::provider::ChatRequest,
        ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(fixture_reply())
        }
    }
    let (_home, host, run) = waiting_budget(80, 500000, fixture_price());
    let provider = Arc::new(NetworkLike(std::sync::atomic::AtomicUsize::new(0)));
    let mut worker = Workbench::open_evaluation_host(Path::new(&run.workspace)).unwrap();
    worker.register_provider("default", provider.clone());
    assert!(worker.draft_role_def("a0", "must not send").is_err());
    assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(host.evaluation_budget_debug().unwrap().requests, 2);
    let ordinary = tempfile::tempdir().unwrap();
    let mut normal = Workbench::for_test(ordinary.path(), &["后端"], None).unwrap();
    normal.register_provider("default", provider.clone());
    assert!(normal
        .draft_role_def("a0", "ordinary unknown-price behavior")
        .is_ok());
    assert_eq!(provider.0.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn evaluation_budget_overflow_receipt_freezes_further_dispatch() {
    let (_home, host, run) = waiting_budget(80, 500000, fixture_price());
    let mut worker = Workbench::open_evaluation_host(Path::new(&run.workspace)).unwrap();
    let mut reply = fixture_reply();
    reply.usage = crate::provider::Usage {
        observed_model: None,
        prompt_tokens: u64::MAX,
        completion_tokens: u64::MAX,
        prompt_reported: true,
        completion_reported: true,
        unpriced: false,
    };
    let provider = Arc::new(crate::provider::ScriptedProvider::new(vec![
        reply,
        fixture_reply(),
    ]));
    worker.register_provider("default", provider.clone());
    let before = host.evaluation_budget_debug().unwrap();
    // D09: settlement persists first, then a blocked receipt interrupts work.
    assert!(worker.draft_role_def("a0", "overflow receipt").is_err());
    let after = host.evaluation_budget_debug().unwrap();
    assert!(after.blocked);
    assert!(after.unknown_mc >= before.unknown_mc + 3000);
    assert!(after.available_mc <= before.available_mc);
    assert!(worker.draft_role_def("a0", "must stop").is_err());
    assert_eq!(provider.recorded().len(), 1);
}

#[test]
fn evaluation_budget_paid_authority_does_not_reset_when_host_changes() {
    let root = tempfile::tempdir().unwrap();
    let authority = root.path().join("isolated-paid-fixture.db");
    let mut previous = None;
    for index in 0..3 {
        let host =
            Workbench::open_evaluation_host(&root.path().join(format!("host-{index}"))).unwrap();
        let batch = host
            .freeze_evaluation(&super::evaluation_config_tests::request(), None)
            .unwrap();
        let plan = host
            .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
            .unwrap();
        let result = host.reserve_evaluation_paid_fixture(&authority, &plan.id, &fixture_price());
        if index < 2 {
            let report = result.unwrap();
            assert_eq!(report.evidence_kind, "paid_first_round");
            assert_eq!(report.pilot_exposure_mc, (index + 1) * 1000000);
            assert_eq!(report.known_mc, 0, "parent reservation is not spend");
            let repeat = host
                .reserve_evaluation_paid_fixture(&authority, &plan.id, &fixture_price())
                .unwrap();
            assert_eq!(
                repeat.reserved_mc, report.reserved_mc,
                "same pair is idempotent"
            );
            previous = Some((host, plan));
        } else {
            assert!(
                result.is_err(),
                "a third host cannot reset the shared pilot cap"
            );
        }
    }
    let (host, plan) = previous.unwrap();
    assert_eq!(
        host.reserve_evaluation_paid_fixture(&authority, &plan.id, &fixture_price())
            .unwrap()
            .pilot_exposure_mc,
        2000000
    );
}

#[test]
fn evaluation_budget_duplicate_receipt_is_idempotent_and_conflict_keeps_exposure() {
    let (_home, host, _run) = waiting_budget(80, 500000, fixture_price());
    let before = host.evaluation_budget_debug().unwrap();
    let key = &before.runs.iter().find(|r| r.requests > 0).unwrap().key;
    host.redeliver_evaluation_receipt_fixture(key, &Default::default())
        .unwrap();
    let duplicate = host.evaluation_budget_debug().unwrap();
    assert_eq!(
        serde_json::to_value(&duplicate).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let usage = crate::provider::Usage {
        observed_model: None,
        prompt_tokens: 600000,
        completion_tokens: 400000,
        prompt_reported: true,
        completion_reported: true,
        unpriced: false,
    };
    assert!(host
        .redeliver_evaluation_receipt_fixture(key, &usage)
        .is_err());
    let conflict = host.evaluation_budget_debug().unwrap();
    assert!(conflict.blocked);
    assert_eq!(
        conflict.known_mc, 1000000,
        "actual cost is not clipped to its reservation"
    );
    assert_eq!(
        conflict.requests, before.requests,
        "second receipt is not another call"
    );
    assert!(conflict.reserved_mc >= before.unknown_mc);
    assert!(conflict.available_mc <= before.available_mc);
}

#[test]
fn evaluation_budget_finished_pair_releases_only_unused_parent_reservation() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let request = super::evaluation_config_tests::request();
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, fixture_price())
        .unwrap();
    for entry in &plan.entries[..2] {
        let task = &request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .unwrap()
            .task;
        let run = host
            .evaluate_next_debug(
                &plan.id,
                &[crate::evaluation::DebugActivation {
                    role: request.fast_role.clone(),
                    writes: task.reference.clone(),
                    request_baseline_merge: false,
                }],
            )
            .unwrap();
        assert_eq!(run.state, "completed");
    }
    let report = host.evaluation_budget_debug().unwrap();
    assert!(
        report.runs.iter().all(|r| r.closed),
        "canonical workspace identity must match terminal closure"
    );
    assert_eq!(report.reserved_mc, report.unknown_mc + report.in_flight_mc);
    assert!(
        report.reserved_mc < 1000000,
        "unused parent allowance is released after both arms"
    );
}
