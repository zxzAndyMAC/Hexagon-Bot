//! MR-08 (test3 12): project auxiliary generation — the opening analysis.
//! Its failure used to be a transient toast and a silent reset to pending,
//! retried only on the next project open. It is now the same model-request
//! item as dispatch/role failures. Retry re-runs only the analysis; a
//! finished analysis (status no longer pending) is never repeated.
use super::*;

const DEDUPE: &str = "opening_intake";

impl Workbench {
    /// Record a failed opening-analysis request and release its claim so the
    /// explicit retry can run it again. Replaces the bare abort.
    pub fn note_opening_intake_failure(
        &self,
        error: &crate::provider::ProviderError,
    ) -> Result<String, ApiError> {
        let speaker = self
            .intake_speaker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|(_, agent)| agent)
            .unwrap_or_default();
        self.abort_opening_intake()?;
        let open = self.open_intake_item()?;
        let agent = open
            .as_ref()
            .and_then(|card| card.agent_id.clone())
            .unwrap_or(speaker);
        let scope = RecoveryScope {
            purpose: "opening_intake",
            agent_id: &agent,
            run_id: None,
            dedupe: DEDUPE.into(),
            packet: json!({"root": self.repo_root.canonicalize()?, "agent_id": agent}),
            // One opening intake item at a time (fixed dedupe): its id is the task.
            task_key: None,
        };
        let entry = failure_entry(failure_code(error), Some(error), None, None, "intake");
        self.record_failed_attempt(
            &scope,
            open.as_ref().map(|card| card.id.as_str()),
            entry,
            &[],
        )
    }

    /// The analysis committed: close its item, if one was open.
    pub fn finish_opening_intake_recovery(&self) -> Result<(), ApiError> {
        if let Some(card) = self.open_intake_item()? {
            let agent = card.agent_id.clone().unwrap_or_default();
            self.finish_recovery(&card.id, &agent, None)?;
        }
        Ok(())
    }

    fn open_intake_item(&self) -> Result<Option<crate::cards::QueuedCard>, ApiError> {
        Ok(crate::cards::queued(&self.db, &self.project_id)?
            .into_iter()
            .find(|card| {
                card.payload["sub"] == "model_request"
                    && card.payload["purpose"] == "opening_intake"
            }))
    }

    /// Core retry path (the desktop shell re-runs its lock-free async command
    /// instead, D01). The status claim makes it single-flight.
    pub(super) fn retry_opening_intake(
        &self,
        card: &crate::cards::Card,
        started: std::time::Instant,
    ) -> Result<UnnamedRoute, ApiError> {
        if card.payload["packet"]["root"].as_str()
            != Some(self.repo_root.canonicalize()?.to_string_lossy().as_ref())
        {
            return Err(self.reject_recovery(card, "original_task_changed", started));
        }
        if self.intake_status()? != crate::intake::STATUS_PENDING {
            // Already analysed (or running elsewhere): nothing to repeat.
            self.finish_opening_intake_recovery()?;
            return Ok(UnnamedRoute::Skipped);
        }
        self.run_opening_intake()?;
        Ok(UnnamedRoute::Skipped)
    }
}
