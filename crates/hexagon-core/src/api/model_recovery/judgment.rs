//! MR-06 (test3 10): internal judge request failures. The fail-closed owner
//! card already exists (permission ask or proposal stamp); this module only
//! re-runs the same judge on the same target after re-checking its evidence.
//! A connection failure never becomes allow/pass, and a re-judge grants
//! nothing a successful first call could not have granted.
use super::*;

/// What a re-judge did. Returned over IPC (D06: a serde type, not a string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum RejudgeOutcome {
    /// Action review allowed it; the saved action and original task continue.
    Allowed,
    /// The judge still says "owner decides" (or failed again); card stays.
    Held,
    /// Execute judgment wrote the proposal.
    Executed,
    /// Execute judgment rejected the proposal.
    Rejected,
    /// Execute judgment handed it to the owner on a fresh card.
    Handed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RejudgeFacts {
    pub queued: bool,
    pub has_failure: bool,
    pub action_review: bool,
    pub execute_judgment: bool,
    pub safety_net: bool,
    pub reviewer_live: bool,
}

/// Cost model: a false refusal costs one owner decision on the card that is
/// still there; a false admit could let a machine verdict bypass the safety
/// net or a non-live reviewer. Fail closed; first failing reason wins.
pub(super) fn rejudge_refusal(facts: &RejudgeFacts) -> Option<&'static str> {
    let checks = [
        (!facts.queued, "not_pending"),
        (!facts.has_failure, "no_model_failure"),
        (
            !facts.action_review && !facts.execute_judgment,
            "purpose_unsupported",
        ),
        (facts.action_review && facts.safety_net, "safety_net"),
        (
            facts.action_review && !facts.reviewer_live,
            "reviewer_not_live",
        ),
    ];
    checks
        .into_iter()
        .find(|(failed, _)| *failed)
        .map(|(_, code)| code)
}

impl Workbench {
    /// Re-run the failed internal judge for this owner card (explicit owner
    /// action; the approve shortcut keeps its own meaning on the card).
    pub fn rejudge_after_model_failure(&self, qid: &str) -> Result<RejudgeOutcome, ApiError> {
        let started = std::time::Instant::now();
        let card = crate::cards::get(&self.db, qid)?;
        let purpose = card.payload["model_failure"]["purpose"]
            .as_str()
            .unwrap_or_default();
        let live = crate::autonomy::reviewer_mode(&self.db, &self.project_id)
            .is_ok_and(|mode| mode == "live")
            && crate::autonomy::execution_rank(&self.db, &self.project_id).unwrap_or(0) >= 1;
        let facts = RejudgeFacts {
            queued: card.project_id == self.project_id
                && card.state == crate::cards::CardState::Queued,
            has_failure: card.payload["model_failure"].is_object(),
            action_review: purpose == "action_review" && card.kind == "permission",
            execute_judgment: purpose == "execute_judgment" && card.kind == "stamp",
            safety_net: card.payload["safety_net"].as_bool() == Some(true),
            reviewer_live: live,
        };
        let refusal = rejudge_refusal(&facts);
        crate::diag::note(
            if refusal.is_some() {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_JUDGE
            },
            refusal.is_some(),
            Some(&self.project_id),
            card.agent_id.as_deref(),
            None,
            Some(qid),
            "judge_rejudge",
            refusal.unwrap_or(purpose),
            started,
        );
        if let Some(code) = refusal {
            return Err(ApiError::BadInput(format!("cannot re-judge: {code}")));
        }
        let agent = card.agent_id.clone().unwrap_or_default();
        if facts.action_review {
            return self.rejudge_action_review(qid, &agent, &card.payload);
        }
        let ctx = self.ctx_for(&agent, None);
        let jev = crate::provider_config::resolve_exact(
            &self.providers,
            crate::provider_config::JEV_SLOT,
        )
        .filter(|p| p.uses_decision_api())
        .map(|p| p.as_ref());
        Ok(
            match crate::proposals::rejudge_after_model_failure(&self.db, &ctx, qid, jev)? {
                crate::execute::Effect::Executed => RejudgeOutcome::Executed,
                crate::execute::Effect::Rejected => RejudgeOutcome::Rejected,
                crate::execute::Effect::Handed { .. } => RejudgeOutcome::Handed,
            },
        )
    }

    fn rejudge_action_review(
        &self,
        qid: &str,
        agent: &str,
        payload: &Value,
    ) -> Result<RejudgeOutcome, ApiError> {
        let input = payload["review_input"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let ctx = self.ctx_for(agent, self.active_run()?.map(|run| run.id));
        let (slot, provider) = self.turn_provider(agent)?;
        let slot = slot.unwrap_or_else(|| "default".into());
        let verdict =
            crate::reviewer::adjudicate(&self.db, &ctx, provider.as_ref(), &slot, qid, &input);
        if !matches!(verdict, crate::reviewer::ReviewOutcome::Allow) {
            return Ok(RejudgeOutcome::Held);
        }
        // Same contract as the in-turn live allow: a machine verdict, never
        // remembered as a shape, executed through the saved-decision path so
        // the original task continues exactly once.
        self.permission_control().save_by_reviewer(&self.db, qid)?;
        self.continue_saved_permission(qid)?;
        Ok(RejudgeOutcome::Allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // MR-06: a re-judge never reaches the safety net or a non-live
        // reviewer, and needs a pending card with a recorded request failure.
        #[test]
        fn rejudge_admission_is_fail_closed(bits in proptest::collection::vec(any::<bool>(), 6)) {
            let facts = RejudgeFacts {
                queued: bits[0],
                has_failure: bits[1],
                action_review: bits[2],
                execute_judgment: bits[3] && !bits[2],
                safety_net: bits[4],
                reviewer_live: bits[5],
            };
            let admitted = rejudge_refusal(&facts).is_none();
            if admitted {
                prop_assert!(facts.queued && facts.has_failure);
                prop_assert!(facts.action_review || facts.execute_judgment);
                if facts.action_review {
                    prop_assert!(!facts.safety_net && facts.reviewer_live);
                }
            }
        }
    }
}
