use super::*;
use crate::evaluation::{self as eval, CandidateEvaluation, CandidateSource};
impl Workbench {
    /// Existing proposals may be evaluated for regression, but a context label
    /// cannot retroactively confer isolated generation provenance.
    pub fn freeze_policy_evaluation(
        &self,
        proposal: &str,
        generation: &str,
    ) -> Result<CandidateSource, ApiError> {
        Ok(eval::candidate::freeze(
            &self.db,
            &self.ctx_for("owner", None),
            proposal,
            generation,
        )?)
    }
    pub fn policy_evaluation(&self, proposal: &str) -> Result<CandidateEvaluation, ApiError> {
        Ok(eval::candidate::inspect(
            &self.db,
            &self.ctx_for("owner", None),
            proposal,
        )?)
    }
}

impl Workbench {
    pub fn policy_generation(&self, context: &str) -> Result<eval::PolicyGeneration, ApiError> {
        Ok(eval::generation::read(&self.db, context)?)
    }

    /// A supplier boundary fixture executes the same isolated request and host
    /// capture path. Its source remains scripted and cannot qualify adoption.
    pub fn generate_policy_candidate_debug(
        &self,
        context: &str,
        edits: &[serde_json::Value],
    ) -> Result<CandidateSource, ApiError> {
        let output = serde_json::to_string(&json!({"edits":edits}))?;
        let provider = Arc::new(crate::provider::ScriptedProvider::new(vec![
            crate::provider::ChatResponse {
                content: vec![crate::provider::ContentBlock::Text { text: output }],
                stop: crate::provider::StopReason::EndTurn,
                usage: crate::provider::Usage {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    prompt_reported: true,
                    completion_reported: true,
                    ..Default::default()
                },
            },
        ]));
        self.generate_policy_with_provider(context, provider)
    }

    pub fn generate_policy_candidate(&self, context: &str) -> Result<CandidateSource, ApiError> {
        let started = std::time::Instant::now();
        let generation = self.evaluation_generation(context)?;
        let batch = self.evaluation_batch(&generation.batch_id)?;
        if !self
            .check_evaluation_configuration(&batch.id, &batch.request)?
            .ready
        {
            return Err(generation_refusal(
                &self.project_id,
                "live_configuration_unverified",
                started,
            ));
        }
        let provider = self.frozen_evaluation_provider(&batch)?;
        if provider.is_scripted() || provider.uses_decision_api() {
            return Err(generation_refusal(
                &self.project_id,
                "generation_live_chat_required",
                started,
            ));
        }
        self.generate_policy_with_provider(context, provider)
    }

