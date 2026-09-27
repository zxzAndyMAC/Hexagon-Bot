use super::*;
use crate::evaluation::{self as eval, PreflightReceipt};
use crate::provider::{ContentBlock, Message, Role, StopReason};

impl Workbench {
    pub(super) fn frozen_evaluation_provider(
        &self,
        batch: &eval::EvaluationBatch,
    ) -> Result<Arc<dyn ModelProvider>, ApiError> {
        #[cfg(test)]
        if let Some(provider) = self.providers.get(&batch.request.main_slot) {
            return Ok(provider.clone());
        }
        let current = crate::provider_config::load()
            .map_err(|_| ApiError::BadInput("preflight_provider_configuration_invalid".into()))?;
        let binding = current
            .slots
            .get(&batch.request.main_slot)
            .ok_or_else(|| ApiError::BadInput("evaluation_model_slot_missing".into()))?;
        let definition = current
            .providers
            .iter()
            .find(|p| p.id == binding.provider_id && p.enabled)
            .ok_or_else(|| ApiError::BadInput("evaluation_provider_missing".into()))?;
        let frozen = batch
            .runtime
            .models
            .get(&batch.request.main_slot)
            .ok_or_else(|| ApiError::BadInput("evaluation_model_not_frozen".into()))?;
        if binding.model != frozen.model || binding.provider_id != frozen.provider_id {
            return Err(ApiError::BadInput("evaluation_model_changed".into()));
        }
        // Only this provider object retains host transport credentials. The
        // worker credential store, MCP configuration and history remain empty.
        Ok(crate::provider_config::make_provider(
            definition,
            &binding.model,
            self.creds.clone(),
        ))
    }

    pub fn evaluation_preflight(&self, id: &str) -> Result<PreflightReceipt, ApiError> {
        Ok(eval::live::read(&self.db, id)?)
    }
    pub fn enable_evaluation_live(&self, plan: &str) -> Result<(), ApiError> {
        Ok(eval::live::enable_plan(&self.db, &self.project_id, plan)?)
    }

