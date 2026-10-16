//! MR-03: role model-request recovery continues the failed call position.
//! It never restarts the activation from the instruction (that replayed a
//! successful write), never falls back to the history-less stage retrigger,
//! and never reconstructs a request from a display summary or fingerprint.
use super::*;

/// The smallest durable continuation envelope for a role model call. It keeps
/// the exact owner input and activation identity; a retry never turns into a
/// fresh owner message or a different agent instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::api) struct TurnRecoveryPacket {
    pub root: PathBuf,
    pub agent_id: String,
    pub role: String,
    pub run_id: Option<String>,
    pub origin_turn_id: i64,
    pub input: String,
    pub attachments: Vec<crate::trace::AttachRef>,
    pub plan_first: bool,
    #[serde(default)]
    pub partial_text: Option<String>,
}

/// Facts compared before a role retry is sent. Pure so proptest can pin the
/// admission order independently of database fixtures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TurnRecoveryFacts {
    pub already_recovered: bool,
    pub same_task: bool,
    /// The original stage run ended or another run is active: proven change.
    pub stage_changed: bool,
    /// A newer independent task, or a later step of this activation, exists
    /// for the role: the saved position is no longer the task's frontier.
    pub superseded: bool,
    pub paused: bool,
    pub agent_sleeping: bool,
    pub unknown_tool_pending: bool,
    pub checkpoint_present: bool,
    pub native_material: bool,
    pub chain_matches: bool,
}

/// MR-03 admission. Cost model: a false refusal costs one owner decision (the
/// card stays queued with its history); a false admit resends a superseded
/// task, revives a stopped team or continues past an unresolved tool effect.
/// The judge leans to refusal and the first failing reason wins in this order.
pub(super) fn turn_recovery_refusal(facts: &TurnRecoveryFacts) -> Option<&'static str> {
    let checks = [
        (facts.already_recovered, "already_recovered"),
        (facts.stage_changed, "stage_changed"),
        (!facts.same_task, "original_task_changed"),
        (facts.superseded, "superseded_by_new_task"),
        (facts.paused, "paused"),
        (facts.agent_sleeping, "agent_sleeping"),
        // Unknown tool outcomes keep their own reconcile/idempotency contract;
        // the model must not continue as if the effect were known.
        (facts.unknown_tool_pending, "tool_outcome_unknown"),
        (!facts.checkpoint_present, "checkpoint_missing"),
        // Screen pixels are never durable material: re-observe in a new turn.
        (!facts.native_material, "native_material_unavailable"),
        (!facts.chain_matches, "activation_changed"),
    ];
    checks
        .into_iter()
        .find(|(failed, _)| *failed)
        .map(|(_, code)| code)
}

/// Ticket 04 (spec T5): which task a role's model request serves. Inside a
/// stage the stage run is the task; outside one, the activation that woke the
/// role (its dispatch or mention) is, and every later turn of that activation
/// shares its root. None when the checkpoint is missing: the item then
/// budgets alone, which is the old behaviour, never a shared unbounded key.
pub(super) fn turn_task_key(run_id: Option<&str>, checkpoint: Option<&Value>) -> Option<String> {
    if let Some(run) = run_id {
        return Some(format!("run:{run}"));
    }
    let checkpoint = checkpoint?;
    checkpoint["activation_root_turn_id"]
        .as_i64()
        .or_else(|| checkpoint["turn_id"].as_i64())
        .map(|root| format!("activation:{root}"))
}

/// One role model request's item key: one item per origin step, whether the
/// first network wait or the final failure creates it.
pub(in crate::api) fn turn_request_key(origin_turn_id: i64) -> String {
    format!("turn_model:{origin_turn_id}")
}

/// The newest top-level TurnStarted of a role: the request's origin step.
pub(in crate::api) fn origin_turn_id(
    db: &Db,
    project_id: &str,
    agent_id: &str,
) -> Result<i64, ApiError> {
    Ok(db.conn().query_row(
        "SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='turn_started' AND COALESCE(json_extract(payload,'$.subagent'),0)=0",
        rusqlite::params![project_id, agent_id],
        |row| row.get(0),
    )?)
}

/// Managed-followups ticket 03 (spec T4): the role turn's model-request item
/// as the turn kernel sees it. On the first network wait it publishes the
/// item as `retrying` (same request key as a later failure, so one request
/// is one item) — that is what the owner's per-item stop acts on before any
/// failure has been recorded. A retry passes its existing item instead.
pub(in crate::api) struct TurnWaitItem {
    pub project_id: String,
    pub root: PathBuf,
    pub agent_id: String,
    pub role: String,
    pub run_id: Option<String>,
    pub input: String,
    pub attachments: Vec<crate::trace::AttachRef>,
    pub plan_first: bool,
    pub activation_root: Option<i64>,
    pub existing: Option<String>,
}

