//! Test3/17–23: act on one delegated decision through the card owner's
//! facade. Every branch re-validates its own target at execution; the
//! decider in scope is the grant actor (set by `managed_commit`).
use super::*;

impl Workbench {
    /// Act through the card owner's facade. Each branch re-validates its own
    /// target (action state, permission, activation chain) at execution.
    pub(super) fn managed_act(
        &mut self,
        candidate: &ManagedCandidate,
        decision: &ManagedDecision,
    ) -> Result<String, ApiError> {
        let card = crate::cards::get(&self.db, &candidate.card_id)?;
        let p = &card.payload;
        let reason = format!("managed: {}", decision.reason);
        let action_id = || {
            p["action_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| ApiError::BadInput("recovery card without an action".into()))
        };
        if decision.reason == "route_unavailable" {
            self.end_card_incomplete(&card)?;
            return Ok("incomplete".into());
        }
        let outcome = match (
            card.kind.as_str(),
            p["sub"].as_str(),
            decision.action.as_str(),
        ) {
            ("permission", _, _) => return self.managed_permission(candidate, decision),
            ("recovery", Some("tool_outcome_unknown"), "new_attempt") => {
                match self.retry_tool_action(&action_id()?, &reason, true)? {
                    crate::tools::CallOutcome::Asked(_) => "new_attempt_awaits_permission",
                    _ => "new_attempt_dispatched",
                }
            }
            ("recovery", Some("tool_outcome_unknown"), _) => {
                self.abandon_tool_action(&action_id()?, &reason)?;
                "abandoned"
            }
            ("recovery", Some("tool_action_ready"), "resume") => {
                self.resume_tool_action(&action_id()?)?;
                "resumed"
            }
            ("recovery", Some("tool_action_ready"), _) => {
                let id = action_id()?;
                let action = crate::actions::get(&self.db, &self.project_id, &id)?;
                let ctx = self.ctx_for(&action.agent_id, action.stage_run_id.clone());
                crate::actions::fail_authorized(&self.db, &ctx, &id, &reason)?;
                "ended_unexecuted"
            }
            ("recovery", Some("model_request"), "retry") => {
                self.retry_model_request(&card.id)?;
                "retried"
            }
            ("recovery", Some("model_request"), "follow_new") => {
                self.resolve_model_request_choice(&card.id, true)?;
                "followed_newer_instruction"
            }
            ("recovery", Some("model_request"), _) => {
                self.retire_model_request_incomplete(&card)?;
                "incomplete"
            }
            ("stamp", _, "adopt") if !p["proposal_id"].is_null() => {
                self.managed_adopt(&card.id)?;
                "adopted"
            }
            ("stamp", _, "reject") if !p["proposal_id"].is_null() => {
                let ctx = self.ctx_for("owner", None);
                crate::proposals::reject_at_stamp_by(
                    &self.db,
                    &ctx,
                    &card.id,
                    &reason,
                    &self.decider(),
                )?;
                "rejected"
            }
            ("stamp", _, _) if !p["proposal_id"].is_null() => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("stamp", None, action) => return self.managed_stage(&card, action, &reason),
            ("stamp", Some("quality_revalidation"), action) => {
                return self.managed_revalidation(&card, action)
            }
            ("stamp", Some("acceptance_exception"), _) => {
                orchestra::cancel_acceptance_exception_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    &self.decider(),
                )?;
                "exception_refused"
            }
            ("escalation", Some("design_direction"), "incomplete") => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("escalation", Some("design_direction"), option) => {
                let revision = p["revision"].as_i64().unwrap_or_default();
                crate::design::choose_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    revision,
                    Some(option),
                    None,
                    &self.decider(),
                )?;
                self.continue_design_direction(revision)?;
                "direction_chosen_and_continued"
            }
            ("escalation", Some("context_overflow"), action) => {
                self.adjudicate_flag(&card.id, action == "resume")?;
                if action == "resume" {
                    "resumed"
                } else {
                    "ended"
                }
            }
            ("escalation", _, "agree" | "reject") => {
                self.adjudicate_flag(&card.id, decision.action == "agree")?;
                if decision.action == "agree" {
                    "rework_requested"
                } else {
                    "flag_overruled"
                }
            }
            ("escalation", _, _) => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("publish", _, "publish") => {
                crate::publish::confirm_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    self.creds.as_ref(),
                    &self.decider(),
                )?;
                "published"
            }
            ("publish", _, _) => {
                crate::publish::reject_by(&self.db, &self.project_id, &card.id, &self.decider())?;
                "not_published"
            }
            // The confirmation rechecks its own facts again at execution.
            ("worktree", _, "confirm") => {
                self.confirm_session_worktree_by(&card.id, &self.decider())?;
                "worktree_confirmed"
            }
            ("worktree", _, _) => {
                self.reject_session_worktree_by(&card.id, &self.decider())?;
                "worktree_not_confirmed"
            }
            ("install", _, "install" | "reject") => {
                let installed = crate::install::resolve_install_by(
                    &self.db,
                    &self.project_id,
                    &self.repo_root,
                    &card.id,
                    decision.action == "install",
                    &self.decider(),
                )?;
                if installed.installed {
                    "installed"
                } else {
                    "install_rejected"
                }
            }
            ("grant", _, "allow" | "deny") => {
                let granted = crate::grants::confirm_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    decision.action == "allow",
                    &self.decider(),
                )?;
                if granted.granted {
                    "granted"
                } else {
                    "grant_denied"
                }
            }
            // Ticket 01: no grants row — direct use lasts only while
            // delegation is active (grants::managed_direct_use).
            ("grant", _, "direct_use") => {
                crate::cards::answer(&self.db, &card.id, &self.decider())?;
                "direct_use_without_grant"
            }
            ("install" | "grant", _, _) => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
            ("recovery", None, "resume_run") => {
                let run = p["run_id"].as_str().unwrap_or_default().to_string();
                self.recover_run(&run)?;
                "run_resumed"
            }
            ("stall", _, "continue" | "retry") => {
                self.stall_retry(&card.id)?;
                "continued"
            }
            ("stall", _, _) => {
                self.stall_ack(&card.id)?;
                "ended"
            }
            _ => {
                self.end_card_incomplete(&card)?;
                "incomplete"
            }
        };
        Ok(outcome.into())
    }

    fn managed_stage(
        &self,
        card: &crate::cards::Card,
        action: &str,
        reason: &str,
    ) -> Result<String, ApiError> {
        let by = self.decider();
        let pack = self.pack()?;
        let stage_outcome = |result: orchestra::StageAction| match result {
            orchestra::StageAction::Incomplete { .. } => "still_incomplete",
            _ => "accepted",
        };
        Ok(match action {
            "accept" => stage_outcome(orchestra::stamp_by(&self.db, &self.project_id, pack, &by)?),
            "run_checks" => {
                orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, pack)?;
                match stage_outcome(orchestra::stamp_by(&self.db, &self.project_id, pack, &by)?) {
                    "accepted" => "checks_ran_and_accepted",
                    _ => "checks_ran_still_incomplete",
                }
            }
            "rework" => {
                let evidence = orchestra::stage_evidence(&self.db, &self.project_id, pack)?;
                if card.payload["final_acceptance"] == true {
                    let stage = evidence
                        .as_ref()
                        .and_then(|e| {
                            e.checks
                                .iter()
                                .find(|c| c.state == orchestra::CheckState::Failed)
                                .map(|c| c.stage.clone())
                        })
                        .or_else(|| card.payload["stage"].as_str().map(str::to_string))
                        .unwrap_or_default();
                    let note = format!(
                        "{reason}; unmet: {}",
                        evidence.map(|e| e.missing.join(", ")).unwrap_or_default()
                    );
                    crate::review::reject_final_by(
                        &self.db,
                        &self.project_id,
                        pack,
                        &stage,
                        &note,
                        &by,
                    )?;
                } else {
                    crate::review::reject_stamp_by(&self.db, &self.project_id, pack, &by)?;
                }
                "rework_requested"
            }
            _ => {
                self.end_card_incomplete(card)?;
                "incomplete"
            }
        }
        .into())
    }

    fn managed_revalidation(
        &self,
        card: &crate::cards::Card,
        action: &str,
    ) -> Result<String, ApiError> {
        let by = self.decider();
        let pack = self.pack()?;
        let confirm = |db: &Db| -> Result<&'static str, ApiError> {
            let fingerprint = orchestra::stage_evidence(db, &self.project_id, pack)?
                .and_then(|e| e.fingerprint)
                .unwrap_or_default();
            Ok(
                match orchestra::confirm_quality_revalidation_by(
                    db,
                    &self.project_id,
                    pack,
                    &card.id,
                    &fingerprint,
                    &by,
                )? {
                    orchestra::StageAction::PackFinished => "revalidated",
                    _ => "still_incomplete",
                },
            )
        };
        Ok(match action {
            "confirm" => confirm(&self.db)?,
            "run_checks" => {
                orchestra::run_checks(&self.db, &self.project_id, &self.repo_root, pack)?;
                confirm(&self.db)?
            }
            _ => {
                orchestra::cancel_quality_revalidation_by(
                    &self.db,
                    &self.project_id,
                    &card.id,
                    &by,
                )?;
                "kept_checks_required"
            }
        }
        .into())
    }

    /// Same controlled adoption as the owner's, recorded under the grant.
    fn managed_adopt(&mut self, qid: &str) -> Result<(), ApiError> {
        crate::proposals::recover_policy(&self.db, &self.repo_root, &self.project_id)?;
        self.refresh_policy()?;
        let by = self.decider();
        crate::proposals::activate_by(&self.db, &self.ctx_for("owner", None), qid, &by)?;
        self.refresh_policy()?;
        Ok(())
    }

    /// The type's terminal "incomplete": the card is answered by the grant
    /// actor and marked, the target stays unaccepted. Visible in the panel.
    pub(super) fn end_card_incomplete(&self, card: &crate::cards::Card) -> Result<(), ApiError> {
        crate::cards::annotate(
            &self.db,
            &card.id,
            &[("managed_outcome", json!("incomplete"))],
        )?;
        crate::cards::answer(&self.db, &card.id, &self.decider())?;
        Ok(())
    }

    fn managed_permission(
        &self,
        candidate: &ManagedCandidate,
        decision: &ManagedDecision,
    ) -> Result<String, ApiError> {
        let allow = decision.action == "allow";
        // The saved-decision path executes the action in the background and
        // continues the original task (ticket 04); the actor is the grant.
        self.permission_control().save_by_managed(
            &self.db,
            &candidate.card_id,
            allow,
            &candidate.actor,
        )?;
        if allow {
            self.continue_saved_permission(&candidate.card_id)?;
            Ok("allowed_and_continued".into())
        } else {
            Ok("denied".into())
        }
    }
}
