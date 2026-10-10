//! MR-04 (test3 08): reopen and supersession of model-request items.
//!
//! Manual mode only (ADR 0083): reopening restores facts and entries but never
//! sends; a definitive cancel/retarget/stage change retires the old entry with
//! its reason; an owner instruction whose relation to a failed role request is
//! unknown waits for the owner's explicit choice. Managed-mode continuation
//! (tickets 17/23) reuses these identity checks but decides on its own.
use super::*;

/// Refusals that prove the original task no longer exists. The item leaves
/// the pending area with its reason. Every other refusal is transient: the
/// item stays queued and shows why it could not continue right now.
/// Cost model: wrongly retiring an item loses an owner entry (they must
/// re-instruct); wrongly keeping a dead one can resend a superseded task. Only
/// codes that prove replacement are definitive. Inconsistent evidence
/// (`original_task_changed`) or a crash-interrupted stage is not proof: an
/// early draft retired those too and lost recoverable PM items after reopen.
pub(super) fn refusal_is_definitive(code: &str) -> bool {
    matches!(code, "stage_changed" | "superseded_by_new_task")
}

/// Item states that still own the original request and may meet a choice.
fn open_for_choice(state: &str) -> bool {
    matches!(state, "failed" | "stopped" | "unknown" | "needs_choice")
}

impl Workbench {
    /// Persist why a retry was refused (see [`refusal_is_definitive`]).
    pub(super) fn note_refusal(
        &self,
        card: &crate::cards::Card,
        code: &str,
    ) -> Result<(), ApiError> {
        if code == "already_running" || card.payload["sub"] != "model_request" {
            return Ok(());
        }
        let run_id = card.payload["view"]["stage_run_id"]
            .as_str()
            .map(str::to_string);
        let agent = card.agent_id.clone().unwrap_or_default();
        if refusal_is_definitive(code) {
            return self.retire_recovery(&card.id, &agent, run_id.as_deref(), code);
        }
        crate::cards::annotate(&self.db, &card.id, &[("last_refusal", json!(code))])?;
        let view = model_request_recovery(&self.db, &self.project_id, &card.id)?;
        crate::cards::annotate(&self.db, &card.id, &[("view", serde_json::to_value(view)?)])?;
        Ok(())
    }

    /// The original task was replaced: close the entry, keep its history and
    /// show the reason on the timeline. It can never be sent again.
    pub(super) fn retire_recovery(
        &self,
        qid: &str,
        agent_id: &str,
        run_id: Option<&str>,
        reason: &str,
    ) -> Result<(), ApiError> {
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        crate::cards::annotate(
            &self.db,
            qid,
            &[
                ("state", json!("superseded")),
                ("superseded_reason", json!(reason)),
                ("retry_deadline_ms", Value::Null),
            ],
        )?;
        crate::cards::answer(&self.db, qid, "host")?;
        self.publish_recovery_view(qid, agent_id, run_id, "model_request_superseded")?;
        tx.commit()?;
        Ok(())
    }

    /// An owner instruction is about to reach `agent_id` while that role's
    /// failed request is still open. The relation is unknown, so neither task
    /// runs: the item records the new instruction and asks the owner.
    /// Returns the item id when the dispatch was held.
    pub(in crate::api) fn hold_for_recovery_choice(
        &self,
        agent_id: &str,
        input: &str,
        attachments: &[crate::trace::AttachRef],
    ) -> Result<Option<String>, ApiError> {
        let Some(message_id) = *self.owner_message.lock().unwrap_or_else(|e| e.into_inner()) else {
            return Ok(None);
        };
        let open = crate::cards::queued(&self.db, &self.project_id)?
            .into_iter()
            .find(|card| {
                card.agent_id.as_deref() == Some(agent_id)
                    && card.payload["sub"] == "model_request"
                    && card.payload["purpose"] == "turn"
                    && open_for_choice(card.payload["state"].as_str().unwrap_or("failed"))
            });
        let Some(card) = open else {
            return Ok(None);
        };
        let run_id = card.payload["packet"]["run_id"]
            .as_str()
            .map(str::to_string);
        crate::cards::annotate(
            &self.db,
            &card.id,
            &[
                ("state", json!("needs_choice")),
                (
                    "conflict",
                    json!({"message_id":message_id,"input":input,"attachments":attachments}),
                ),
            ],
        )?;
        self.publish_recovery_view(
            &card.id,
            agent_id,
            run_id.as_deref(),
            "model_request_failed",
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(agent_id),
            run_id.as_deref(),
            Some(&card.id),
            "model_recovery",
            "owner_choice_required",
            std::time::Instant::now(),
        );
        Ok(Some(card.id))
    }