impl TurnWaitItem {
    fn publish(
        &self,
        db: &Db,
        retry_deadline_ms: i64,
        retry_window_ms: i64,
    ) -> Result<String, ApiError> {
        let origin = origin_turn_id(db, &self.project_id, &self.agent_id)?;
        let dedupe = turn_request_key(origin);
        if let Some(open) = crate::cards::queued_by_key(db, &self.project_id, &dedupe)? {
            return Ok(open);
        }
        let packet = TurnRecoveryPacket {
            root: self.root.clone(),
            agent_id: self.agent_id.clone(),
            role: self.role.clone(),
            run_id: self.run_id.clone(),
            origin_turn_id: origin,
            input: self.input.clone(),
            attachments: self.attachments.clone(),
            plan_first: self.plan_first,
            partial_text: None,
        };
        let task_key = turn_task_key(
            self.run_id.as_deref(),
            Some(&json!({"activation_root_turn_id": self.activation_root, "turn_id": origin})),
        );
        let qid = crate::cards::enqueue(
            db,
            &self.project_id,
            Some(&self.agent_id),
            crate::cards::CardKind::Recovery,
            json!({"sub":"model_request","purpose":"turn","packet":packet,"state":"retrying",
                "history":[],"retry_deadline_ms":retry_deadline_ms,
                "retry_window_ms":retry_window_ms,"task_key":task_key}),
            Some(&dedupe),
        )?;
        publish_view(
            db,
            &self.project_id,
            &qid,
            &self.agent_id,
            self.run_id.as_deref(),
            "model_request_retrying",
        )?;
        Ok(qid)
    }
}

impl crate::turn::RecoveryItem for TurnWaitItem {
    fn locate_or_publish(
        &self,
        db: &Db,
        retry_deadline_ms: i64,
        retry_window_ms: i64,
    ) -> Option<String> {
        if let Some(existing) = &self.existing {
            return Some(existing.clone());
        }
        // A missing item only removes the per-item stop for this wait; the
        // project pause and role sleep still stop it.
        self.publish(db, retry_deadline_ms, retry_window_ms)
            .map_err(|error| log::warn!("model request item not published: {error}"))
            .ok()
    }

    fn recovered(&self, db: &Db, item: &str) {
        // A retry's own item is closed by run_recovered_turn.
        if self.existing.is_some() {
            return;
        }
        let closed = (|| -> Result<(), ApiError> {
            crate::cards::annotate(
                db,
                item,
                &[
                    ("state", json!("recovered")),
                    ("retry_deadline_ms", Value::Null),
                ],
            )?;
            crate::cards::answer(db, item, "host")?;
            publish_view(
                db,
                &self.project_id,
                item,
                &self.agent_id,
                self.run_id.as_deref(),
                "model_request_recovered",
            )
        })();
        if let Err(error) = closed {
            log::warn!("model request item {item} not closed: {error}");
        }
    }
}

