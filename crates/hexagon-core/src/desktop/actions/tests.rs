use super::*;
use crate::tools::{CallOutcome, RiskClass, Tool, ToolContext};
use proptest::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn request(op: &str, snapshot: Option<&str>) -> NativeRequest {
    serde_json::from_value(json!({"op":op,"snapshot_id":snapshot})).unwrap()
}
fn owner(project: &str, agent: &str) -> Owner {
    Owner {
        project: PathBuf::from(project),
        agent: agent.into(),
    }
}
fn observed(id: &str) -> NativeReply {
    NativeReply {
        protocol_version: 1,
        ok: true,
        result: Some(json!({"snapshot_id":id})),
        applications: None,
        error: None,
        outcome_unknown: false,
        cancelled: false,
        outcome: None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn preparing_never_resumes_or_clears_unknown_delivery(paused in any::<bool>(), unknown in any::<bool>(), busy in any::<bool>()) {
        let mut state = State { owner: Some(owner("/first", "qa")), paused, outcome_unknown: unknown, busy, snapshot: Some("old".into()), ..State::default() };
        prop_assert!(state.control(Path::new("/second"), DesktopControl::PrepareCapture).is_err());
        prop_assert_eq!(state.control(Path::new("/first"), DesktopControl::PrepareCapture).is_ok(), !busy);
        prop_assert_eq!(state.paused, paused);
        prop_assert_eq!(state.outcome_unknown, unknown);
        prop_assert_eq!(state.snapshot.as_deref(), Some("old"));
    }

    #[test]
    fn owner_pause_is_global_but_resume_and_disable_respect_project_lease(busy in any::<bool>(), operation in 1u64..u64::MAX) {
        let mut state = State { owner: Some(owner("/first", "qa")), busy, active: busy.then_some(operation), ..State::default() };
        prop_assert_eq!(state.control(Path::new("/second"), DesktopControl::Disable).unwrap(), None);
        prop_assert!(!state.paused);
        prop_assert!(state.control(Path::new("/second"), DesktopControl::Resume).is_err());
        prop_assert_eq!(state.control(Path::new("/second"), DesktopControl::Pause).unwrap(), busy.then_some(operation));
        prop_assert!(state.paused);
        prop_assert!(state.claim(&owner("/first", "qa"), &request("observe", None)).is_err());
        prop_assert_eq!(state.control(Path::new("/first"), DesktopControl::Resume).is_ok(), !busy);
    }

    #[test]
    fn native_refusal_pauses_without_inventing_unknown_effects(unknown in any::<bool>(), op in prop::sample::select(vec!["observe", "observe_screen", "click", "type"])) {
        let mut state = State { busy: true, active: Some(1), snapshot: Some("old".into()), ..State::default() };
        state.completed(&request(op, Some("old")), &NativeReply { ok: false, outcome_unknown: unknown, error: Some("system_permission_required".into()), ..observed("unused") });
        prop_assert!(state.paused);
        prop_assert!(!state.busy);
        prop_assert!(state.snapshot.is_none());
        prop_assert_eq!(state.outcome_unknown, unknown);
    }

    #[test]
    fn desktop_lease_is_exclusive_and_stale_snapshots_never_dispatch(
        project in "[a-z]{1,10}", other in "[a-z]{1,10}", agent in "[a-z]{1,10}",
        snapshot in "[a-z0-9]{1,20}", busy in any::<bool>(), paused in any::<bool>()
    ) {
        let first = owner(&project, &agent);
        let mut state = State::default();
        let observe = request("observe", None);
        prop_assert!(state.claim(&first, &observe).is_ok());
        state.completed(&observe, &observed(&snapshot));
        let second = owner(&format!("other-{other}"), &agent);
        prop_assert!(state.claim(&second, &observe).is_err());
        prop_assert!(state.claim(&first, &request("click", Some("foreign-snapshot"))).is_err());
        state.busy = busy;
        state.paused = paused;
        let click = request("click", Some(&snapshot));
        prop_assert_eq!(state.claim(&first, &click).is_ok(), !busy && !paused);
        if !busy && !paused {
            state.completed(&click, &NativeReply { outcome_unknown: true, ..observed("unused") });
            prop_assert!(state.claim(&first, &click).is_err());
            prop_assert!(state.claim(&first, &request("activate", None)).is_err());
            prop_assert!(state.claim(&first, &observe).is_ok());
        }
    }

    #[test]
    fn desktop_mutation_cannot_be_autoapproved_or_remembered(
        mode in prop::sample::select(vec![crate::approval_mode::ApprovalMode::Restricted, crate::approval_mode::ApprovalMode::Assisted, crate::approval_mode::ApprovalMode::Broad]),
        operation in prop::sample::select(vec!["click", "type", "key", "scroll", "drag", "activate"]),
        remembered in any::<bool>()
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        wb.set_approval_mode(mode).unwrap();
        let ctx = wb.ctx_for("a0", None);
        if remembered {
            wb.db.conn().execute("INSERT INTO permission_rules(id,project_id,tool,shape,effect,scope) VALUES('desktop-rule',?1,'computer_action','*','allow','project')", [&wb.project_id]).unwrap();
        }
        let decision = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::desktop::ComputerAction, "computer_action", &json!({"op":operation})).unwrap();
        prop_assert!(matches!(decision, crate::permissions::Decision::Ask { safety_net: true, .. }), "critical action must ask");
    }

    #[test]
    fn only_host_limited_navigation_is_released_by_assisted_modes(
        mode in prop::sample::select(vec![crate::approval_mode::ApprovalMode::Restricted, crate::approval_mode::ApprovalMode::Assisted, crate::approval_mode::ApprovalMode::Broad]),
        operation in prop::sample::select(vec!["activate", "scroll", "click", "type", "key", "drag"])
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
        wb.set_approval_mode(mode).unwrap();
        let ctx = wb.ctx_for("a0", None);
        let decision = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::desktop::ComputerNavigate, "computer_navigate", &json!({"op":operation})).unwrap();
        let permitted = mode != crate::approval_mode::ApprovalMode::Restricted && matches!(operation, "activate" | "scroll");
        prop_assert_eq!(matches!(decision, crate::permissions::Decision::Allow { .. }), permitted);
        if !matches!(operation, "activate" | "scroll") {
            prop_assert!(crate::tools::desktop::ComputerNavigate.precondition(&json!({"op":operation}), &ctx).is_err(), "navigation cannot accept arbitrary effects");
        }
    }
}

