//! Test3/13 (spec 06, Q11/Q12): manual handling of a tool action whose
//! outcome is unknown. The owner sees what was attempted, gets an explicit
//! check result (never an empty success), and — once the action is resolved —
//! an explicit entry to continue the original task through the validated
//! activation chain. Nothing here replays the original effect.
use super::*;
use serde::Serialize;

/// The conclusion of one owner-triggered check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ReconcileVerdict {
    /// Checked; the evidence still cannot decide.
    StillUnknown,
    /// A host receipt proves the effect happened.
    ProvenSucceeded,
    /// A host receipt proves it never ran.
    ProvenNotExecuted,
    /// The owner already resolved it (abandon / new attempt).
    AlreadyResolved,
    /// This tool has no read-only check; the owner decides directly.
    Unsupported,
}

/// D06: a structured result per check, so the card can show "checked again,
/// same evidence" instead of an identical silent refresh.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReconcileReport {
    pub action_id: String,
    pub verdict: ReconcileVerdict,
    /// Stable reason code (i18n key suffix), never provider or tool prose.
    pub reason: String,
    pub check_count: u32,
    #[ts(type = "number")]
    pub checked_at_ms: i64,
    /// An explicit "continue original task" entry was offered.
    pub continue_offered: bool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Facts that decide whether the original task may be offered again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ContinueFacts {
    pub settled: bool,
    pub attempt_pending: bool,
    pub paused: bool,
    pub activation_found: bool,
}

/// Cost model: a false refusal costs the owner one explicit instruction to
/// the role; a false offer could revive a superseded task or continue while a
/// new attempt is still pending/unknown. Fail closed.
pub(super) fn continue_refusal(facts: &ContinueFacts) -> Option<&'static str> {
    let checks = [
        (!facts.settled, "action_not_resolved"),
        (facts.attempt_pending, "new_attempt_unfinished"),
        (facts.paused, "paused"),
        (!facts.activation_found, "original_task_missing"),
    ];
    checks
        .into_iter()
        .find(|(failed, _)| *failed)
        .map(|(_, code)| code)
}

impl Workbench {
    /// Owner-triggered read-only check with an explicit result.
    pub fn reconcile_tool_action_report(
        &self,
        action_id: &str,
    ) -> Result<ReconcileReport, ApiError> {
        let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
        self.instance_role(&action.agent_id)?;
        let (verdict, reason) = self.reconcile_verdict(&action)?;
        let card = crate::cards::find_by_idem(
            &self.db,
            &action.agent_id,
            &format!("unknown:{action_id}"),
        )?;
        let check_count = card
            .as_ref()
            .and_then(|card| card.payload["reconcile_checks"].as_u64())
            .unwrap_or(0) as u32
            + 1;
        let proven = matches!(
            verdict,
            ReconcileVerdict::ProvenSucceeded | ReconcileVerdict::ProvenNotExecuted
        );
        let continue_offered = proven && self.offer_continue_after_unknown(action_id)?;
        let report = ReconcileReport {
            action_id: action_id.into(),
            verdict,
            reason: reason.into(),
            check_count,
            checked_at_ms: now_ms(),
            continue_offered,
        };
        if let Some(card) = card {
            crate::cards::annotate(
                &self.db,
                &card.id,
                &[
                    ("reconcile_checks", json!(check_count)),
                    ("last_reconcile", serde_json::to_value(&report)?),
                ],
            )?;
        }
        Ok(report)
    }

