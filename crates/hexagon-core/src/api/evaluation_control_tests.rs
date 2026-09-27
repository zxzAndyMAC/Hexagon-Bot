use super::*;

#[test]
fn evaluation_stop_waiting_run_preserves_costs_and_prevents_owner_resume() {
    let home = tempfile::tempdir().unwrap();
    let wb = Workbench::open_evaluation_host(home.path()).unwrap();
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
    assert_eq!(run.state, "waiting_human");
    let before = wb.evaluation_budget_debug().unwrap();
    let stopped = wb.stop_evaluation_run(&run.id).unwrap();
    assert_eq!(
        stopped.state,
        crate::evaluation::EvaluationControlState::Interrupted
    );
    assert_eq!(stopped.reason.as_deref(), Some("owner_stopped"));
    assert!(stopped.cleanup_confirmed);
    assert!(wb
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .is_err());
    let budget = wb.evaluation_budget_debug().unwrap();
    assert_eq!(budget.requests, before.requests);
    assert_eq!(budget.unknown_mc, before.unknown_mc);
    assert_eq!(budget.known_mc, before.known_mc);
    drop(wb);
    let reopened = Workbench::open_evaluation_host(home.path()).unwrap();
    assert_eq!(
        reopened.evaluation_control(&run.id).unwrap().state,
        crate::evaluation::EvaluationControlState::Interrupted
    );
    assert_eq!(
        reopened.evaluation_result(&run.id).unwrap().state,
        "incomplete"
    );
}