impl Workbench {
    /// Persist a terminal role model-request failure on its recovery item.
    /// The checkpoint is the one written by this attempt's TurnStarted scope;
    /// a later retry failure keeps the original packet and refreshes its
    /// unfinished text and continuation position.
    fn save_turn_failure(
        &self,
        packet: &TurnRecoveryPacket,
        recovery_id: Option<&str>,
        error: &crate::provider::ProviderError,
        slot: &str,
        model: Option<&str>,
    ) -> Result<String, ApiError> {
        let checkpoint = self.latest_turn_checkpoint(&packet.agent_id, packet.run_id.as_deref())?;
        let mut packet: TurnRecoveryPacket = match recovery_id {
            Some(qid) => {
                serde_json::from_value(crate::cards::get(&self.db, qid)?.payload["packet"].clone())?
            }
            None => packet.clone(),
        };
        packet.partial_text = checkpoint
            .as_ref()
            .and_then(|(_, payload)| payload["partial_text"].as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        let request_id = checkpoint
            .as_ref()
            .and_then(|(_, payload)| payload["request_id"].as_str())
            .map(str::to_string);
        let entry = failure_entry(failure_code(error), Some(error), request_id, model, slot);
        let scope = RecoveryScope {
            purpose: "turn",
            agent_id: &packet.agent_id,
            run_id: packet.run_id.as_deref(),
            dedupe: turn_request_key(packet.origin_turn_id),
            packet: serde_json::to_value(&packet)?,
            task_key: turn_task_key(
                packet.run_id.as_deref(),
                checkpoint.as_ref().map(|(_, payload)| payload),
            ),
        };
        let checkpoint_id = checkpoint.map(|(id, _)| id);
        self.record_failed_attempt(
            &scope,
            recovery_id,
            entry,
            &[
                ("packet", scope.packet.clone()),
                ("checkpoint_event_id", json!(checkpoint_id)),
            ],
        )
    }

    /// The checkpoint written after the agent's latest top-level TurnStarted.
    fn latest_turn_checkpoint(
        &self,
        agent_id: &str,
        run_id: Option<&str>,
    ) -> Result<Option<(i64, Value)>, ApiError> {
        use rusqlite::OptionalExtension;
        let row: Option<(i64, String)> = self
            .db
            .conn()
            .query_row(
                "SELECT id,payload FROM events WHERE project_id=?1 AND agent_id=?2
                 AND stage_run_id IS ?3 AND kind='system'
                 AND json_extract(payload,'$.kind')='model_request_checkpoint'
                 AND id>(SELECT COALESCE(MAX(id),0) FROM events WHERE project_id=?1
                   AND agent_id=?2 AND kind='turn_started'
                   AND COALESCE(json_extract(payload,'$.subagent'),0)=0)
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![self.project_id, agent_id, run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(id, payload)| {
            let payload: Value = serde_json::from_str(&payload)?;
            Ok((id, self.db.hydrate_payload(&self.project_id, payload)))
        })
        .transpose()
    }

    /// Owner stop (Interrupted) is not a failure and creates no item, but a
    /// running retry of an existing item becomes `stopped`: same item, honest
    /// state, and neither stall rescue nor autonomy may resend it.
    pub(in crate::api) fn note_turn_model_failure(
        &self,
        packet: &TurnRecoveryPacket,
        recovery_id: Option<&str>,
        error: &crate::provider::ProviderError,
        slot: &str,
        model: Option<&str>,
    ) -> Result<(), ApiError> {
        if matches!(error.cause(), crate::provider::ProviderError::Interrupted) {
            if let Some(qid) = recovery_id {
                self.stop_turn_item(qid, &packet.agent_id, packet.run_id.as_deref())?;
            }
            return Ok(());
        }
        self.save_turn_failure(packet, recovery_id, error, slot, model)?;
        Ok(())
    }

    /// A stopped or interrupted request: its item becomes `stopped`, honest
    /// and not revived by stall rescue, autonomy or delegation. Ticket 03: an
    /// item the first wait opened has no position yet; the kernel saved one
    /// when it stopped, so an explicit retry continues the same call.
    pub(in crate::api) fn stop_turn_item(
        &self,
        qid: &str,
        agent_id: &str,
        run_id: Option<&str>,
    ) -> Result<(), ApiError> {
        if crate::cards::get(&self.db, qid)?.payload["checkpoint_event_id"].is_null() {
            let checkpoint = self.latest_turn_checkpoint(agent_id, run_id)?;
            crate::cards::annotate(
                &self.db,
                qid,
                &[("checkpoint_event_id", json!(checkpoint.map(|(id, _)| id)))],
            )?;
        }
        self.publish_recovery_state(qid, agent_id, run_id, "stopped", None)
    }

    pub(super) fn retry_turn_model_request(
        &self,
        card: &crate::cards::Card,
        started: std::time::Instant,
    ) -> Result<UnnamedRoute, ApiError> {
        let reject = |code: &str| self.reject_recovery(card, code, started);
        let packet: TurnRecoveryPacket = serde_json::from_value(card.payload["packet"].clone())
            .map_err(|_| reject("packet_missing"))?;
        let checkpoint = self.recovery_checkpoint(card, &packet)?;
        let saved: Option<crate::turn::ModelContinuation> = checkpoint
            .as_ref()
            .and_then(|payload| serde_json::from_value(payload["checkpoint"].clone()).ok());
        let resume = checkpoint
            .as_ref()
            .and_then(|c| c["turn_id"].as_i64())
            .and_then(|turn_id| {
                self.activation_resume(&packet.agent_id, packet.run_id.as_deref(), Some(turn_id))
                    .ok()
            });
        let root = checkpoint
            .as_ref()
            .and_then(|c| c["activation_root_turn_id"].as_i64());
        let facts = TurnRecoveryFacts {
            already_recovered: card.payload["state"] == "recovered",
            same_task: self.turn_recovery_same_task(card, &packet)?,
            stage_changed: self.stage_changed(packet.run_id.as_deref())?,
            superseded: checkpoint
                .as_ref()
                .map(|c| self.turn_superseded(&packet.agent_id, c))
                .transpose()?
                .unwrap_or(false),
            paused: orchestra::is_paused(&self.db, &self.project_id)?,
            agent_sleeping: self.agent_sleeping(&packet.agent_id),
            unknown_tool_pending: self.unknown_tool_pending(&packet.agent_id)?,
            checkpoint_present: checkpoint.is_some(),
            native_material: saved.is_some(),
            chain_matches: resume.as_ref().is_some_and(|resume| {
                resume.instruction == packet.input && root == Some(resume.root_turn_id)
            }),
        };
        if let Some(code) = turn_recovery_refusal(&facts) {
            return Err(reject(code));
        }
        let (Some(saved), Some(mut resume)) = (saved, resume) else {
            return Err(reject("checkpoint_missing"));
        };
        self.turn_provider(&packet.agent_id)
            .map_err(|_| reject("provider_unavailable"))?;
        let _lock = self.lock_request(
            &format!("turn-{}", packet.origin_turn_id),
            &packet.agent_id,
            packet.run_id.as_deref(),
        )?;
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(&packet.agent_id),
            packet.run_id.as_deref(),
            Some(&card.id),
            "model_recovery",
            "continue_failed_call",
            started,
        );
        let plan_first = saved.purpose == "planning";
        resume.model_continuation = Some(saved);
        self.run_recovered_turn(&card.id, &packet, &resume, plan_first)
    }

    fn run_recovered_turn(
        &self,
        qid: &str,
        packet: &TurnRecoveryPacket,
        resume: &evaluation_resume::ActivationResume,
        plan_first: bool,
    ) -> Result<UnnamedRoute, ApiError> {
        let run_id = packet.run_id.as_deref();
        self.publish_recovery_state(qid, &packet.agent_id, run_id, "retrying", None)?;
        let outcome = self.run_turn_agent_with_history(
            &packet.agent_id,
            &packet.input,
            &packet.attachments,
            plan_first,
            &resume.history,
            Some(resume),
            false,
            Some(qid),
        );
        match outcome {
            Ok(TurnOutcome::Finished) | Ok(TurnOutcome::AwaitingPermission(_)) => {
                self.finish_recovery(qid, &packet.agent_id, run_id)?;
                Ok(UnnamedRoute::Dispatched {
                    role: packet.role.clone(),
                    via: "model_recovery".into(),
                })
            }
            // Ticket 03: the owner stopped this retry (or paused/slept the
            // team) — stopped, never "failed".
            Ok(TurnOutcome::Interrupted) => {
                self.stop_turn_item(qid, &packet.agent_id, run_id)?;
                Err(ApiError::Decision(
                    "model recovery stopped: Interrupted".into(),
                ))
            }
            Ok(other) => {
                self.publish_recovery_state(qid, &packet.agent_id, run_id, "failed", None)?;
                Err(ApiError::Decision(format!(
                    "model recovery stopped: {other:?}"
                )))
            }
            Err(error) => {
                // Provider failures already updated this item; anything else
                // (tool/IO) must not leave it looking like a running retry.
                if crate::cards::get(&self.db, qid)?.payload["state"] == "retrying" {
                    self.publish_recovery_state(qid, &packet.agent_id, run_id, "failed", None)?;
                }
                Err(error)
            }
        }
    }

    /// The exact checkpoint this item recorded; never "the latest one".
    fn recovery_checkpoint(
        &self,
        card: &crate::cards::Card,
        packet: &TurnRecoveryPacket,
    ) -> Result<Option<Value>, ApiError> {
        use rusqlite::OptionalExtension;
        let Some(id) = card.payload["checkpoint_event_id"].as_i64() else {
            return Ok(None);
        };
        let payload: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT payload FROM events WHERE id=?1 AND project_id=?2 AND agent_id=?3
                 AND stage_run_id IS ?4 AND kind='system'
                 AND json_extract(payload,'$.kind')='model_request_checkpoint'",
                rusqlite::params![id, self.project_id, packet.agent_id, packet.run_id],
                |row| row.get(0),
            )
            .optional()?;
        // A corrupt payload is missing material, not an error to surface raw.
        Ok(payload
            .and_then(|text| serde_json::from_str(&text).ok())
            .map(|payload| self.db.hydrate_payload(&self.project_id, payload)))
    }

