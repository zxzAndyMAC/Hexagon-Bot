//! Evaluation driver lives above the core facade, like replay/setup.
use super::*;
use crate::evaluation::{self as eval, EvaluationArm, EvaluationBatch, EvaluationResult};
use crate::provider::{ChatResponse, ContentBlock, ScriptedProvider, StopReason};

enum DriverMode {
    Scripted { owner: bool },
    Live(Arc<dyn ModelProvider>),
}

fn scripted_responses(activation: &eval::DebugActivation) -> Vec<ChatResponse> {
    let text = |value: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: value.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    // Dispatch consumes one planning reply before the tool loop (ticket 01).
    let mut responses = vec![text("Read declared task inputs before writing.")];
    for (path, body) in &activation.writes {
        for (tool, input) in [
            ("fs_read", json!({"path":path})),
            ("fs_write", json!({"path":path,"content":body})),
        ] {
            responses.push(ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: format!("eval-{}", responses.len()),
                    name: tool.into(),
                    input,
                }],
                stop: StopReason::ToolUse,
                usage: Default::default(),
            });
        }
    }
    if activation.request_baseline_merge {
        responses.push(ChatResponse {
            content: vec![ContentBlock::ToolUse {
                id: format!("eval-{}", responses.len()),
                name: "git_baseline_merge".into(),
                input: json!({}),
            }],
            stop: StopReason::ToolUse,
            usage: Default::default(),
        });
    }
    responses.push(text("Debug activation ended"));
    responses
}

impl Workbench {
    #[cfg(test)]
    pub(super) fn move_evaluation_report_binding_fixture(
        &self,
        plan: &str,
        run: &str,
        position: u32,
    ) -> Result<(), ApiError> {
        self.db.conn().execute(
            "UPDATE evaluation_plan_runs SET run_id=NULL WHERE plan_id=?1 AND run_id=?2",
            rusqlite::params![plan, run],
        )?;
        self.db.conn().execute("UPDATE evaluation_plan_runs SET run_id=?2,state='failed' WHERE plan_id=?1 AND position=?3", rusqlite::params![plan,run,position])?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn summarize_evaluation_measurements_fixture(
        &self,
        rows: &[eval::BenefitRun],
    ) -> eval::report::CategoryBenefits {
        eval::report::summarize("bug", &rows.iter().collect::<Vec<_>>())
    }

    pub fn evaluation_benefit_report(&self, batch: &str) -> Result<eval::BenefitReport, ApiError> {
        Ok(eval::report::build(&self.db, &self.repo_root, batch)?)
    }

    pub(super) fn record_evaluation_service_failure(
        &self,
        run: &EvaluationResult,
        error: &ApiError,
    ) -> Result<(), ApiError> {
        if let ApiError::Turn(turn::TurnError::Provider(error)) = error {
            if let Some(status) = error.http_status() {
                eval::supplement::record_failure(&self.db, run, status, error.request_id())?;
            }
        }
        Ok(())
    }
    pub fn supplement_evaluation_pair(
        &self,
        plan: &str,
        position: usize,
    ) -> Result<eval::EvaluationPlan, ApiError> {
        Ok(eval::supplement::create(&self.db, plan, position)?)
    }
    pub fn evaluation_supplements(
        &self,
        plan: &str,
    ) -> Result<Vec<eval::EvaluationPlan>, ApiError> {
        Ok(eval::supplement::list(&self.db, plan)?)
    }

    pub fn plan_evaluation(
        &self,
        batch: &str,
        kind: eval::PlanKind,
    ) -> Result<eval::EvaluationPlan, ApiError> {
        Ok(eval::plan::create(&self.db, batch, kind)?)
    }
    pub fn evaluation_plan(&self, id: &str) -> Result<eval::EvaluationPlan, ApiError> {
        Ok(eval::plan::read(&self.db, id)?)
    }

    pub fn stop_evaluation_plan(&self, id: &str) -> Result<eval::EvaluationPlan, ApiError> {
        Ok(eval::plan::stop(
            &self.db,
            &self.repo_root,
            id,
            eval::plan::PlanStopReason::OwnerStopped,
        )?)
    }

    /// Scripted owner decisions below are debug evidence, never human samples.
    /// Only the persisted next arm can start; the caller cannot select a winner.
    pub fn evaluate_next_debug(
        &self,
        plan: &str,
        activations: &[eval::DebugActivation],
    ) -> Result<EvaluationResult, ApiError> {
        self.evaluate_next_scripted(plan, activations, false)
    }

    pub fn evaluate_next_with_owner_debug(
        &self,
        plan: &str,
        activations: &[eval::DebugActivation],
    ) -> Result<EvaluationResult, ApiError> {
        self.evaluate_next_scripted(plan, activations, true)
    }

    fn evaluate_next_scripted(
        &self,
        plan: &str,
        activations: &[eval::DebugActivation],
        owner: bool,
    ) -> Result<EvaluationResult, ApiError> {
        let paid:bool=self.db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_budget_plans WHERE plan_id=?1 AND scope='paid_first_round')",[plan],|r|r.get(0))?;
        if paid {
            return Err(ApiError::BadInput("paid_plan_requires_live_driver".into()));
        }
        let current = eval::plan::read(&self.db, plan)?;
        let batch = self.evaluation_batch(&current.batch_id)?;
        let check = self.check_evaluation_configuration(&batch.id, &batch.request)?;
        if check.blocks.iter().any(|b| {
            matches!(
                b,
                eval::AdmissionBlock::ConfigurationDrift | eval::AdmissionBlock::RuntimeDrift
            )
        }) {
            return Err(ApiError::BadInput("evaluation configuration drift".into()));
        }
        let (current, position, id, lease) = eval::plan::claim(&self.db, &self.repo_root, plan)?;
        let entry = &current.entries[position];
        let task = &batch
            .request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .ok_or_else(|| ApiError::BadInput("planned task missing".into()))?
            .task;
        let result = self.run_evaluation_mode(
            task,
            &id,
            Some((&batch, entry.arm)),
            activations,
            DriverMode::Scripted { owner },
            Some(lease),
        )?;
        if result.state != "waiting_human" {
            eval::plan::finish(&self.db, plan, position, &id)?;
            eval::candidate::refresh_finished_plan(&self.db, &self.repo_root, plan)?;
        }
        Ok(result)
    }