#[test]
fn missing_consent_or_vision_never_reaches_native_backend() {
    let dir = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let mut ctx = wb.ctx_for("a0", None);
    let observe = crate::tools::desktop::ComputerObserve;
    assert!(observe
        .precondition(&json!({"op":"observe"}), &ctx)
        .is_err());
    ctx.caps.insert("vision".into());
    assert!(matches!(
        observe.exec(&wb.db, &json!({"op":"observe"}), &ctx),
        Err(ToolError::NotExecuted(_))
    ));
    let registry = crate::tools::Registry::builtin();
    assert!(registry.get("computer_action").is_some());
    let child = registry.subagent_scope(&[]);
    assert!(child.get("computer_action").is_none());
    assert!(child.get("computer_observe").is_none());
    assert!(child.get("computer_navigate").is_none());
}

struct NoncooperativeBackend {
    complete: AtomicBool,
    cancelled: AtomicBool,
    starts: AtomicUsize,
}
impl Backend for NoncooperativeBackend {
    fn start(&self, _: &str) -> Result<u64, String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(1)
    }
    fn cancel(&self, _: u64) -> Result<(), String> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn poll(&self, _: u64) -> Result<Option<NativeReply>, String> {
        Ok(self
            .complete
            .load(Ordering::SeqCst)
            .then(|| observed("fresh")))
    }
}