    fn turn_recovery_same_task(
        &self,
        card: &crate::cards::Card,
        packet: &TurnRecoveryPacket,
    ) -> Result<bool, ApiError> {
        Ok(packet.root == self.repo_root.canonicalize()?
            && card.agent_id.as_deref() == Some(packet.agent_id.as_str())
            && self.instance_role(&packet.agent_id).ok().as_deref() == Some(packet.role.as_str())
            && self.active_run()?.map(|run| run.id) == packet.run_id)
    }

    /// MR-04: the original stage run is finished/rejected, or a different run
    /// is active. A crash-interrupted run with nothing newer is not a change.
    fn stage_changed(&self, run_id: Option<&str>) -> Result<bool, ApiError> {
        let Some(run_id) = run_id else {
            return Ok(false);
        };
        let state: Option<String> =
            rusqlite::OptionalExtension::optional(self.db.conn().query_row(
                "SELECT state FROM stage_runs WHERE id=?1 AND project_id=?2",
                rusqlite::params![run_id, self.project_id],
                |row| row.get(0),
            ))?;
        let Some(state) = state else {
            return Ok(false);
        };
        let other_active = self.active_run()?.is_some_and(|run| run.id != run_id);
        Ok(matches!(state.as_str(), "done" | "rejected") || other_active)
    }