    pub fn evaluate_next_live(&self, plan: &str) -> Result<EvaluationResult, ApiError> {
        let current = eval::plan::read(&self.db, plan)?;
        let batch = self.evaluation_batch(&current.batch_id)?;
        self.enable_evaluation_live(plan)?;
        let provider = self.frozen_evaluation_provider(&batch)?;
        if provider.is_scripted() {
            return Err(ApiError::BadInput("live_driver_requires_transport".into()));
        }
        let (current, position, id, lease) = eval::plan::claim(&self.db, &self.repo_root, plan)?;
        let entry = &current.entries[position];
        let task = &batch
            .request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .ok_or_else(|| ApiError::BadInput("planned_task_missing".into()))?
            .task;
        let result = self.run_evaluation_mode(
            task,
            &id,
            Some((&batch, entry.arm)),
            &[],
            DriverMode::Live(provider),
            Some(lease),
        )?;
        if result.state != "waiting_human" {
            eval::plan::finish(&self.db, plan, position, &id)?;
            eval::candidate::refresh_finished_plan(&self.db, &self.repo_root, plan)?;
        }
        Ok(result)
    }

    pub(super) fn run_scripted_evaluation(
        &self,
        task: &eval::EvaluationTask,
        id: &str,
        frozen: Option<(&EvaluationBatch, EvaluationArm)>,
        activations: &[eval::DebugActivation],
    ) -> Result<EvaluationResult, ApiError> {
        self.run_evaluation_mode(
            task,
            id,
            frozen,
            activations,
            DriverMode::Scripted { owner: false },
            None,
        )
    }

