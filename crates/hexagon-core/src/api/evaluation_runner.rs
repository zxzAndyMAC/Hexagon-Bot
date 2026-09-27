//! Evaluation driver lives above the core facade, like replay/setup.
use super::*;
use crate::evaluation::{self as eval, EvaluationArm, EvaluationBatch, EvaluationResult};
use crate::provider::{ChatResponse, ContentBlock, ScriptedProvider, StopReason};
use std::collections::BTreeMap;

fn scripted_responses(writes: &BTreeMap<String, String>) -> Vec<ChatResponse> {
    let text = |value: &str| ChatResponse {
        content: vec![ContentBlock::Text { text: value.into() }],
        stop: StopReason::EndTurn,
        usage: Default::default(),
    };
    // Dispatch consumes one planning reply before the tool loop (ticket 01).
    let mut responses = vec![text("Read declared task inputs before writing.")];
    for (path, body) in writes {
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
    responses.push(text("Debug activation ended"));
    responses
}

impl Workbench {
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
        let (current, position, id) = eval::plan::claim(&self.db, plan)?;
        let entry = &current.entries[position];
        let task = &batch
            .request
            .corpora
            .iter()
            .flat_map(|c| &c.cases)
            .find(|c| c.task.id == entry.task_id)
            .ok_or_else(|| ApiError::BadInput("planned task missing".into()))?
            .task;
        let result =
            self.run_scripted_evaluation(task, &id, Some((&batch, entry.arm)), activations)?;
        eval::plan::finish(&self.db, plan, position, &id)?;
        Ok(result)
    }

    pub(super) fn run_scripted_evaluation(
        &self,
        task: &eval::EvaluationTask,
        id: &str,
        frozen: Option<(&EvaluationBatch, EvaluationArm)>,
        activations: &[eval::DebugActivation],
    ) -> Result<EvaluationResult, ApiError> {
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
            evidence_kind: "scripted_debug".into(),
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
            let mut finished = true;
            let mut next_activation = 0;
            let stages: Vec<Vec<String>> = match &full_pack {
                Some(pack) => pack.stages.iter().map(|s| s.roles.clone()).collect(),
                None => vec![vec![frozen
                    .map(|(b, _)| b.request.fast_role.clone())
                    .unwrap_or_else(|| "后端".into())]],
            };
            for stage_roles in stages {
                for role in stage_roles {
                    let activation = activations
                        .get(next_activation)
                        .ok_or_else(|| ApiError::BadInput("missing scripted activation".into()))?;
                    if activation.role != role {
                        return Err(ApiError::BadInput(
                            "scripted role differs from frozen execution order".into(),
                        ));
                    }
                    next_activation += 1;
                    let provider = Arc::new(ScriptedProvider::new(scripted_responses(
                        &activation.writes,
                    )));
                    worker.providers.clear();
                    let slot = frozen
                        .map(|(b, _)| b.request.main_slot.as_str())
                        .unwrap_or("default");
                    worker.register_provider(slot, provider);
                    let aid = worker.agent_by_role(&role)?;
                    if !matches!(
                        worker.dispatch_instance(&aid, &task.requirements, &[])?,
                        TurnOutcome::Finished
                    ) {
                        finished = false;
                        break;
                    }
                }
                if !finished {
                    break;
                }
                if full_pack.is_some() {
                    let mut action = worker.advance()?;
                    if matches!(
                        action,
                        orchestra::StageAction::AwaitingStamp { .. }
                            | orchestra::StageAction::WaitingStamp { .. }
                    ) {
                        action = worker.stamp()?;
                    }
                    match action {
                        orchestra::StageAction::StageOpened { .. } => {}
                        orchestra::StageAction::PackFinished => {
                            result.flow_completed = true;
                            break;
                        }
                        _ => {
                            finished = false;
                            break;
                        }
                    }
                }
            }
            result.state = if finished { "completed" } else { "incomplete" }.into();
            let acceptance = eval::accept(&worker.db, &copy, task, result.git_baseline.as_ref())?;
            result.independent_passed = acceptance.passed;
            result.acceptance = Some(acceptance);
            Ok(())
        })();
        if let Err(error) = execute {
            result.state = "failed".into();
            result.error = Some(error.to_string());
        }
        result.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        eval::update_started(&self.db, &result)?;
        crate::diag::note(
            if result.state == "completed" {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            result.state != "completed",
            Some(&self.project_id),
            None,
            None,
            None,
            "evaluation_scripted_run",
            &format!("{}:{}", result.state, result.id),
            started,
        );
        Ok(result)
    }
}