    /// MR-04: a later independent dispatch (or a later step that continued
    /// the same activation elsewhere, e.g. the crash-resume entry) means this
    /// saved position must never be sent again.
    fn turn_superseded(&self, agent_id: &str, checkpoint: &Value) -> Result<bool, ApiError> {
        let (Some(turn), Some(root)) = (
            checkpoint["turn_id"].as_i64(),
            checkpoint["activation_root_turn_id"].as_i64(),
        ) else {
            return Ok(false);
        };
        if crate::actions::activation_superseded(&self.db, &self.project_id, agent_id, root)? {
            return Ok(true);
        }
        Ok(self.db.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND agent_id=?2
             AND kind='turn_started' AND id>?3
             AND COALESCE(json_extract(payload,'$.subagent'),0)=0)",
            rusqlite::params![self.project_id, agent_id, turn],
            |row| row.get(0),
        )?)
    }

    fn agent_sleeping(&self, agent_id: &str) -> bool {
        // Unreadable status counts as sleeping: same fail-closed model as the
        // turn kernel's halt check (a false stop costs one manual retry).
        self.db
            .conn()
            .query_row(
                "SELECT status='sleeping' FROM agents WHERE project_id=?1 AND id=?2",
                rusqlite::params![self.project_id, agent_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(true)
    }

    fn unknown_tool_pending(&self, agent_id: &str) -> Result<bool, ApiError> {
        Ok(crate::cards::queued(&self.db, &self.project_id)?
            .iter()
            .any(|card| {
                card.kind == "recovery"
                    && card.agent_id.as_deref() == Some(agent_id)
                    && card.payload["sub"] == "tool_outcome_unknown"
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn admitted() -> TurnRecoveryFacts {
        TurnRecoveryFacts {
            same_task: true,
            checkpoint_present: true,
            native_material: true,
            chain_matches: true,
            ..Default::default()
        }
    }

    #[test]
    fn complete_valid_facts_are_admitted() {
        assert_eq!(turn_recovery_refusal(&admitted()), None);
    }

    proptest! {
        // MR-03: any single invalid fact refuses; admission requires all of
        // them. A refused retry never sends, so refusal is always safe.
        #[test]
        fn any_invalid_fact_refuses(bits in proptest::collection::vec(any::<bool>(), 10)) {
            let facts = TurnRecoveryFacts {
                already_recovered: bits[0],
                same_task: bits[1],
                paused: bits[2],
                agent_sleeping: bits[3],
                unknown_tool_pending: bits[4],
                checkpoint_present: bits[5],
                native_material: bits[6],
                chain_matches: bits[7],
                superseded: bits[8],
                stage_changed: bits[9],
            };
            prop_assert_eq!(turn_recovery_refusal(&facts).is_none(), facts == admitted());
            if facts.unknown_tool_pending && !facts.already_recovered && facts.same_task
                && !facts.stage_changed && !facts.superseded && !facts.paused && !facts.agent_sleeping {
                prop_assert_eq!(turn_recovery_refusal(&facts), Some("tool_outcome_unknown"));
            }
        }
    }
}