    fn generate_policy_with_provider(
        &self,
        context_id: &str,
        provider: Arc<dyn ModelProvider>,
    ) -> Result<CandidateSource, ApiError> {
        use eval::generation;
        let started = std::time::Instant::now();
        let context = self.evaluation_generation(context_id)?;
        let batch = self.evaluation_batch(&context.batch_id)?;
        let checked = self.check_evaluation_configuration(&batch.id, &batch.request)?;
        if !context.eligible_for_generation
            || checked.blocks.iter().any(|b| {
                matches!(
                    b,
                    eval::AdmissionBlock::ConfigurationDrift
                        | eval::AdmissionBlock::RuntimeDrift
                        | eval::AdmissionBlock::HeldoutRetired
                )
            })
        {
            return Err(generation_refusal(
                &self.project_id,
                "generation_inputs_stale_or_disclosed",
                started,
            ));
        }
        let author = crate::policydev::policy_dev_agent(&self.db, &self.project_id)?;
        let baseline = PackDef::pinned(&self.repo_root)?;
        if !generation::same_pack(&baseline, &batch.request.full_pack)? {
            return Err(generation_refusal(
                &self.project_id,
                "generation_baseline_mismatch",
                started,
            ));
        }
        let exists: bool = self.db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_policy_generations WHERE context_id=?1)",
            [context_id],
            |r| r.get(0),
        )?;
        if exists {
            return Err(generation_refusal(
                &self.project_id,
                "generation_already_started",
                started,
            ));
        }
        let request = generation::request(&self.db, context_id)?;
        let id = format!(
            "policy-generation-{}",
            self.db.next_id("policy_generation")?
        );
        let parent = self.repo_root.join(".hexagon/evaluation-generators");
        std::fs::create_dir_all(&parent)?;
        let workspace = tempfile::Builder::new()
            .prefix(&id)
            .tempdir_in(parent)?
            .keep();
        std::fs::create_dir_all(workspace.join(".hexagon"))?;
        std::fs::write(
            workspace.join(".hexagon/evaluation-worker"),
            "private-state-v1",
        )?;
        let worker = Workbench::open_scoped(
            &workspace,
            "Isolated policy generation",
            &[("generator".into(), crate::policydev::ROLE.into())],
            None,
            false,
        )?;
        let mut op = eval::PolicyGeneration {
            id,
            context_id: context_id.into(),
            batch_id: batch.id.clone(),
            batch_fingerprint: batch.fingerprint.clone(),
            input_fingerprint: eval::config::digest(&(
                &request.model_slot,
                &request.messages,
                &request.tools,
            ))?,
            output_fingerprint: None,
            workspace: workspace.canonicalize()?.to_string_lossy().into_owned(),
            state: "started".into(),
            source_kind: if provider.is_scripted() {
                "scripted_generation"
            } else if cfg!(test) {
                "boundary_generation"
            } else {
                "live_generation"
            }
            .into(),
            request_id: None,
            proposal_id: None,
            reason: None,
        };
        let start_tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let _activity =
            eval::recovery::new_lease(&self.db, &self.repo_root, "activity", &op.id, &op.id)?;
        self.db.conn().execute("INSERT INTO evaluation_policy_generations(id,context_id,operation_json,fingerprint) VALUES (?1,?2,?3,?4)",rusqlite::params![op.id,context_id,serde_json::to_string(&op)?,eval::config::digest(&op)?])?;
        start_tx.commit()?;
        let result = (|| -> Result<CandidateSource, ApiError> {
            if provider.is_scripted() {
                eval::budget::bind_debug_activity(
                    &self.db,
                    &self.repo_root,
                    &workspace,
                    &op.id,
                    &batch.request.limits,
                    &eval::DebugPrice {
                        prompt_per_1k_mc: 1,
                        completion_per_1k_mc: 1,
                        prompt_bound: 1_000_000,
                        output_bound: 4096,
                    },
                )?;
            } else {
                eval::budget::live::bind_activity(
                    &self.repo_root,
                    &workspace,
                    &op.id,
                    &batch.request.limits,
                    &eval::live::price(&batch)?,
                    false,
                    1,
                )?;
            }
            #[cfg(test)]
            eval::live::activity_crash("reserved");
            let _boundary = eval::control::probe(&workspace)?;
            let response = crate::usage::complete_project_request(
                &worker.ctx_for("generator", None),
                provider.as_ref(),
                &request,
                "policy_generation",
            )
            .map_err(turn::TurnError::Provider);
            // This private worker performs exactly one generation request. Query
            // all its receipts, never the latest row of a shared project ledger.
            let mut query=worker.db.conn().prepare("SELECT request_id FROM usage WHERE purpose='policy_generation' AND record_kind='request'")?;
            let ids = query
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if ids.len() != 1 {
                return Err(ApiError::BadInput(
                    "generation_request_binding_missing".into(),
                ));
            }
            op.request_id = ids.into_iter().next();
            generation::save(&self.db, &op, None)?;
            #[cfg(test)]
            eval::live::activity_crash("returned");
            let response = response?;
            let raw = generation::closed_output(&response)?;
            let edits = generation::edits(&raw)?;
            op.output_fingerprint = Some(eval::config::digest(&raw)?);
            op.state = "response_received".into();
            generation::save(&self.db, &op, Some(&raw))?;
            let mut roles = std::collections::BTreeSet::new();
            for stage in &baseline.stages {
                roles.extend(stage.roles.iter().cloned());
            }
            let scenario = crate::scenario::Scenario {
                roles: roles.into_iter().collect(),
                pack: Some(baseline.clone()),
                scripts: Default::default(),
                steps: vec![],
            };
            let body = crate::policydev::prepare_body(
                &self.ctx_for(&author, None),
                &baseline,
                &edits,
                &scenario,
                "隔离开发材料生成的候选；回放仅作结构诊断，质量与收益等待独立验收。",
                &workspace.join("replay"),
            )?;
            let author_ctx = self.ctx_for(&author, None);
            let artifact = crate::artifacts::deliver(
                &self.db,
                &author_ctx,
                &author_ctx.tiers,
                &format!("proposals/{}.md", op.id),
                &body,
                Some("改进提案"),
            )?;
            let tx = rusqlite::Transaction::new_unchecked(
                self.db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )?;
            // Revalidate disclosure/input/config after IO, before producing any
            // trusted source binding. A racing reveal cannot be erased by commit.
            let fresh = generation::request(&self.db, context_id)?;
            if eval::config::digest(&(&fresh.model_slot, &fresh.messages, &fresh.tools))?
                != op.input_fingerprint
            {
                return Err(ApiError::BadInput("generation_inputs_changed".into()));
            }
            let proposal = crate::proposals::submit(&self.db, &author_ctx, &artifact, &body)?;
            let mut source = eval::candidate::freeze(
                &self.db,
                &self.ctx_for("owner", None),
                &proposal,
                context_id,
            )?;
            source.source_kind = op.source_kind.clone();
            source.generation_operation = Some(op.id.clone());
            self.db.conn().execute("UPDATE evaluation_candidates SET source_json=?2,fingerprint=?3 WHERE proposal_id=?1",rusqlite::params![proposal,serde_json::to_string(&source)?,eval::config::digest(&source)?])?;
            op.state = "completed".into();
            op.proposal_id = Some(proposal);
            generation::save(&self.db, &op, None)?;
            tx.commit()?;
            #[cfg(test)]
            eval::live::activity_crash("committed");
            Ok(source)
        })();
        if provider.is_scripted() {
            eval::budget::finish_debug_activity(&self.db, &op.id)?;
        } else {
            eval::budget::live::finish_activity(&self.repo_root, &workspace, &op.id)?;
        }
        if result.is_err() {
            op.state = "failed".into();
            op.proposal_id = None;
            op.reason = Some("generation_not_committed".into());
            generation::save(&self.db, &op, None)?;
        }
        crate::diag::note(
            if result.is_ok() {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            result.is_err(),
            Some(&self.project_id),
            Some(&author),
            None,
            None,
            "policy_generation",
            if result.is_ok() {
                "source_captured"
            } else {
                "generation_not_committed"
            },
            started,
        );
        result
    }
}

fn generation_refusal(project: &str, code: &str, started: std::time::Instant) -> ApiError {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(project),
        Some("owner"),
        None,
        None,
        "policy_generation",
        code,
        started,
    );
    ApiError::BadInput(code.into())
}