#[test]
fn evaluation_stop_inflight_tool_only_response_never_executes_its_write() {
    use crate::provider::{
        ChatRequest, ChatResponse, ContentBlock, ModelProvider, ProviderError, ScriptedProvider,
        StopReason,
    };
    let (_home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    let task = host.evaluation_result(&run.id).unwrap().task_id;
    let request = super::evaluation_config_tests::request();
    let task = &request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .find(|c| c.task.id == task)
        .unwrap()
        .task;
    let target = task.allowed_paths[0].clone();
    let original = std::fs::read(Path::new(&run.workspace).join(&target)).unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    struct BlockingFixture {
        inner: ScriptedProvider,
        ready: std::sync::mpsc::Sender<()>,
        resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl ModelProvider for BlockingFixture {
        fn is_scripted(&self) -> bool {
            true
        }
        fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            let result = self.inner.complete(req)?;
            if result
                .content
                .iter()
                .any(|b| matches!(b,ContentBlock::ToolUse{name,..} if name=="fs_write"))
            {
                self.ready.send(()).unwrap();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(30))
                    .unwrap();
            }
            Ok(result)
        }
    }
    let response = |content| ChatResponse {
        content,
        stop: StopReason::ToolUse,
        usage: Default::default(),
    };
    let provider = Arc::new(BlockingFixture {
        inner: ScriptedProvider::new(vec![
            response(vec![ContentBlock::Text {
                text: "plan".into(),
            }]),
            response(vec![ContentBlock::ToolUse {
                id: "before-stop-read".into(),
                name: "fs_read".into(),
                input: serde_json::json!({"path":target}),
            }]),
            response(vec![ContentBlock::ToolUse {
                id: "late-write".into(),
                name: "fs_write".into(),
                input: serde_json::json!({"path":target,"content":"unexpected late write"}),
            }]),
        ]),
        ready: ready_tx,
        resume: std::sync::Mutex::new(resume_rx),
    });
    let root = run.workspace.clone();
    let execution = std::thread::spawn(move || {
        let mut worker = Workbench::open_evaluation_host(Path::new(&root)).unwrap();
        worker.register_provider("default", provider);
        worker.dispatch_instance("a0", "Read the task file and update it.", &[])
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    let stopped = host.stop_evaluation_run(&run.id).unwrap();
    // Always release/join before assertions, so a failed test leaves no work.
    resume_tx.send(()).unwrap();
    let _ = execution.join().unwrap();
    assert_eq!(
        stopped.state,
        crate::evaluation::EvaluationControlState::Stopping,
        "pending response is not yet cleaned up"
    );
    assert_eq!(
        std::fs::read(Path::new(&run.workspace).join(target)).unwrap(),
        original,
        "tool-only response after stop must not write"
    );
    assert!(!host.evaluation_result(&run.id).unwrap().flow_completed);
}

#[test]
fn evaluation_stop_blocks_owner_stage_check_processes() {
    let (_home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    // A native stage-check fixture shares the controlled workspace. Its own
    // facade DB is test-only; the host-owned stop binding is still authoritative.
    let pack:PackDef=serde_json::from_value(serde_json::json!({"name":"stop-check","version":1,"stages":[{"name":"check","roles":["后端"],"due":[],"checks":["printf unexpected > stop-proof"]}]})).unwrap();
    let worker = Workbench::for_test(Path::new(&run.workspace), &["后端"], Some(pack)).unwrap();
    worker.open_stage(0).unwrap();
    host.stop_evaluation_run(&run.id).unwrap();
    assert!(
        worker.run_checks().is_err(),
        "owner check commands must obey the evaluation stop"
    );
    assert!(!Path::new(&run.workspace).join("stop-proof").exists());
}

#[test]
fn evaluation_stop_interrupts_running_stage_check_processes() {
    let (_home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    // Keep an independently owned work lease while the process exits. This
    // fixes the observation point instead of racing the watchdog's kill signal.
    let active_work = host.hold_evaluation_work_fixture(&run.id).unwrap();
    let root = PathBuf::from(&run.workspace);
    let workroot = root.clone();
    let execution = std::thread::spawn(move || {
        let pack:PackDef=serde_json::from_value(serde_json::json!({"name":"stop-running-check","version":1,"stages":[{"name":"check","roles":["后端"],"due":[],"checks":["printf started > started-proof; sleep 30; printf finished > finished-proof"]}]})).unwrap();
        let worker = Workbench::for_test(&workroot, &["后端"], Some(pack)).unwrap();
        worker.open_stage(0).unwrap();
        worker.run_checks()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !root.join("started-proof").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(root.join("started-proof").exists());
    let stopped = std::time::Instant::now();
    let control = host.stop_evaluation_run(&run.id).unwrap();
    assert!(
        !control.cleanup_confirmed,
        "a kill request is not proof the process exited"
    );
    assert!(execution.join().unwrap().is_err());
    assert!(stopped.elapsed() < std::time::Duration::from_secs(5));
    assert!(!root.join("finished-proof").exists());
    assert!(!host.evaluation_control(&run.id).unwrap().cleanup_confirmed);
    drop(active_work);
}

#[test]
fn evaluation_active_deadline_prevents_first_request() {
    let home = tempfile::tempdir().unwrap();
    let host = Workbench::open_evaluation_host(home.path()).unwrap();
    let mut request = super::evaluation_config_tests::request();
    request.limits.active_ms = 1;
    let batch = host.freeze_evaluation(&request, None).unwrap();
    let plan = host
        .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
        .unwrap();
    host.enable_evaluation_budget_debug(&plan.id, super::evaluation_budget_tests::fixture_price())
        .unwrap();
    let run = host
        .evaluate_next_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: request.fast_role,
                writes: Default::default(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    assert_eq!(run.state, "incomplete");
    assert_eq!(
        host.evaluation_control(&run.id).unwrap().reason.as_deref(),
        Some("active_time_limit")
    );
    assert_eq!(host.evaluation_budget_debug().unwrap().requests, 0);
}

#[test]
fn evaluation_stop_http_silence_and_partial_usage_keep_unknown_reservation() {
    stop_http_peer(false);
}
#[test]
fn evaluation_stop_slow_stream_preserves_partial_usage() {
    stop_http_peer(true);
}
fn stop_http_peer(streaming: bool) {
    use crate::credentials::CredentialStore;
    use crate::provider::{
        ChatRequest, ChatResponse, HttpProvider, ModelMeta, ModelProvider, ProviderError,
        ProviderKind,
    };
    use std::io::{BufRead, Read, Write};
    // Loopback-only transport fixture. This adapter cannot connect to a paid
    // model and exercises the real HTTP parser through the Workbench facade.
    struct LocalPeer(HttpProvider);
    impl ModelProvider for LocalPeer {
        fn is_scripted(&self) -> bool {
            true
        }
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            self.0.complete(request)
        }
        fn stream(
            &self,
            request: &ChatRequest,
            sink: &mut crate::provider::StreamSink<'_>,
        ) -> Result<ChatResponse, ProviderError> {
            self.0.stream(request, sink)
        }
    }
    let (_home, host, run) = super::evaluation_budget_tests::waiting_budget(
        80,
        500000,
        super::evaluation_budget_tests::fixture_price(),
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (arrived_tx, arrived_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .unwrap();
        let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
        let mut size = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                size = n.trim().parse::<usize>().unwrap();
            }
            if line.trim().is_empty() {
                break;
            }
        }
        let mut bytes = vec![0; size];
        reader.read_exact(&mut bytes).unwrap();
        let prefix =
            "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":100}}}\n\n";
        let suffix="event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"late\"}}\n\nevent: message_stop\ndata: {}\n\n";
        if streaming {
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",prefix.len()+suffix.len(),prefix).unwrap();
            socket.flush().unwrap();
        }
        arrived_tx.send(()).unwrap();
        release_rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        if streaming {
            let _ = socket.write_all(suffix.as_bytes());
            return;
        }
        let response = r#"{"choices":[{"message":{"content":"late reply"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100}}"#;
        let _=write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response);
    });
    let before = host.evaluation_budget_debug().unwrap();
    let root = run.workspace.clone();
    let execution = std::thread::spawn(move || {
        let creds = Arc::new(crate::credentials::MemoryStore::default());
        creds.set("local-test", "synthetic").unwrap();
        let provider = LocalPeer(HttpProvider::new(
            if streaming {
                ProviderKind::Anthropic
            } else {
                ProviderKind::OpenAi
            },
            base,
            "local-test".into(),
            "local-test".into(),
            creds,
            ModelMeta::default(),
        ));
        let mut worker = Workbench::open_evaluation_host(Path::new(&root)).unwrap();
        worker.register_provider("default", Arc::new(provider));
        if streaming {
            let outcome = worker.run_instance("a0", "local stream cancellation test")?;
            if matches!(outcome, crate::turn::TurnOutcome::Interrupted) {
                Err(ApiError::BadInput("interrupted".into()))
            } else {
                Ok(format!("{outcome:?}"))
            }
        } else {
            worker.draft_role_def("a0", "local transport cancellation test")
        }
    });
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let stopping = host.stop_evaluation_run(&run.id).unwrap();
    release_tx.send(()).unwrap();
    let result = execution.join().unwrap();
    peer.join().unwrap();
    assert_eq!(
        stopping.state,
        crate::evaluation::EvaluationControlState::Stopping
    );
    assert!(stopping.pending_requests > 0);
    assert!(!stopping.remote_cancellation_confirmed);
    assert!(result.is_err());
    let budget = host.evaluation_budget_debug().unwrap();
    assert_eq!(budget.requests, before.requests + 1);
    assert_eq!(budget.known_mc, before.known_mc + 100);
    assert!(budget.unknown_mc > before.unknown_mc);
    assert_eq!(
        host.evaluation_control(&run.id).unwrap().pending_requests,
        0
    );
}