#[test]
fn timeout_keeps_native_lane_until_noncooperative_work_actually_returns() {
    let backend = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let controller = Arc::new(Controller {
        backend: backend.clone(),
        state: Mutex::new(State::default()),
    });
    let observe = request("observe", None);
    assert!(matches!(
        controller.run(owner("/one", "qa"), &observe, Duration::from_millis(1)),
        Err(ToolError::OutcomeUnknown(_))
    ));
    assert!(backend.cancelled.load(Ordering::SeqCst));
    assert!(controller.state.lock().unwrap().busy);
    assert!(controller
        .run(owner("/two", "dev"), &observe, Duration::from_millis(1))
        .is_err());
    assert_eq!(backend.starts.load(Ordering::SeqCst), 1);
    backend.complete.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.state.lock().unwrap().busy && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = controller.state.lock().unwrap();
    assert!(!state.busy);
    assert!(state.paused);
    assert!(state.outcome_unknown);
}

#[test]
fn a_pause_winning_the_start_lock_never_enqueues_native_work() {
    let backend = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(true),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let controller = Arc::new(Controller {
        backend: backend.clone(),
        state: Mutex::new(State::default()),
    });
    controller
        .state
        .lock()
        .unwrap()
        .control(Path::new("/one"), DesktopControl::Pause)
        .unwrap();
    assert!(matches!(
        controller.run(
            owner("/one", "qa"),
            &request("observe", None),
            Duration::from_secs(1)
        ),
        Err(ToolError::NotExecuted(_))
    ));
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
}

struct ScreenshotTool;
impl Tool for ScreenshotTool {
    fn name(&self) -> &str {
        "computer_observe"
    }
    fn description(&self) -> &str {
        "test host image boundary"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _: &Db, _: &Value, _: &ToolContext) -> Result<Value, ToolError> {
        Ok(
            json!({"image":{"media_type":"image/png","data":"SCREENSHOT_BYTES_DO_NOT_PERSIST"},"result":{"screenshot_path":"/local/evidence.png"}}),
        )
    }
}

#[test]
fn tool_images_reach_model_but_not_durable_actions_or_trace() {
    let dir = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let registry = crate::tools::Registry::builtin();
    registry.register(ScreenshotTool);
    let result = registry
        .call(
            &wb.db,
            &wb.ctx_for("a0", None),
            "computer_observe",
            json!({}),
        )
        .unwrap();
    let CallOutcome::Done(output) = result else {
        panic!("read should complete");
    };
    assert_eq!(output["image"]["data"], "SCREENSHOT_BYTES_DO_NOT_PERSIST");
    let durable: String = wb
        .db
        .conn()
        .query_row(
            "SELECT output_json FROM tool_actions WHERE tool='computer_observe'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!durable.contains("SCREENSHOT_BYTES_DO_NOT_PERSIST"));
    assert!(durable.contains("/local/evidence.png"));
    let leaks: i64 = wb
        .db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE payload LIKE '%SCREENSHOT_BYTES_DO_NOT_PERSIST%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leaks, 0);
}

#[test]
fn screenshots_are_private_clearable_and_not_readable_through_generic_tools() {
    let dir = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let saved = save_screenshot(dir.path(), b"test evidence").unwrap();
    assert_eq!(screenshot_files(dir.path()).unwrap().len(), 1);
    let ctx = wb.ctx_for("a0", None);
    let relative = Path::new(&saved)
        .strip_prefix(dir.path().canonicalize().unwrap())
        .unwrap()
        .to_string_lossy();
    assert!(crate::tools::FsRead
        .builtin_deny(&json!({"path":relative}), &ctx)
        .is_some());
    let status = control(&wb.db, dir.path(), DesktopControl::ClearScreenshots).unwrap();
    assert_eq!(status.screenshot_count, 0);
    assert!(!Path::new(&saved).exists());
}

#[cfg(unix)]
#[test]
fn system_lease_is_exclusive_and_released_on_close() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("runtime");
    let first = lease::acquire_at(&directory).unwrap();
    assert!(lease::acquire_at(&directory).is_err());
    drop(first);
    let next = lease::acquire_at(&directory).unwrap();
    drop(next);
    std::fs::remove_file(directory.join("desktop.lock")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("victim"), directory.join("desktop.lock")).unwrap();
    assert!(lease::acquire_at(&directory).is_err());
    assert!(!temp.path().join("victim").exists());
}

