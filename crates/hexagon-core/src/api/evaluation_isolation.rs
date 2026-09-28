use super::*;
use crate::evaluation::{self as eval, GenerationContext, IsolationReport};
use crate::provider::{ChatResponse, ContentBlock, ScriptedProvider, StopReason};
use crate::tools::{CallOutcome, ToolError};

fn denied(result: Result<CallOutcome, ToolError>) -> bool {
    matches!(
        result,
        Ok(CallOutcome::Denied(_)) | Err(ToolError::PathEscape(_))
    )
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl Workbench {
    pub fn prepare_evaluation_generation(
        &self,
        batch: &str,
    ) -> Result<GenerationContext, ApiError> {
        Ok(eval::isolation::prepare(&self.db, &self.repo_root, batch)?)
    }
    pub fn evaluation_generation(&self, id: &str) -> Result<GenerationContext, ApiError> {
        Ok(eval::isolation::read(&self.db, id)?)
    }
    pub fn reveal_evaluation_task(
        &self,
        context: &str,
        task: &str,
    ) -> Result<GenerationContext, ApiError> {
        Ok(eval::isolation::reveal(&self.db, context, task)?)
    }
    pub fn evaluation_isolation_check(&self, id: &str) -> Result<IsolationReport, ApiError> {
        Ok(eval::isolation::read_check(&self.db, id)?)
    }
    /// Fixed host-owned probes, never an arbitrary command execution interface.
    /// Positive controls prove the tool/process ran before a denial can count.
    pub fn check_evaluation_isolation(&self) -> Result<IsolationReport, ApiError> {
        let started = std::time::Instant::now();
        let parent = tempfile::tempdir()?;
        let secret = parent.path().join("hidden-answer.txt");
        let canary = format!("hidden-answer-{}", self.db.next_id("isolation_canary")?);
        std::fs::write(&secret, &canary)?;
        let root = parent.path().join("worker");
        std::fs::create_dir_all(root.join(".hexagon"))?;
        std::fs::write(root.join("README.md"), "Public task requirements")?;
        std::fs::write(root.join(".hexagon/evaluation-worker"), "private-state-v1")?;
        let mut worker = Workbench::open_scoped(
            &root,
            "Isolation probe",
            &[("a0".into(), "后端".into())],
            None,
            false,
        )?;
        let _probe = eval::control::probe(&root)?;
        let ctx = worker.ctx_for("a0", None);
        let public = worker
            .registry
            .call(&worker.db, &ctx, "fs_read", json!({"path":"README.md"}));
        let public_requirements_readable = matches!(public,Ok(CallOutcome::Done(ref v)) if v.to_string().contains("Public task requirements"));
        let read_denied = denied(worker.registry.call(
            &worker.db,
            &ctx,
            "fs_read",
            json!({"path":secret}),
        ));
        let write_denied = denied(worker.registry.call(
            &worker.db,
            &ctx,
            "fs_write",
            json!({"path":secret,"content":"changed"}),
        ));
        // Ticket 24: agents can no longer read private state. This host-owned
        // probe supplies the read prerequisite itself so it still exercises the
        // non-overridable write denial, rather than failing on "read first".
        ctx.reads
            .record(&root.canonicalize()?.join(".hexagon/evaluation-worker"));
        let marker_denied = denied(worker.registry.call(
            &worker.db,
            &ctx,
            "fs_write",
            json!({"path":".hexagon/evaluation-worker","content":"changed"}),
        ));
        let mut index_clean = true;
        for name in ["fs_grep", "sem_search"] {
            let result = worker
                .registry
                .call(&worker.db, &ctx, name, json!({"query":canary}));
            index_clean &=
                matches!(result,Ok(CallOutcome::Done(ref v)) if !v.to_string().contains(&canary));
        }
        let quoted = quote(&secret.to_string_lossy());
        let direct=format!("cat README.md >/dev/null || exit 10\nif cat {quoted}; then echo READ_OPEN; else echo READ_BLOCKED; fi\nif printf changed > {quoted}; then echo WRITE_OPEN; else echo WRITE_BLOCKED; fi");
        let child=format!("/bin/sh -c 'cat README.md' >/dev/null || exit 10\nif /bin/sh -c {}; then echo CHILD_OPEN; else echo CHILD_BLOCKED; fi",quote(&format!("cat {quoted}")));
        let check_command = |command: &str, expected: &str| {
            worker
                .sessions
                .run_oneshot(
                    &worker.db,
                    &ctx,
                    command,
                    std::time::Duration::from_secs(10),
                    false,
                )
                .is_ok_and(|r| {
                    r["exit_code"].as_i64() == Some(0)
                        && r["timed_out"].as_bool() == Some(false)
                        && r["stdout"].as_str().is_some_and(|s| s.trim() == expected)
                })
        };
        let terminal_denied = check_command(&direct, "READ_BLOCKED\nWRITE_BLOCKED");
        let child_process_denied = check_command(&child, "CHILD_BLOCKED");
        let text = |body: &str| ChatResponse {
            content: vec![ContentBlock::Text { text: body.into() }],
            stop: StopReason::EndTurn,
            usage: Default::default(),
        };
        let provider = Arc::new(ScriptedProvider::new(vec![
            text("Read public requirements."),
            ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: "public-read".into(),
                    name: "fs_read".into(),
                    input: json!({"path":"README.md"}),
                }],
                stop: StopReason::ToolUse,
                usage: Default::default(),
            },
            text("Probe completed."),
        ]));
        worker.register_provider("default", provider.clone());
        let dispatched = matches!(
            worker.dispatch_instance("a0", "Read the public requirements", &[]),
            Ok(TurnOutcome::Finished)
        );
        let calls = provider.recorded();
        let messages_clean = dispatched
            && !calls.is_empty()
            && calls
                .iter()
                .all(|r| serde_json::to_string(&r.messages).is_ok_and(|s| !s.contains(&canary)));
        let host_material_unchanged = std::fs::read_to_string(&secret).is_ok_and(|s| s == canary)
            && std::fs::read_to_string(root.join(".hexagon/evaluation-worker"))
                .is_ok_and(|s| s == "private-state-v1");
        let mut report = IsolationReport {
            id: format!("isolation-{}", self.db.next_id("evaluation_isolation")?),
            runtime_fingerprint: eval::config::code_fingerprint()?,
            passed: false,
            public_requirements_readable,
            filesystem_denied: read_denied && write_denied && marker_denied,
            terminal_denied,
            child_process_denied,
            index_clean,
            messages_clean,
            host_material_unchanged,
            evidence_kind: "host_boundary_probe".into(),
        };
        report.passed = eval::isolation::passed(&report);
        eval::isolation::store_check(&self.db, &report)?;
        crate::diag::note(
            if report.passed {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            !report.passed,
            Some(&self.project_id),
            None,
            None,
            None,
            "evaluation_isolation",
            if report.passed {
                "boundaries_verified"
            } else {
                "boundary_not_verified"
            },
            started,
        );
        Ok(report)
    }
}