    fn run_evaluation_mode(
        &self,
        task: &eval::EvaluationTask,
        id: &str,
        frozen: Option<(&EvaluationBatch, EvaluationArm)>,
        activations: &[eval::DebugActivation],
        mode: DriverMode,
        lease: Option<std::fs::File>,
    ) -> Result<EvaluationResult, ApiError> {
        let (owner, live_provider) = match mode {
            DriverMode::Scripted { owner } => (owner, None),
            DriverMode::Live(provider) => (true, Some(provider)),
        };
        let _owner = match lease {
            Some(file) => file,
            None => eval::recovery::new_lease(&self.db, &self.repo_root, "driver", id, id)?,
        };
        use sha2::{Digest, Sha256};
        let parent = self.repo_root.join(".hexagon/evaluation-runs");
        std::fs::create_dir_all(&parent)?;
        let copy = tempfile::Builder::new()
            .prefix(&format!("{id}-"))
            .tempdir_in(parent)?
            .keep();
        let mut result = EvaluationResult {
            version: 1,
            id: id.into(),
            task_id: task.id.clone(),
            task_fingerprint: format!("v1:{:x}", Sha256::digest(serde_json::to_vec(task)?)),
            workspace: copy.to_string_lossy().into_owned(),
            evidence_kind: if live_provider.is_some() {
                if cfg!(test) {
                    "provider_boundary_fixture"
                } else {
                    "live_model"
                }
            } else {
                "scripted_debug"
            }
            .into(),
            state: "started".into(),
            flow_completed: false,
            independent_passed: false,
            owner_exception: false,
            acceptance: None,
            git_baseline: None,
            elapsed_ms: 0,
            error: None,
        };
        eval::insert(&self.db, task, &result)?;
        eval::control::install(
            &self.db,
            &self.repo_root,
            &result,
            frozen
                .map(|(b, _)| b.request.limits.active_ms)
                .unwrap_or(1_800_000),
        )?;
        let started_at = eval::now_ms()?;
        let started = std::time::Instant::now();
        let execute = (|| -> Result<(), ApiError> {
            if frozen.is_some() {
                eval::self_check(task)?;
            }
            for activation in activations {
                if activation
                    .writes
                    .keys()
                    .any(|p| !eval::safe_path(p) || !task.allowed_paths.contains(p))
                {
                    return Err(ApiError::BadInput(
                        "debug write outside declared task scope".into(),
                    ));
                }
            }
            result.git_baseline = eval::reconstruct(&copy, task)?;
            eval::budget::bind_debug(&self.db, &self.repo_root, &copy, id)?;
            eval::control::apply_stop(&self.db, &mut result)?;
            eval::update_started(&self.db, &result)?;
            let full_pack = frozen
                .filter(|(_, arm)| *arm == EvaluationArm::Full)
                .map(|(b, _)| b.request.full_pack.clone());
            let roles: Vec<(String, String)> = match frozen {
                Some((batch, arm)) => {
                    let mut names = std::collections::BTreeSet::new();
                    if arm == EvaluationArm::Fast {
                        let mut next = Some(batch.request.fast_role.clone());
                        while let Some(name) = next {
                            if !names.insert(name.clone()) {
                                break;
                            }
                            next = batch.runtime.roles[&name].reviewer.clone();
                        }
                    } else {
                        names.extend(batch.runtime.roles.keys().cloned());
                    }
                    names
                        .into_iter()
                        .enumerate()
                        .map(|(i, name)| (format!("a{i}"), name))
                        .collect()
                }
                None => vec![("a0".into(), "后端".into())],
            };
            std::fs::create_dir_all(copy.join(".hexagon"))?;
            std::fs::write(copy.join(".hexagon/evaluation-worker"), "private-state-v1")?;
            let mut worker =
                Workbench::open_scoped(&copy, &task.id, &roles, full_pack.clone(), false)?;
            if let Some((batch, _)) = frozen {
                for (aid, name) in &roles {
                    let def = &batch.runtime.roles[name];
                    // D03/D04: an explicit pre-frozen owner assignment adds
                    // exact task paths; never clear ordinary ownership checks.
                    let mut globs = def.globs.clone();
                    if batch.request.task_owners.contains(name) {
                        globs.extend(task.allowed_paths.clone());
                    }
                    globs.sort();
                    globs.dedup();
                    crate::roles::update_agent_def(
                        &worker.db,
                        &worker.project_id,
                        aid,
                        &crate::roles::AgentPatch {
                            duty: Some(def.duty.clone()),
                            reviewer: Some(def.reviewer.clone().unwrap_or_default()),
                            model_slot: Some(batch.request.main_slot.clone()),
                            skills: Some(def.skills.clone()),
                            globs: Some(globs),
                        },
                    )?;
                }
                crate::autonomy::set_reviewer_mode(
                    &worker.db,
                    &worker.project_id,
                    &batch.runtime.host_policy.reviewer_mode,
                )?;
            }
            if full_pack.is_some() {
                worker.open_stage(0)?;
            } else {
                let role = frozen
                    .map(|(batch, _)| batch.request.fast_role.as_str())
                    .unwrap_or("后端");
                let aid = worker.agent_by_role(role)?;
                worker.db.conn().execute(
                    "UPDATE projects SET mode='fastpath',fastpath_agent_id=?2 WHERE id=?1",
                    rusqlite::params![worker.project_id, aid],
                )?;
            }
            if let Some(provider) = &live_provider {
                worker.providers.clear();
                worker.register_provider(
                    &frozen
                        .ok_or_else(|| ApiError::BadInput("live_batch_missing".into()))?
                        .0
                        .request
                        .main_slot,
                    provider.clone(),
                );
            }
            let mut cursor = EvaluationCursor {
                live: live_provider.is_some(),
                roles,
                pack: full_pack,
                slot: frozen
                    .map(|(b, _)| b.request.main_slot.clone())
                    .unwrap_or_else(|| "default".into()),
                fast_role: frozen
                    .map(|(b, _)| b.request.fast_role.clone())
                    .unwrap_or_else(|| "后端".into()),
                activations: activations.to_vec(),
                stage: 0,
                next_activation: 0,
                await_owner_decision: owner,
                role_index: 0,
                permission: None,
                rework: false,
            };
            self.db.conn().execute("INSERT INTO evaluation_cursors(run_id,cursor_json,started_at_ms) VALUES (?1,?2,?3)",rusqlite::params![id,serde_json::to_string(&cursor)?,i64::try_from(started_at).map_err(std::io::Error::other)?])?;
            let _watch =
                eval::control::watch(&copy, worker.sessions.clone(), worker.tasks.clone())?;
            #[cfg(test)]
            worker.inject_evaluation_service_failure_fixture()?;
            drive_evaluation(&mut worker, task, &mut cursor, &mut result)?;
            self.save_evaluation_cursor(&cursor, &result)?;
            Ok(())
        })();
        // D11: external failure receipt and terminal result share one host
        // transaction; a crash cannot leave only half of the eligibility fact.
        let terminal_tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Err(error) = execute {
            self.record_evaluation_service_failure(&result, &error)?;
            result.state = "failed".into();
            result.error = Some(error.to_string());
        }
        result.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        eval::control::apply_stop(&self.db, &mut result)?;
        eval::update_started(&self.db, &result)?;
        terminal_tx.commit()?;
        crate::diag::note(
            if matches!(result.state.as_str(), "completed" | "waiting_human") {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            !matches!(result.state.as_str(), "completed" | "waiting_human"),
            Some(&self.project_id),
            None,
            None,
            None,
            if live_provider.is_some() {
                "evaluation_live_run"
            } else {
                "evaluation_scripted_run"
            },
            &format!("{}:{}", result.state, result.id),
            started,
        );
        Ok(result)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct EvaluationCursor {
    #[serde(default)]
    pub live: bool,
    pub roles: Vec<(String, String)>,
    pub pack: Option<PackDef>,
    pub slot: String,
    pub fast_role: String,
    pub activations: Vec<eval::DebugActivation>,
    pub stage: usize,
    pub next_activation: usize,
    pub await_owner_decision: bool,
    pub role_index: usize,
    pub permission: Option<(String, usize)>,
    pub rework: bool,
}

pub(super) fn drive_evaluation(
    worker: &mut Workbench,
    task: &eval::EvaluationTask,
    cursor: &mut EvaluationCursor,
    result: &mut EvaluationResult,
) -> Result<(), ApiError> {
    let stages: Vec<Vec<String>> = match &cursor.pack {
        Some(pack) => pack
            .stages
            .iter()
            .map(|s| {
                let mut roles = s.roles.clone();
                let mut reviewers = std::collections::BTreeSet::new();
                // 2026-09-28 pilot eval-2: dispatching only producers stopped at
                // the first review gate. Reviewers run after all producers, even
                // if they also produced work; advance still checks current evidence.
                for review in &s.reviews {
                    if reviewers.insert(&review.reviewer) {
                        roles.push(review.reviewer.clone());
                    }
                }
                roles
            })
            .collect(),
        None => vec![vec![cursor.fast_role.clone()]],
    };
    result.state = "started".into();
    while cursor.stage < stages.len() {
        while cursor.role_index < stages[cursor.stage].len() {
            let role = &stages[cursor.stage][cursor.role_index];
            let consumed = cursor.permission.as_ref().map(|(_, n)| *n).unwrap_or(0);
            let provider = if cursor.live {
                None
            } else {
                let activation = cursor
                    .activations
                    .get(cursor.next_activation)
                    .ok_or_else(|| ApiError::BadInput("missing scripted activation".into()))?;
                if activation.role != *role {
                    return Err(ApiError::BadInput(
                        "scripted role differs from frozen execution order".into(),
                    ));
                }
                let provider = Arc::new(ScriptedProvider::new(
                    scripted_responses(activation)
                        .into_iter()
                        .skip(consumed)
                        .collect(),
                ));
                worker.providers.clear();
                worker.register_provider(&cursor.slot, provider.clone());
                Some(provider)
            };
            let aid = worker.agent_by_role(role)?;
            let reviewing = cursor
                .pack
                .as_ref()
                .is_some_and(|pack| cursor.role_index >= pack.stages[cursor.stage].roles.len());
            let instruction = if reviewing {
                let kinds: Vec<_> = cursor.pack.as_ref().unwrap().stages[cursor.stage]
                    .reviews
                    .iter()
                    .filter(|r| r.reviewer == *role)
                    .map(|r| &r.artifact_kind)
                    .collect();
                json!({
                    "assignment": "Independently review each current-stage artifact of the declared review_kinds. Read each target with artifact_read and submit a review artifact with its exact review_target as target_evidence and your pass/reject verdict. Do not implement, self-approve, skip review, or advance the stage.",
                    "review_kinds": kinds,
                    "task_requirements": task.requirements,
                }).to_string()
            } else {
                task.requirements.clone()
            };
            let review_stage = if reviewing {
                worker
                    .db
                    .active_stage_run(&worker.project_id)?
                    .map(|r| r.id)
            } else {
                None
            };
            let started = std::time::Instant::now();
            let outcome = if consumed > 0 {
                let resume_run = worker.active_run()?.map(|run| run.id);
                let resume = worker.activation_resume(&aid, resume_run.as_deref(), None)?;
                worker.run_turn_agent_with_history(
                    &aid,
                    &resume.instruction,
                    &[],
                    false,
                    &resume.history,
                    Some(&resume),
                    false,
                )?
            } else {
                worker.dispatch_instance_with_followup(&aid, &instruction, &[], false)?
            };
            if reviewing {
                crate::diag::note(
                    crate::diag::CLASS_JUDGE,
                    false,
                    Some(&worker.project_id),
                    Some(&aid),
                    review_stage.as_deref(),
                    None,
                    "evaluation_review",
                    "declared_reviewer_dispatched",
                    started,
                );
            }
            match outcome {
                TurnOutcome::AwaitingPermission(question_id) => {
                    // D07: retain response offset, role and activation. Replaying
                    // the scripted tool would create a second side-effect intent.
                    cursor.permission = Some((
                        question_id,
                        consumed + provider.as_ref().map(|p| p.recorded().len()).unwrap_or(1),
                    ));
                    result.state = "waiting_human".into();
                    return Ok(());
                }
                TurnOutcome::Finished => {
                    cursor.permission = None;
                    cursor.next_activation += 1;
                    cursor.role_index += 1;
                }
                _ => {
                    result.state = "incomplete".into();
                    break;
                }
            }
        }
        if result.state == "incomplete" {
            break;
        }
        cursor.stage += 1;
        cursor.role_index = 0;
        if cursor.pack.is_some() {
            let mut action = worker.advance()?;
            if matches!(
                action,
                orchestra::StageAction::AwaitingStamp { .. }
                    | orchestra::StageAction::WaitingStamp { .. }
            ) {
                if cursor.await_owner_decision {
                    result.state = "waiting_human".into();
                    return Ok(());
                }
                action = worker.stamp()?;
            }
            match action {
                orchestra::StageAction::StageOpened { .. } => {}
                orchestra::StageAction::PackFinished => {
                    result.flow_completed = true;
                    break;
                }
                _ => {
                    result.state = "incomplete".into();
                    break;
                }
            }
        }
    }
    if result.state == "started" && cursor.await_owner_decision && cursor.pack.is_none() {
        // D07: fast-path owners also inspect the delivered files. This review
        // closes evaluation only; it does not invent a pack stamp or merge.
        result.state = "waiting_human".into();
        return Ok(());
    }
    if result.state == "started" {
        result.state = "completed".into();
    }
    let acceptance = eval::accept(
        &worker.db,
        Path::new(&result.workspace),
        task,
        result.git_baseline.as_ref(),
    )?;
    result.independent_passed = acceptance.passed;
    result.acceptance = Some(acceptance);
    Ok(())
}

impl Workbench {
    pub(super) fn save_evaluation_cursor(
        &self,
        cursor: &EvaluationCursor,
        result: &EvaluationResult,
    ) -> Result<(), ApiError> {
        let now = i64::try_from(eval::now_ms()?).map_err(std::io::Error::other)?;
        let fingerprint = eval::fingerprint(Path::new(&result.workspace))?;
        self.db.conn().execute("UPDATE evaluation_cursors SET cursor_json=?2,wait_fingerprint=?5,waiting_since_ms=CASE WHEN ?3='waiting_human' THEN ?4 ELSE NULL END,ended_at_ms=CASE WHEN ?3 IN ('completed','incomplete','failed') THEN ?4 ELSE NULL END WHERE run_id=?1",rusqlite::params![result.id,serde_json::to_string(cursor)?,result.state,now,fingerprint])?;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {static SERVICE_FAULT:std::cell::Cell<Option<(u16,u8)>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
impl Workbench {
    pub(super) fn arm_evaluation_service_failure_fixture(&self, status: u16) {
        SERVICE_FAULT.set(Some((status, 0)));
    }
    pub(super) fn arm_interleaved_evaluation_service_failure_fixture(&self) {
        SERVICE_FAULT.set(Some((503, 1)));
    }
    pub(super) fn arm_unbound_evaluation_service_failure_fixture(&self) {
        SERVICE_FAULT.set(Some((503, 2)));
    }
    fn inject_evaluation_service_failure_fixture(&self) -> Result<(), ApiError> {
        if let Some((status, mode)) = SERVICE_FAULT.take() {
            let provider = ScriptedProvider::new(vec![]);
            let ctx = self.ctx_for("a0", None);
            let response = crate::usage::request(
                &self.db,
                &ctx,
                "default",
                "service_failure_fixture",
                &provider,
                None,
                || {
                    Err(crate::provider::ProviderError::WithUsage(
                        Box::new(crate::provider::ProviderError::HttpStatus(
                            Box::new(crate::provider::ProviderError::Transport(
                                "HTTP boundary fixture".into(),
                            )),
                            status,
                            None,
                        )),
                        crate::provider::Usage {
                            prompt_tokens: 100,
                            completion_tokens: 50,
                            prompt_reported: true,
                            completion_reported: true,
                            ..Default::default()
                        },
                    ))
                },
            );
            if mode == 1 {
                crate::usage::request(
                    &self.db,
                    &ctx,
                    "default",
                    "later_independent_request",
                    &provider,
                    None,
                    || {
                        Ok(ChatResponse {
                            content: vec![],
                            stop: StopReason::EndTurn,
                            usage: crate::provider::Usage {
                                prompt_reported: true,
                                completion_reported: true,
                                ..Default::default()
                            },
                        })
                    },
                )
                .map_err(turn::TurnError::Provider)?;
            }
            response.map_err(|mut error| {
                if mode == 2 {
                    error.bind_request("missing-fixture-request");
                }
                turn::TurnError::Provider(error)
            })?;
        }
        Ok(())
    }
}