#[cfg(unix)]
proptest::proptest! {
    #[test]
    fn lease_never_accepts_nonprivate_runtime_permissions(mode in 0u32..0o777u32) {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("runtime");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(mode)).unwrap();
        let result = lease::acquire_at(&directory);
        if mode != 0o700 { proptest::prop_assert!(result.is_err()); }
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn clearing_evidence_does_not_resurrect_a_capture_waiting_to_be_saved() {
    let temp = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(temp.path(), &["QA"], None).unwrap();
    let captured = EVIDENCE_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    control(&wb.db, temp.path(), DesktopControl::ClearScreenshots).unwrap();
    assert!(persist_capture(temp.path(), b"test evidence", captured).is_err());
    assert!(screenshot_files(temp.path()).unwrap().is_empty());
}

fn browser_request(read: bool, observation: bool, snapshot: Option<&str>) -> LaneRequest {
    LaneRequest {
        executor: Executor::Browser,
        read,
        observation,
        bootstrap: false,
        snapshot_id: snapshot.map(str::to_owned),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn browser_and_native_receipts_never_cross_executor_authority(
        snapshot in "[a-z0-9]{1,24}",
        browser_first in any::<bool>(),
        unknown in any::<bool>(),
    ) {
        let actor = owner("/one", "qa");
        let mut state = State::default();
        let mut observe = browser_request(true, true, None);
        if !browser_first { observe.executor = Executor::Native; }
        prop_assert!(state.claim_lane(&actor, &observe).is_ok());
        state.completed_lane(&observe, &observed(&snapshot));
        let mut action = browser_request(false, false, Some(&snapshot));
        action.executor = if browser_first { Executor::Native } else { Executor::Browser };
        // Even a byte-identical receipt from another executor is insufficient.
        prop_assert!(state.claim_lane(&actor, &action).is_err());
        action.executor = observe.executor;
        state.outcome_unknown = unknown;
        prop_assert_eq!(state.claim_lane(&actor, &action).is_ok(), !unknown);
        if !unknown {
            state.completed_lane(&action, &observed("unused"));
            prop_assert!(state.claim_lane(&actor, &action).is_err());
        }
    }

    #[test]
    fn browser_bootstrap_cannot_bypass_pause_unknown_or_foreign_owner(
        paused in any::<bool>(), unknown in any::<bool>(), foreign in any::<bool>(),
    ) {
        let mut state = State {
            paused,
            outcome_unknown: unknown,
            owner: foreign.then(|| owner("/other", "qa")),
            ..State::default()
        };
        let mut request = browser_request(false, false, None);
        request.bootstrap = true;
        prop_assert_eq!(state.claim_lane(&owner("/one", "qa"), &request).is_ok(), !paused && !unknown && !foreign);
    }
}

#[test]
fn browser_timeout_retains_shared_lane_and_cancels_browser_not_native() {
    let native = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(true),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let browser = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let controller = Arc::new(Controller {
        backend: native.clone(),
        state: Mutex::new(State::default()),
    });
    assert!(matches!(
        controller.run_with_backend(
            owner("/one", "qa"),
            browser_request(true, true, None),
            "{}",
            Duration::from_millis(1),
            browser.clone()
        ),
        Err(ToolError::OutcomeUnknown(_))
    ));
    assert!(browser.cancelled.load(Ordering::SeqCst));
    assert!(!native.cancelled.load(Ordering::SeqCst));
    assert!(controller
        .run(
            owner("/one", "qa"),
            &request("observe", None),
            Duration::from_secs(1)
        )
        .is_err());
    assert_eq!(native.starts.load(Ordering::SeqCst), 0);
    assert!(controller.state.lock().unwrap().busy);
    browser.complete.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.state.lock().unwrap().busy && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = controller.state.lock().unwrap();
    assert!(!state.busy);
    assert!(state.active_backend.is_none());
    assert!(state.paused);
    assert!(state.outcome_unknown);
}

#[test]
fn owner_pause_routes_to_running_browser_and_blocks_native_dispatch() {
    let native = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(true),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let browser = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let controller = Arc::new(Controller {
        backend: native.clone(),
        state: Mutex::new(State::default()),
    });
    let running = Arc::clone(&controller);
    let executor = browser.clone();
    let worker = std::thread::spawn(move || {
        running.run_with_backend(
            owner("/one", "qa"),
            browser_request(true, true, None),
            "{}",
            Duration::from_secs(2),
            executor,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while browser.starts.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let mut state = controller.state.lock().unwrap();
        let id = state
            .control(Path::new("/one"), DesktopControl::Pause)
            .unwrap()
            .unwrap();
        state.cancel_active(id).unwrap();
    }
    assert!(browser.cancelled.load(Ordering::SeqCst));
    assert!(!native.cancelled.load(Ordering::SeqCst));
    assert!(controller
        .run(
            owner("/one", "qa"),
            &request("observe", None),
            Duration::from_secs(1)
        )
        .is_err());
    assert_eq!(native.starts.load(Ordering::SeqCst), 0);
    browser.complete.store(true, Ordering::SeqCst);
    assert!(worker.join().unwrap().is_ok());
    assert!(controller.state.lock().unwrap().paused);
}

#[test]
fn browser_without_project_consent_never_starts_backend() {
    let dir = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let mut ctx = wb.ctx_for("a0", None);
    ctx.caps.insert("vision".into());
    let backend = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(true),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let request = BrowserLaneRequest {
        serialized: "{}".into(),
        read: true,
        observation: true,
        bootstrap: false,
        snapshot_id: None,
    };
    assert!(matches!(
        execute_browser(&wb.db, &ctx, request, backend.clone()),
        Err(ToolError::NotExecuted(_))
    ));
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn owner_focus_allows_paused_inspection_but_never_retargets_busy_or_foreign_work(
        busy in any::<bool>(), paused in any::<bool>(), foreign in any::<bool>(), unknown in any::<bool>(),
    ) {
        let controller = Arc::new(Controller {
            backend: Arc::new(NoncooperativeBackend { complete: AtomicBool::new(true), cancelled: AtomicBool::new(false), starts: AtomicUsize::new(0) }),
            state: Mutex::new(State { owner: Some(owner(if foreign { "/other" } else { "/one" }, "qa")), busy, paused, outcome_unknown: unknown, snapshot: Some("old-focus".into()), ..State::default() }),
        });
        let result = controller.reserve_owner_focus(Path::new("/one"));
        prop_assert_eq!(result.is_ok(), !busy && !foreign);
        if let Ok(guard) = result {
            {
                let mut state = controller.state.lock().unwrap();
                prop_assert!(state.busy);
                prop_assert!(state.snapshot.is_none());
                prop_assert!(state.claim(&owner("/one", "qa"), &request("observe", None)).is_err());
                prop_assert_eq!(state.paused, paused);
            }
            drop(guard);
            let state = controller.state.lock().unwrap();
            prop_assert!(!state.busy);
            prop_assert_eq!(state.paused, paused);
            prop_assert_eq!(state.outcome_unknown, unknown);
        }
    }
}

// Owner issue19 / 2026-10-02: explicit lifecycle must not steal a lane, clear
// unknown effects, or turn release/reacquire into an implicit owner resume.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn session_lifecycle_preserves_owner_boundaries(paused in any::<bool>(), busy in any::<bool>(), unknown in any::<bool>(), foreign in any::<bool>()) {
        let caller=owner("/a","qa");
        let holder=if foreign { owner("/b","qa") } else { caller.clone() };
        let mut state=State { owner:Some(holder.clone()),paused,busy,outcome_unknown:unknown,snapshot:Some("old".into()),..State::default() };
        let status=state.session(&caller,"status").unwrap();
        prop_assert_eq!(status["held_by_caller"].as_bool(),Some(!foreign));
        prop_assert_eq!(state.session(&caller,"acquire").is_ok(),!paused && !busy && !foreign);
        prop_assert_eq!(state.session(&caller,"release").is_ok(),!busy && !foreign);
        prop_assert_eq!(state.paused,paused);
        prop_assert_eq!(state.outcome_unknown,unknown);
        if !busy && !foreign { prop_assert!(state.owner.is_none()); prop_assert!(state.snapshot.is_none()); }
        else { prop_assert_eq!(state.owner,Some(holder)); }
    }
}

#[test]
fn session_tools_are_not_available_to_subagents_and_cannot_resume() {
    let child = crate::tools::Registry::builtin().subagent_scope(&[]);
    assert!(child.get("computer_session").is_none());
    assert!(child.get("browser_session").is_none());
    let dir = tempfile::tempdir().unwrap();
    let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let tool = crate::tools::desktop::ComputerSession;
    for op in ["resume", "enable", "disable", "launch", "close"] {
        assert!(tool
            .precondition(&json!({"op":op}), &wb.ctx_for("a0", None))
            .is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn unknown_browser_lifecycle_cannot_replay_after_role_release(reply_ok in any::<bool>()) {
        // Issue19: open/close use bootstrap (no prior screenshot), but bootstrap
        // must never waive uncertain delivery, including after relinquishing.
        let caller=owner("/a","qa");
        let lifecycle=LaneRequest { executor:Executor::Browser,read:false,observation:false,bootstrap:true,snapshot_id:None };
        let mut state=State::default();
        state.claim_lane(&caller,&lifecycle).unwrap();
        state.completed_lane(&lifecycle,&NativeReply { ok:reply_ok,outcome_unknown:true,..observed("unused") });
        prop_assert!(state.claim_lane(&caller,&lifecycle).is_err());
        state.session(&caller,"release").unwrap();
        let _=state.session(&caller,"acquire");
        prop_assert!(state.claim_lane(&caller,&lifecycle).is_err());
    }
}

// Benchmark I1: preview Stop did not cancel the executor that owned a real
// role operation. Closing must retain its lease until completion is observed.
#[test]
fn closing_project_cancels_its_backend_and_waits_for_actual_drain() {
    let backend = Arc::new(NoncooperativeBackend {
        complete: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        starts: AtomicUsize::new(0),
    });
    let controller = Arc::new(Controller {
        backend: backend.clone(),
        state: Mutex::new(State::default()),
    });
    let running = controller.clone();
    let worker = std::thread::spawn(move || {
        running.run(
            owner("/old", "qa"),
            &request("observe", None),
            Duration::from_secs(2),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while backend.starts.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let mut state = controller.state.lock().unwrap();
        assert!(state.close_project(Path::new("/old")).is_err());
        assert!(state.busy);
        assert!(state.owner.is_some());
        assert!(backend.cancelled.load(Ordering::SeqCst));
    }
    backend.complete.store(true, Ordering::SeqCst);
    worker.join().unwrap().unwrap();
    let mut state = controller.state.lock().unwrap();
    state.outcome_unknown = true;
    state.close_project(Path::new("/old")).unwrap();
    assert!(state.owner.is_none());
    assert!(!state.busy);
    assert!(state.paused);
    assert!(state.outcome_unknown);
    assert!(state.snapshot.is_none());
}

proptest! {
    #[test]
    fn closing_never_releases_foreign_control_or_erases_unknown(unknown in any::<bool>(), busy in any::<bool>()) {
        let mut state = State { owner: Some(owner("/old", "qa")), paused: false, busy, outcome_unknown: unknown, snapshot: Some("old".into()), ..State::default() };
        prop_assert!(state.close_project(Path::new("/other")).is_ok());
        prop_assert_eq!(state.owner.as_ref().map(|o|o.project.as_path()), Some(Path::new("/old")));
        prop_assert!(!state.paused);
        prop_assert_eq!(state.outcome_unknown, unknown);
        prop_assert_eq!(state.busy, busy);
        if !busy {
            prop_assert!(state.close_project(Path::new("/old")).is_ok());
            prop_assert!(state.owner.is_none());
            prop_assert!(state.paused);
            prop_assert_eq!(state.outcome_unknown, unknown);
            prop_assert!(state.snapshot.is_none());
        }
    }
}