    pub fn preflight_evaluation(&self, batch_id: &str) -> Result<PreflightReceipt, ApiError> {
        let started = std::time::Instant::now();
        let batch = self.evaluation_batch(batch_id)?;
        let checked = self.check_evaluation_configuration(batch_id, &batch.request)?;
        if checked.blocks.iter().any(|b| {
            matches!(
                b,
                eval::AdmissionBlock::ConfigurationDrift
                    | eval::AdmissionBlock::RuntimeDrift
                    | eval::AdmissionBlock::ModelNotConfigured
                    | eval::AdmissionBlock::EnvironmentNotVerified
            )
        }) {
            return Err(preflight_refusal(
                &self.project_id,
                "preflight_configuration_blocked",
                started,
            ));
        }
        let price = eval::live::price(&batch)?;
        let provider = self.frozen_evaluation_provider(&batch)?;
        if provider.is_scripted() || provider.uses_decision_api() {
            return Err(preflight_refusal(
                &self.project_id,
                "preflight_chat_transport_required",
                started,
            ));
        }
        let isolation = self.check_evaluation_isolation()?;
        if !isolation.passed {
            return Err(preflight_refusal(
                &self.project_id,
                "preflight_isolation_unproven",
                started,
            ));
        }
        let id = format!("preflight-{}", self.db.next_id("evaluation_preflight")?);
        let parent = self.repo_root.join(".hexagon/evaluation-preflights");
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
            "Model preflight",
            &[("probe".into(), "后端".into())],
            None,
            false,
        )?;
        let mut receipt = PreflightReceipt {
            id,
            batch_id: batch.id.clone(),
            batch_fingerprint: batch.fingerprint.clone(),
            host: self
                .repo_root
                .canonicalize()?
                .to_string_lossy()
                .into_owned(),
            workspace: workspace.canonicalize()?.to_string_lossy().into_owned(),
            evidence_kind: if cfg!(test) {
                "provider_boundary_fixture"
            } else {
                "live_model"
            }
            .into(),
            state: "started".into(),
            request_ids: vec![],
            observed_models: vec![],
            tools_passed: false,
            price_fingerprint: price.source_fingerprint.clone(),
            isolation_id: isolation.id,
            reason: None,
            nonce: String::new(),
            exchanges: vec![],
        };
        let start_tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let _activity = eval::recovery::new_lease(
            &self.db,
            &self.repo_root,
            "activity",
            &receipt.id,
            &receipt.id,
        )?;
        self.db.conn().execute(
            "INSERT INTO evaluation_preflights(id,batch_id,receipt_json,fingerprint) VALUES (?1,?2,?3,?4)",
            rusqlite::params![receipt.id, batch.id, serde_json::to_string(&receipt)?,eval::config::digest(&receipt)?],
        )?;
        start_tx.commit()?;
        let result = (|| -> Result<(), ApiError> {
            eval::budget::live::bind_activity(
                &self.repo_root,
                &workspace,
                &receipt.id,
                &batch.request.limits,
                &price,
                true,
                2,
            )?;
            #[cfg(test)]
            eval::live::activity_crash("reserved");
            let _boundary = eval::control::probe(&workspace)?;
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|e| ApiError::BadInput(e.to_string()))?;
            let nonce = random
                .iter()
                .map(|n| format!("{n:02x}"))
                .collect::<String>();
            receipt.nonce = nonce.clone();
            let mut request = eval::live::probe_request(&nonce, &batch.request.main_slot);
            let first = crate::usage::complete_project_request(
                &worker.ctx_for("probe", None),
                provider.as_ref(),
                &request,
                "evaluation_preflight",
            )
            .map_err(turn::TurnError::Provider)?;
            receipt
                .exchanges
                .push(eval::live::ProbeExchange::capture(&request, &first)?);
            receipt
                .observed_models
                .push(first.usage.observed_model.clone().unwrap_or_default());
            let calls: Vec<_> = first
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some((id, name, input))
                    } else {
                        None
                    }
                })
                .collect();
            if first.stop != StopReason::ToolUse
                || calls.len() != 1
                || calls[0].1 != "evaluation_probe"
                || calls[0].2 != &json!({"nonce":nonce})
                || first.content.iter().any(|b| {
                    !matches!(
                        b,
                        ContentBlock::ToolUse { .. }
                            | ContentBlock::Text { .. }
                            | ContentBlock::Thinking { .. }
                    )
                })
            {
                return Err(ApiError::BadInput("preflight_tool_contract_failed".into()));
            }
            let call_id = calls[0].0.clone();
            request.messages.push(Message {
                role: Role::Assistant,
                content: first.content,
            });
            request.messages.push(Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: call_id,
                    content: nonce.clone(),
                    is_error: false,
                    images: vec![],
                }],
            });
            request.tools.clear();
            let second = crate::usage::complete_project_request(
                &worker.ctx_for("probe", None),
                provider.as_ref(),
                &request,
                "evaluation_preflight",
            )
            .map_err(turn::TurnError::Provider)?;
            receipt
                .exchanges
                .push(eval::live::ProbeExchange::capture(&request, &second)?);
            receipt
                .observed_models
                .push(second.usage.observed_model.clone().unwrap_or_default());
            let text = second
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            if second.stop != StopReason::EndTurn
                || text.trim() != nonce
                || second.content.iter().any(|b| {
                    !matches!(b, ContentBlock::Text { .. } | ContentBlock::Thinking { .. })
                })
                || receipt.observed_models != vec![price.model.clone(), price.model.clone()]
            {
                return Err(ApiError::BadInput(
                    "preflight_model_or_tool_result_failed".into(),
                ));
            }
            receipt.tools_passed = true;
            Ok(())
        })();
        #[cfg(test)]
        eval::live::activity_crash("returned");
        let mut query=worker.db.conn().prepare("SELECT request_id FROM usage WHERE purpose='evaluation_preflight' AND record_kind='request' ORDER BY rowid")?;
        receipt.request_ids = query
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        eval::budget::live::finish_activity(&self.repo_root, &workspace, &receipt.id)?;
        receipt.state = if result.is_ok() { "passed" } else { "failed" }.into();
        receipt.reason = result
            .as_ref()
            .err()
            .map(|_| "preflight_not_verified".into());
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        eval::live::save(&self.db, &receipt)?;
        if result.is_ok() && receipt.request_ids.len() == 2 {
            let mut verification = batch.verification.clone();
            verification.model_request_id = Some(receipt.request_ids[0].clone());
            verification.tools_request_id = Some(receipt.request_ids[1].clone());
            verification.price_receipt = Some(receipt.id.clone());
            let mut check = batch.clone();
            check.verification = verification.clone();
            if eval::live::proof(&self.db, &check)? {
                self.db.conn().execute(
                    "UPDATE evaluation_batches SET verification_json=?2 WHERE id=?1",
                    rusqlite::params![batch.id, serde_json::to_string(&verification)?],
                )?;
            } else {
                receipt.state = "failed".into();
                receipt.reason = Some("preflight_receipt_incomplete".into());
                eval::live::save(&self.db, &receipt)?;
            }
        }
        tx.commit()?;
        crate::diag::note(
            if receipt.state == "passed" {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            receipt.state != "passed",
            Some(&self.project_id),
            None,
            None,
            None,
            "evaluation_preflight",
            if receipt.state == "passed" {
                "model_tools_price_verified"
            } else {
                "preflight_not_verified"
            },
            started,
        );
        Ok(receipt)
    }
}

impl Workbench {
    pub(super) fn attach_evaluation_resume_transport(
        &self,
        worker: &mut Workbench,
        run: &eval::EvaluationResult,
    ) -> Result<(), ApiError> {
        let plan_id:String=self.db.conn().query_row("SELECT p.id FROM evaluation_plans p JOIN evaluation_plan_runs r ON r.plan_id=p.id WHERE r.run_id=?1",[&run.id],|r|r.get(0))?;
        self.enable_evaluation_live(&plan_id)?;
        let plan = self.evaluation_plan(&plan_id)?;
        let batch = self.evaluation_batch(&plan.batch_id)?;
        let provider = self.frozen_evaluation_provider(&batch)?;
        worker.providers.clear();
        worker.register_provider(&batch.request.main_slot, provider);
        Ok(())
    }
}

fn preflight_refusal(project: &str, code: &str, started: std::time::Instant) -> ApiError {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(project),
        None,
        None,
        None,
        "evaluation_preflight",
        code,
        started,
    );
    ApiError::BadInput(code.into())
}