    /// The owner's explicit answer to [`Self::hold_for_recovery_choice`].
    /// `follow_new` retires the old entry, then runs the new instruction;
    /// otherwise the held instruction stays in chat unexecuted and the failed
    /// request continues through the normal validated retry.
    pub fn resolve_model_request_choice(
        &self,
        qid: &str,
        follow_new: bool,
    ) -> Result<UnnamedRoute, ApiError> {
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Recovery)?;
        if card.project_id != self.project_id
            || card.payload["sub"] != "model_request"
            || card.payload["state"] != "needs_choice"
        {
            return Err(ApiError::BadInput(
                "model recovery choice not pending".into(),
            ));
        }
        let agent = card.agent_id.clone().unwrap_or_default();
        let run_id = card.payload["packet"]["run_id"]
            .as_str()
            .map(str::to_string);
        let conflict = card.payload["conflict"].clone();
        if follow_new {
            let input = conflict["input"]
                .as_str()
                .ok_or_else(|| ApiError::BadInput("held instruction missing".into()))?
                .to_string();
            let attachments: Vec<crate::trace::AttachRef> =
                serde_json::from_value(conflict["attachments"].clone()).unwrap_or_default();
            self.retire_recovery(qid, &agent, run_id.as_deref(), "owner_chose_new")?;
            let role = self.instance_role(&agent)?;
            self.dispatch_instance(&agent, &input, &attachments)?;
            return Ok(UnnamedRoute::Dispatched {
                role,
                via: "owner_choice".into(),
            });
        }
        crate::cards::annotate(
            &self.db,
            qid,
            &[("state", json!("failed")), ("conflict", Value::Null)],
        )?;
        self.publish_recovery_view(qid, &agent, run_id.as_deref(), "model_request_failed")?;
        self.retry_model_request(qid)
    }

    /// Reopen: an attempt that was in flight when the process ended has an
    /// unknown result. It must not stay `retrying` (no entry), and must not be
    /// shown as failed-before-send; its ledger row keeps the unknown cost.
    pub(in crate::api) fn reconcile_model_requests_after_reopen(&self) -> Result<(), ApiError> {
        for card in crate::cards::queued(&self.db, &self.project_id)? {
            if card.payload["sub"] != "model_request" || card.payload["state"] != "retrying" {
                continue;
            }
            let agent = card.agent_id.clone().unwrap_or_default();
            let run_id = card.payload["packet"]["run_id"]
                .as_str()
                .map(str::to_string);
            crate::cards::annotate(
                &self.db,
                &card.id,
                &[
                    ("state", json!("unknown")),
                    ("retry_deadline_ms", Value::Null),
                ],
            )?;
            self.publish_recovery_view(
                &card.id,
                &agent,
                run_id.as_deref(),
                "model_request_failed",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // MR-04: only proof of replacement retires an entry; every transient
        // refusal (pause, sleep, unknown tool, missing material, lock) keeps it.
        #[test]
        fn only_replacement_is_definitive(code in "[a-z_]{1,32}") {
            let definitive = refusal_is_definitive(&code);
            prop_assert_eq!(
                definitive,
                code == "stage_changed" || code == "superseded_by_new_task"
            );
        }
    }

    #[test]
    fn transient_refusals_keep_the_entry() {
        for code in [
            "original_task_changed",
            "paused",
            "agent_sleeping",
            "tool_outcome_unknown",
            "checkpoint_missing",
            "native_material_unavailable",
            "activation_changed",
            "provider_unavailable",
            "team_stopped",
        ] {
            assert!(!refusal_is_definitive(code), "{code}");
        }
    }
}