#[test]
fn evaluation_pause_away_and_stop_preserve_human_timing_boundaries() {
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
    let before = host.evaluation_control(&run.id).unwrap().active_ms;
    assert_eq!(
        host.pause_evaluation_run(&run.id).unwrap().state,
        crate::evaluation::EvaluationControlState::Paused
    );
    assert!(host
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .is_err());
    std::thread::sleep(std::time::Duration::from_millis(25));
    assert_eq!(host.evaluation_control(&run.id).unwrap().active_ms, before);
    host.resume_evaluation_run(&run.id).unwrap();
    let attention = host
        .begin_evaluation_attention(&run.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let stopped = host.stop_evaluation_run(&run.id).unwrap();
    assert!(stopped.active_ms >= before + 10);
    assert!(!stopped.active_time_complete);
    assert!(host
        .end_evaluation_attention(&attention, crate::evaluation::AttentionEnd::Away)
        .is_err());
    assert!(host.resume_evaluation_run(&run.id).is_err());
    assert_eq!(
        host.evaluation_timing(&run.id).unwrap().intervals[0]
            .end_reason
            .as_deref(),
        Some("interrupted")
    );
    std::thread::sleep(std::time::Duration::from_millis(25));
    assert_eq!(
        host.evaluation_control(&run.id).unwrap().active_ms,
        stopped.active_ms
    );
    assert_eq!(
        host.stop_evaluation_run(&run.id).unwrap().active_ms,
        stopped.active_ms
    );
    let next = host
        .evaluate_next_with_owner_debug(
            &plan.id,
            &[crate::evaluation::DebugActivation {
                role: "后端".into(),
                writes: Default::default(),
                request_baseline_merge: false,
            }],
        )
        .unwrap();
    let next_attention = host
        .begin_evaluation_attention(&next.id, crate::evaluation::EvaluationActor::Scripted)
        .unwrap();
    host.end_evaluation_attention(&next_attention, crate::evaluation::AttentionEnd::Away)
        .unwrap();
}

#[test]
fn evaluation_last_reserved_request_is_sent_before_boundary_stop() {
    for (requests, run_mc, reason) in [(1, 500000, "request_limit"), (80, 3000, "run_budget_limit")]
    {
        let home = tempfile::tempdir().unwrap();
        let host = Workbench::open_evaluation_host(home.path()).unwrap();
        let mut request = super::evaluation_config_tests::request();
        request.limits.requests = requests;
        request.limits.run_mc = run_mc;
        let batch = host.freeze_evaluation(&request, None).unwrap();
        let plan = host
            .plan_evaluation(&batch.id, crate::evaluation::PlanKind::Pilot)
            .unwrap();
        host.enable_evaluation_budget_debug(
            &plan.id,
            super::evaluation_budget_tests::fixture_price(),
        )
        .unwrap();
        let run = host
            .evaluate_next_debug(
                &plan.id,
                &[crate::evaluation::DebugActivation {
                    role: request.fast_role,
                    writes: Default::default(),
                    request_baseline_merge: false,
                }],
            )
            .unwrap();
        let budget = host.evaluation_budget_debug().unwrap();
        assert_eq!(budget.requests, 1);
        // D09: the last streamed reply may be interrupted at its first delta,
        // before complete usage arrives. Dispatch survives as unknown, not a
        // falsely confirmed response or a refunded not-sent reservation.
        assert!(budget.unknown_mc >= 3000);
        assert_eq!(run.state, "incomplete");
        assert_eq!(
            host.evaluation_control(&run.id).unwrap().reason.as_deref(),
            Some(reason)
        );
    }
}