    fn reconcile_verdict(
        &self,
        action: &crate::actions::Action,
    ) -> Result<(ReconcileVerdict, &'static str), ApiError> {
        if crate::actions::has_resolution(&self.db, &action.id)? {
            return Ok((ReconcileVerdict::AlreadyResolved, "owner_resolved"));
        }
        match action.state.as_str() {
            "unknown" => {}
            "succeeded" => return Ok((ReconcileVerdict::ProvenSucceeded, "already_settled")),
            _ => return Ok((ReconcileVerdict::ProvenNotExecuted, "already_settled")),
        }
        let Some(tool) = self.registry.get(&action.tool) else {
            return Ok((ReconcileVerdict::Unsupported, "tool_unavailable"));
        };
        if !tool.supports_reconcile() {
            return Ok((ReconcileVerdict::Unsupported, "no_reconciliation_contract"));
        }
        let ctx = self.ctx_for(&action.agent_id, action.stage_run_id.clone());
        let allowed = matches!(
            crate::permissions::evaluate_logged(
                &self.db,
                &ctx,
                tool.as_ref(),
                &action.tool,
                &action.input
            )?,
            crate::permissions::Decision::Allow { .. }
        );
        if !allowed {
            // Revocation also forbids querying through the old grant.
            return Ok((ReconcileVerdict::StillUnknown, "permission_revoked"));
        }
        Ok(
            match self.registry.reconcile_action(&self.db, &ctx, &action.id) {
                Ok(_) => (ReconcileVerdict::ProvenSucceeded, "receipt_confirms_effect"),
                Err(crate::tools::ToolError::NotExecuted(_)) => (
                    ReconcileVerdict::ProvenNotExecuted,
                    "receipt_proves_not_executed",
                ),
                Err(crate::tools::ToolError::OutcomeUnknown(_)) => {
                    (ReconcileVerdict::StillUnknown, "evidence_inconclusive")
                }
                Err(error) => return Err(error.into()),
            },
        )
    }

    /// Offer the explicit "continue original task" entry once the unknown
    /// action no longer blocks it. Returns whether an entry now exists.
    pub fn offer_continue_after_unknown(&self, action_id: &str) -> Result<bool, ApiError> {
        let started = std::time::Instant::now();
        let action = crate::actions::get(&self.db, &self.project_id, action_id)?;
        let resolution = crate::actions::resolution(&self.db, action_id)?;
        let attempt_pending = match resolution
            .as_ref()
            .and_then(|(d, _, next)| (d == "new_attempt").then_some(next.clone()))
        {
            // A new attempt that itself ended unknown and was then resolved
            // (abandoned) no longer blocks the original task (test3/18).
            Some(Some(next)) => {
                !matches!(
                    crate::actions::get(&self.db, &self.project_id, &next)?
                        .state
                        .as_str(),
                    "succeeded" | "failed" | "denied"
                ) && !crate::actions::has_resolution(&self.db, &next)?
            }
            Some(None) => true,
            None => false,
        };
        let resume = self.action_activation(&action.agent_id, action_id)?;
        let facts = ContinueFacts {
            settled: resolution.is_some() || action.state != "unknown",
            attempt_pending,
            paused: orchestra::is_paused(&self.db, &self.project_id)?,
            activation_found: resume.is_some(),
        };
        let refusal = continue_refusal(&facts);
        crate::diag::note(
            if refusal.is_some() {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_JUDGE
            },
            refusal.is_some(),
            Some(&self.project_id),
            Some(&action.agent_id),
            action.stage_run_id.as_deref(),
            Some(action_id),
            "unknown_continue",
            refusal.unwrap_or("offered"),
            started,
        );
        let (None, Some(resume)) = (refusal, resume) else {
            return Ok(false);
        };
        // The existing activation-resume entry re-validates project, instance,
        // stage, pause and model when the owner actually continues.
        self.queue_activation_resume(
            &action.agent_id,
            action.stage_run_id.as_deref(),
            &resume,
            "unknown_action_resolved",
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // Test3/13: continuation is offered only for a resolved action with no
        // unfinished new attempt, an unpaused project and a found activation.
        #[test]
        fn continue_is_offered_only_when_every_fact_holds(bits in proptest::collection::vec(any::<bool>(), 4)) {
            let facts = ContinueFacts {
                settled: bits[0],
                attempt_pending: bits[1],
                paused: bits[2],
                activation_found: bits[3],
            };
            let offered = continue_refusal(&facts).is_none();
            prop_assert_eq!(offered, facts.settled && !facts.attempt_pending && !facts.paused && facts.activation_found);
        }
    }
}
