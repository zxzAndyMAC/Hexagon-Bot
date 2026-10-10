//! MR-01 / ADR 0083: recover the saved closed choice, never reroute the owner
//! message through stage kickoff or replay a completed worker turn.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct ChoiceOrigin {
    pub from_owner: bool,
    pub source_message: Option<i64>,
    pub kickoff_lead: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChoicePacket {
    pub root: PathBuf,
    pub pm_id: String,
    pub run_id: Option<String>,
    pub speaker: String,
    pub body: String,
    pub attachments: Vec<crate::trace::AttachRef>,
    pub roster: Vec<String>,
    pub instances: Option<Vec<(String, String)>>,
    pub request: crate::provider::ChatRequest,
    pub stage_name: Option<String>,
    pub activation: Vec<String>,
    pub choosable: Vec<String>,
    pub use_jev: bool,
    pub slot_name: String,
    pub via: String,
    pub origin_trace_id: i64,
    pub origin: ChoiceOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ModelRequestFailure {
    pub request_id: Option<String>,
    pub code: String,
    pub model: Option<String>,
    pub model_slot: String,
    pub http_status: Option<u16>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ModelRequestRecovery {
    pub recovery_id: String,
    pub project_root: String,
    pub purpose: String,
    pub agent_id: String,
    pub stage_run_id: Option<String>,
    pub target_role: Option<String>,
    pub state: String,
    pub history: Vec<ModelRequestFailure>,
    pub can_retry: bool,
}

pub(super) fn retry_budget_allows(
    elapsed: std::time::Duration,
    budget: std::time::Duration,
) -> bool {
    elapsed < budget
}

pub fn model_request_recovery(
    db: &Db,
    project_id: &str,
    qid: &str,
) -> Result<ModelRequestRecovery, ApiError> {
    let card = crate::cards::get(db, qid)?;
    if card.project_id != project_id
        || card.kind != "recovery"
        || card.payload["sub"] != "model_request"
        || card.payload["purpose"] != "pm_route"
    {
        return Err(ApiError::BadInput("model recovery scope invalid".into()));
    }
    let packet: ChoicePacket = serde_json::from_value(card.payload["packet"].clone())?;
    Ok(ModelRequestRecovery {
        recovery_id: card.id,
        project_root: packet.root.to_string_lossy().into_owned(),
        purpose: "pm_route".into(),
        agent_id: packet.pm_id,
        stage_run_id: packet.run_id,
        target_role: packet
            .origin
            .kickoff_lead
            .or_else(|| packet.activation.first().cloned()),
        state: card.payload["state"].as_str().unwrap_or("failed").into(),
        history: serde_json::from_value(card.payload["history"].clone())?,
        can_retry: card.state == crate::cards::CardState::Queued,
    })
}

impl Workbench {
    pub(super) fn execute_saved_pm_choice(
        &self,
        packet: &ChoicePacket,
        provider: Arc<dyn ModelProvider>,
        recovery_id: Option<&str>,
    ) -> Result<UnnamedRoute, ApiError> {
        // MR-02: a budget includes admission, every send and every wait. A
        // manual retry always gets a fresh owner budget, even for background work.
        let started = std::time::Instant::now();
        let budget = self.wait_policy.budget.min(std::time::Duration::from_secs(
            if recovery_id.is_some() || packet.origin.from_owner {
                60
            } else {
                300
            },
        ));
        let deadline = started + budget;
        let lock = Arc::new(self.lock_pm_choice_with_diag(packet)?);
        let mut recovery = recovery_id.map(str::to_string);
        loop {
            let (request_id, result) =
                self.pm_request_attempt(packet, provider.clone(), deadline, lock.clone());
            match result {
                Ok(response) => {
                    // A late result is accounting evidence, never permission to
                    // revive a stopped/superseded task.
                    if self.pm_choice_stopped(packet)? {
                        let qid = self.save_pm_failure(
                            packet,
                            recovery.as_deref(),
                            request_id,
                            &crate::provider::ProviderError::Interrupted,
                            provider.billing_model(),
                        )?;
                        self.publish_pm_wait_state(&qid, packet, "stopped")?;
                        return Err(ApiError::Decision("interrupted".into()));
                    }
                    return self.deliver_pm_choice(
                        packet,
                        crate::pm_route::choice_text(&response),
                        recovery.as_deref(),
                        None,
                    );
                }
                Err(error) => {
                    let qid = self.save_pm_failure(
                        packet,
                        recovery.as_deref(),
                        request_id,
                        &error,
                        provider.billing_model(),
                    )?;
                    recovery = Some(qid.clone());
                    if !error.retryable() || !retry_budget_allows(started.elapsed(), budget) {
                        if matches!(error.cause(), crate::provider::ProviderError::Interrupted) {
                            self.publish_pm_wait_state(&qid, packet, "stopped")?;
                        }
                        return Err(ApiError::Decision(error.to_string()));
                    }
                    self.publish_pm_wait_state(&qid, packet, "retrying")?;
                    let next =
                        (std::time::Instant::now() + self.wait_policy.probe_interval).min(deadline);
                    while std::time::Instant::now() < next {
                        if self.pm_choice_stopped(packet)? {
                            self.publish_pm_wait_state(&qid, packet, "stopped")?;
                            return Err(ApiError::Decision("interrupted".into()));
                        }
                        std::thread::sleep(
                            self.wait_policy
                                .tick
                                .max(std::time::Duration::from_millis(1))
                                .min(next.saturating_duration_since(std::time::Instant::now())),
                        );
                    }
                    if !retry_budget_allows(started.elapsed(), budget) {
                        self.publish_pm_wait_state(&qid, packet, "failed")?;
                        return Err(ApiError::Decision(error.to_string()));
                    }
                }
            }
        }
    }

    fn lock_pm_choice(&self, packet: &ChoicePacket) -> Result<std::fs::File, ApiError> {
        // MR-02: automatic and manual recovery lock the original request, not
        // just its later card. Detached accounting retains this lock until IO ends.
        let dir = self.repo_root.join(".hexagon/request-locks");
        let path = dir.join(format!("pm-route-{}.lock", packet.origin_trace_id));
        if dir
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
            || path
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(ApiError::BadInput("lock_path_invalid".into()));
        }
        std::fs::create_dir_all(dir)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        lock.try_lock()
            .map_err(|_| ApiError::BadInput("already_running".into()))?;
        Ok(lock)
    }

    fn lock_pm_choice_with_diag(&self, packet: &ChoicePacket) -> Result<std::fs::File, ApiError> {
        let started = std::time::Instant::now();
        match self.lock_pm_choice(packet) {
            Ok(lock) => Ok(lock),
            Err(error) => {
                let code = if error.to_string().contains("already_running") {
                    "already_running"
                } else if error.to_string().contains("lock_path_invalid") {
                    "lock_path_invalid"
                } else {
                    "lock_unavailable"
                };
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&self.project_id),
                    Some(&packet.pm_id),
                    packet.run_id.as_deref(),
                    Some(&packet.origin_trace_id.to_string()),
                    "model_recovery",
                    code,
                    started,
                );
                Err(error)
            }
        }
    }

    fn pm_request_attempt(
        &self,
        packet: &ChoicePacket,
        provider: Arc<dyn ModelProvider>,
        deadline: std::time::Instant,
        lock: Arc<std::fs::File>,
    ) -> (
        Option<String>,
        Result<crate::provider::ChatResponse, crate::provider::ProviderError>,
    ) {
        use crate::provider::ProviderError;
        let ctx = crate::tools::ToolContext {
            project_id: self.project_id.clone(),
            agent_id: packet.pm_id.clone(),
            repo_root: self.repo_root.clone(),
            stage_run_id: packet.run_id.clone(),
            ..Default::default()
        };
        // Existing synthetic in-memory seams stay inline. Real workbenches
        // settle on a separate initialized DB connection, including late usage.
        let Some(path) = self.db.path() else {
            let mut id = None;
            let result = crate::usage::request_with_id(
                &self.db,
                &ctx,
                &packet.slot_name,
                "pm_route",
                provider.as_ref(),
                (!packet.use_jev).then_some(&packet.request),
                |request| {
                    id = Some(request.to_string());
                    if self.pm_choice_stopped(packet).unwrap_or(true) {
                        return Err(ProviderError::Interrupted);
                    }
                    send_pm_choice(provider.as_ref(), packet, deadline)
                },
            );
            return (id, result);
        };
        let (sent, received) = std::sync::mpsc::channel();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let halt = cancelled.clone();
        let saved = packet.clone();
        std::thread::spawn(move || {
            let _lock = lock;
            let result = Db::open_current(path)
                .map_err(|_| ProviderError::Refused("request ledger unavailable".into()))
                .and_then(|db| {
                    crate::usage::request_with_id(
                        &db,
                        &ctx,
                        &saved.slot_name,
                        "pm_route",
                        provider.as_ref(),
                        (!saved.use_jev).then_some(&saved.request),
                        |request| {
                            let _ = sent.send(PmAttemptEvent::Admitted(request.to_string()));
                            if halt.load(Ordering::SeqCst)
                                || pm_choice_stopped_at(&db, &ctx.project_id, &saved)
                                    .unwrap_or(true)
                            {
                                return Err(ProviderError::Interrupted);
                            }
                            send_pm_choice(provider.as_ref(), &saved, deadline)
                        },
                    )
                });
            let _ = sent.send(PmAttemptEvent::Finished(result));
        });
        let mut id = None;
        loop {
            let failure = if self.pm_choice_stopped(packet).unwrap_or(true) {
                Some(ProviderError::Interrupted)
            } else if std::time::Instant::now() >= deadline {
                Some(ProviderError::Deadline)
            } else {
                None
            };
            if let Some(error) = failure {
                cancelled.store(true, Ordering::SeqCst);
                return (id, Err(error));
            }
            let slice = self
                .wait_policy
                .tick
                .max(std::time::Duration::from_millis(1))
                .min(deadline.saturating_duration_since(std::time::Instant::now()));
            match received.recv_timeout(slice) {
                Ok(PmAttemptEvent::Admitted(request)) => id = Some(request),
                Ok(PmAttemptEvent::Finished(result)) => return (id, result),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => (),
                Err(_) => {
                    return (
                        id,
                        Err(ProviderError::Refused("request worker exited".into())),
                    )
                }
            }
        }
    }

    pub(super) fn publish_pm_wait_state(
        &self,
        qid: &str,
        packet: &ChoicePacket,
        state: &str,
    ) -> Result<(), ApiError> {
        crate::cards::annotate(&self.db, qid, &[("state", json!(state))])?;
        let view = model_request_recovery(&self.db, &self.project_id, qid)?;
        crate::cards::annotate(&self.db, qid, &[("view", serde_json::to_value(&view)?)])?;
        let mut event = serde_json::to_value(view)?;
        event["kind"] = json!(if state == "retrying" {
            "model_request_retrying"
        } else {
            "model_request_failed"
        });
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            event,
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
        )?;
        Ok(())
    }

    fn deliver_pm_choice(
        &self,
        packet: &ChoicePacket,
        raw: String,
        recovery_id: Option<&str>,
        prior_event: Option<i64>,
    ) -> Result<UnnamedRoute, ApiError> {
        let pm_id = &packet.pm_id;
        let run_id = packet.run_id.as_deref();
        let instances = packet.instances.as_deref();
        let choosable = &packet.choosable;
        let via = packet.via.as_str();
        let body = packet.body.as_str();
        let attachments = packet.attachments.as_slice();
        let trace = packet.origin_trace_id.to_string();
        let route_started = std::time::Instant::now();

        // reliability 07 / Q8: a mistaken refusal asks for selection; accepting
        // an out-of-role or stale ID starts an unauthorized instance. Fail closed.
        let choice = crate::pm_route::parse_route_choice(&raw, choosable).filter(|c| {
            instances.is_none() || matches!(c, crate::pm_route::RouteChoice::Dispatch(id)
                if instances.is_some_and(|xs| xs.iter().any(|(aid,role)| aid == id && self.instance_role(aid).ok().as_ref() == Some(role))))
        });
        let mut eligible = choosable.clone();
        if instances.is_none() {
            eligible.push(crate::pm_route::HOLD.to_string());
        }
        let (held, rejected, role) = match &choice {
            Some(crate::pm_route::RouteChoice::Dispatch(role)) => {
                (false, false, Some(role.clone()))
            }
            Some(crate::pm_route::RouteChoice::Hold) => (true, false, None),
            None => (false, true, None),
        };
        // diagnostic-records 票 02：unparsed 记拒绝/Warn——模型回了花名册外
        // 的字符串等于这次路由被拒，关 Debug 也得看得见；dispatch 原因码带
        // 派给的角色，回答「派给谁」。
        crate::diag::note(
            if rejected { "拒绝" } else { "判定" },
            rejected,
            Some(&self.project_id),
            Some(pm_id),
            run_id,
            Some(&trace),
            "pm_route",
            &if rejected {
                "unparsed".to_string()
            } else if held {
                "hold".into()
            } else {
                format!("dispatch:{}", role.as_deref().unwrap_or("?"))
            },
            route_started,
        );
        let mut payload = json!({
            "held": held,
            "rejected": rejected,
            "via": via,
            "role": role,
            "raw": raw,
        });
        if let Some(candidates) = instances {
            // reliability 07: role remains a role in the historical event
            // contract; the selected instance has its own field.
            let selected_agent_id = role.as_deref();
            let selected_role = candidates
                .iter()
                .find(|(id, _)| Some(id.as_str()) == selected_agent_id)
                .map(|(_, role)| role);
            payload["role"] = json!(selected_role);
            payload["agent_id"] = json!(selected_agent_id);
            payload["scope"] = json!("role_instances");
        }
        // 拒绝的输出不是一次决策：不写 decision，免得花名册外的字符串
        // 被回放当成 chosen。接受/先不派活才落闭集决策。
        if let Some(chosen) = choice.as_ref().map(|c| match c {
            crate::pm_route::RouteChoice::Dispatch(role) => role.clone(),
            crate::pm_route::RouteChoice::Hold => crate::pm_route::HOLD.to_string(),
        }) {
            payload["choice"] = json!(chosen);
            payload["decision"] = json!({
                "kind": "pm_route",
                "eligible": eligible,
                "chosen": chosen,
                "via": via,
            });
        }
        let selection_event = if let Some(event) = prior_event {
            event
        } else {
            payload["original_trace_id"] = json!(packet.origin_trace_id);
            payload["model_recovery"] = json!(recovery_id);
            self.db.append_event(
                &self.project_id,
                EventKind::PmRouted,
                payload,
                Some(pm_id),
                run_id,
            )?
        };
        if let Some(qid) = recovery_id {
            if choice.is_none() {
                self.save_pm_failure(
                    packet,
                    Some(qid),
                    None,
                    &crate::provider::ProviderError::Refused(
                        "choice outside saved eligible set".into(),
                    ),
                    None,
                )?;
                return Err(ApiError::Decision(
                    "choice outside saved eligible set".into(),
                ));
            }
            crate::cards::annotate(
                &self.db,
                qid,
                &[
                    ("state", json!("choice_ready")),
                    (
                        "saved_choice",
                        json!({"raw":raw,"event_id":selection_event}),
                    ),
                ],
            )?;
            let target = match &choice {
                Some(crate::pm_route::RouteChoice::Dispatch(target)) => {
                    Some(if instances.is_some() {
                        target.clone()
                    } else {
                        self.agent_by_role(target)?
                    })
                }
                Some(crate::pm_route::RouteChoice::Hold) => packet
                    .origin
                    .kickoff_lead
                    .as_ref()
                    .map(|role| self.agent_by_role(role))
                    .transpose()?,
                None => None,
            };
            if let Some(aid) = target {
                let role = self.instance_role(&aid)?;
                self.handoff_recovered_choice(packet, qid, &aid)?;
                return Ok(UnnamedRoute::Dispatched {
                    role,
                    via: via.to_string(),
                });
            }
            self.finish_pm_recovery(qid, packet)?;
        }
        match choice {
            Some(crate::pm_route::RouteChoice::Dispatch(role)) => {
                let role = if instances.is_some() {
                    let actual_role = self.instance_role(&role)?;
                    self.dispatch_instance(&role, body, attachments)?;
                    actual_role
                } else {
                    self.dispatch(&role, body, attachments)?;
                    role
                };
                Ok(UnnamedRoute::Dispatched {
                    role,
                    via: via.to_string(),
                })
            }
            Some(crate::pm_route::RouteChoice::Hold) => {
                if let Some(role) = &packet.origin.kickoff_lead {
                    self.dispatch(role, body, attachments)?;
                    Ok(UnnamedRoute::Dispatched {
                        role: role.clone(),
                        via: via.to_string(),
                    })
                } else {
                    Ok(UnnamedRoute::Held {
                        via: via.to_string(),
                    })
                }
            }
            None => Ok(UnnamedRoute::Rejected {
                raw,
                via: via.to_string(),
            }),
        }
    }

    fn pm_choice_stopped(&self, packet: &ChoicePacket) -> Result<bool, ApiError> {
        pm_choice_stopped_at(&self.db, &self.project_id, packet)
    }

    fn handoff_recovered_choice(
        &self,
        packet: &ChoicePacket,
        qid: &str,
        aid: &str,
    ) -> Result<(), ApiError> {
        use rusqlite::OptionalExtension;
        let role = self.instance_role(aid)?;
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        // MR-01 review: stop/stage checks and the durable handoff claim share
        // one writer. A committed stop wins; a later stop concerns in-flight work.
        if self.pm_choice_stopped(packet)? || self.roster()? != packet.roster {
            drop(tx);
            self.note_handoff_failure(qid, packet, "handoff_stopped");
            return Err(ApiError::BadInput(
                "original dispatch stopped or changed".into(),
            ));
        }
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Recovery)?;
        let marker = if let Some(marker) = card.payload["handoff_event_id"].as_i64() {
            let witness: String = self.db.conn().query_row("SELECT payload FROM events WHERE id=?1 AND project_id=?2 AND agent_id=?3 AND stage_run_id IS ?4 AND kind='fastpath_dispatched'",rusqlite::params![marker,self.project_id,aid,packet.run_id],|r|r.get(0))?;
            if serde_json::from_str::<serde_json::Value>(&witness)?["model_recovery"] != qid {
                drop(tx);
                self.note_handoff_failure(qid, packet, "handoff_binding_changed");
                return Err(ApiError::BadInput("handoff binding changed".into()));
            }
            marker
        } else {
            let status: String = self.db.conn().query_row(
                "SELECT status FROM agents WHERE project_id=?1 AND id=?2",
                rusqlite::params![self.project_id, aid],
                |r| r.get(0),
            )?;
            if status == "sleeping" {
                orchestra::write_agent_status(&self.db, &self.project_id, aid, false)?;
                self.db.append_event(
                    &self.project_id,
                    EventKind::AgentActivated,
                    json!({"by":"dispatch"}),
                    Some(aid),
                    None,
                )?;
            }
            let marker = self.db.append_event(
                &self.project_id,
                EventKind::FastpathDispatched,
                json!({"role":role,"model_recovery":qid}),
                Some(aid),
                packet.run_id.as_deref(),
            )?;
            crate::cards::annotate(&self.db, qid, &[("handoff_event_id", json!(marker))])?;
            marker
        };
        let next: Option<i64> = self.db.conn().query_row("SELECT MIN(id) FROM events WHERE project_id=?1 AND agent_id=?2 AND kind='fastpath_dispatched' AND id>?3",rusqlite::params![self.project_id,aid,marker],|r|r.get(0))?;
        let child: Option<String> = self.db.conn().query_row("SELECT payload FROM events WHERE project_id=?1 AND agent_id=?2 AND stage_run_id IS ?3 AND kind='turn_started' AND id>?4 AND (?5 IS NULL OR id<?5) AND COALESCE(json_extract(payload,'$.subagent'),0)=0 ORDER BY id LIMIT 1",rusqlite::params![self.project_id,aid,packet.run_id,marker,next],|r|r.get(0)).optional()?;
        if let Some(child) = child {
            if serde_json::from_str::<serde_json::Value>(&child)?["instruction"] != packet.body {
                drop(tx);
                self.note_handoff_failure(qid, packet, "worker_binding_changed");
                return Err(ApiError::BadInput("worker task binding changed".into()));
            }
            tx.commit()?;
            // Receipt of the task closes PM recovery, not worker execution.
            // Its own unfinished model request belongs to the worker recovery path.
            return self.finish_pm_recovery(qid, packet);
        }
        if next.is_some() {
            drop(tx);
            self.note_handoff_failure(qid, packet, "worker_handoff_superseded");
            return Err(ApiError::BadInput("worker handoff superseded".into()));
        }
        let slot: Option<String> = self.db.conn().query_row(
            "SELECT model_slot FROM agents WHERE project_id=?1 AND id=?2",
            rusqlite::params![self.project_id, aid],
            |r| r.get(0),
        )?;
        if crate::provider_config::resolve_slot(
            &self.providers,
            slot.as_deref().unwrap_or("default"),
        )
        .is_none()
        {
            drop(tx);
            self.note_handoff_failure(qid, packet, "worker_provider_unavailable");
            return Err(ApiError::NoProvider(slot.unwrap_or_default()));
        }
        tx.commit()?;
        let outcome = self.run_turn_agent_with_history(
            aid,
            &packet.body,
            &packet.attachments,
            true,
            &[],
            None,
            true,
        );
        match outcome {
            Ok(crate::turn::TurnOutcome::SkippedSleeping)
            | Ok(crate::turn::TurnOutcome::SkippedCap) => {
                // The marker is only an admission intent. A stop/cap can win
                // after the transaction commits but before TurnStarted; keep
                // the card retryable instead of claiming the worker got it.
                self.note_handoff_failure(qid, packet, "worker_not_admitted");
                Err(ApiError::BadInput("worker was not admitted".into()))
            }
            Ok(_) => self.finish_pm_recovery(qid, packet),
            Err(error) => {
                // Keep the recovery card actionable after a worker crash. The
                // durable handoff marker lets a later retry reuse any child turn
                // that was already admitted instead of replaying the task.
                crate::cards::annotate(&self.db, qid, &[("state", json!("failed"))])?;
                Err(error)
            }
        }
    }

    fn note_handoff_failure(&self, qid: &str, packet: &ChoicePacket, code: &str) {
        // A saved choice remains queued, but its last attempt must be visible
        // as a failed recovery instead of looking like an idle retry.
        let _ = crate::cards::annotate(&self.db, qid, &[("state", json!("failed"))]);
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&self.project_id),
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
            Some(qid),
            "model_recovery",
            code,
            std::time::Instant::now(),
        );
    }

    pub(super) fn save_pm_failure(
        &self,
        packet: &ChoicePacket,
        recovery_id: Option<&str>,
        request_id: Option<String>,
        error: &crate::provider::ProviderError,
        actual_model: Option<&str>,
    ) -> Result<String, ApiError> {
        let started = std::time::Instant::now();
        let code = match error.cause() {
            crate::provider::ProviderError::QuotaUnavailable => "quota",
            _ => match error.http_status() {
                Some(401 | 403) => "authentication",
                Some(402) => "quota",
                Some(429) => "rate_limited",
                _ => match error.cause() {
                    crate::provider::ProviderError::Transport(_) => "transport",
                    crate::provider::ProviderError::MissingCredential(_) => "missing_credential",
                    crate::provider::ProviderError::CredentialStore(_) => "credential_store",
                    crate::provider::ProviderError::BudgetUnavailable => "budget_unavailable",
                    crate::provider::ProviderError::Interrupted => "interrupted",
                    crate::provider::ProviderError::Deadline => "deadline",
                    _ => "provider_error",
                },
            },
        };
        let mut history = match recovery_id {
            Some(qid) => crate::cards::get(&self.db, qid)?.payload["history"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            None => vec![],
        };
        history.push(json!({"request_id":request_id,"code":code,"http_status":error.http_status(),"model":actual_model,"model_slot":packet.slot_name,"message": match error.cause() {
            crate::provider::ProviderError::Transport(text) if text.to_lowercase().contains("unexpected eof") => "unexpected EOF",
            crate::provider::ProviderError::Transport(text) if text.to_lowercase().contains("connection reset") => "connection reset",
            // MR-01: provider text can contain URLs, response bodies or secrets.
            // Preserve structured status and safe categories, never persist raw text.
            _ => code,
        }}));
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let qid = if let Some(qid) = recovery_id {
            crate::cards::annotate(
                &self.db,
                qid,
                &[("state", json!("failed")), ("history", json!(history))],
            )?;
            qid.to_string()
        } else {
            crate::cards::enqueue(
                &self.db,
                &self.project_id,
                Some(&packet.pm_id),
                crate::cards::CardKind::Recovery,
                json!({"sub":"model_request","purpose":"pm_route","state":"failed","history":history,"packet":packet}),
                Some(&format!("pm_route:{}", packet.origin_trace_id)),
            )?
        };
        let view = model_request_recovery(&self.db, &self.project_id, &qid)?;
        crate::cards::annotate(&self.db, &qid, &[("view", serde_json::to_value(&view)?)])?;
        let mut event = serde_json::to_value(&view)?;
        event["kind"] = json!("model_request_failed");
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            event,
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
        )?;
        tx.commit()?;
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&self.project_id),
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
            Some(&qid),
            "pm_route",
            code,
            started,
        );
        Ok(qid)
    }

    fn finish_pm_recovery(&self, qid: &str, packet: &ChoicePacket) -> Result<(), ApiError> {
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        crate::cards::annotate(&self.db, qid, &[("state", json!("recovered"))])?;
        crate::cards::answer(&self.db, qid, "owner")?;
        let view = model_request_recovery(&self.db, &self.project_id, qid)?;
        crate::cards::annotate(&self.db, qid, &[("view", serde_json::to_value(&view)?)])?;
        let mut event = serde_json::to_value(&view)?;
        event["kind"] = json!("model_request_recovered");
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            event,
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn retry_model_request(&self, qid: &str) -> Result<UnnamedRoute, ApiError> {
        let started = std::time::Instant::now();
        // MR-01: a false negative costs one owner retry; a false positive can
        // dispatch an unreviewed superseded task. Refuse missing scope evidence.
        let _in_flight = InFlight::enter(&self.turn_depth);
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Recovery)?;
        let reject = |code: &str| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                card.agent_id.as_deref(),
                None,
                Some(qid),
                "model_recovery",
                code,
                started,
            );
            ApiError::BadInput(format!("cannot recover model request: {code}"))
        };
        if card.project_id != self.project_id
            || card.payload["sub"] != "model_request"
            || card.payload["purpose"] != "pm_route"
        {
            return Err(reject("scope_invalid"));
        }
        let packet: ChoicePacket = serde_json::from_value(card.payload["packet"].clone())
            .map_err(|_| reject("packet_missing"))?;
        if packet.root != self.repo_root.canonicalize()?
            || card.agent_id.as_deref() != Some(&packet.pm_id)
            || self.agent_by_role(crate::pm_route::PM_ROLE).ok().as_deref() != Some(&packet.pm_id)
            || self.active_run()?.map(|r| r.id) != packet.run_id
            || self.roster()? != packet.roster
        {
            return Err(reject("original_task_changed"));
        }
        if orchestra::is_paused(&self.db, &self.project_id)? {
            return Err(reject("paused"));
        }
        // MR-01: the PM can be outside the stage activation list and sleeping
        // before the original choice. Only a later explicit team stop invalidates it.
        if self.pm_choice_stopped(&packet)? {
            return Err(reject("team_stopped"));
        }
        let trace: String = self.db.conn().query_row("SELECT payload FROM events WHERE id=?1 AND project_id=?2 AND agent_id=?3 AND kind='system'",rusqlite::params![packet.origin_trace_id,self.project_id,packet.pm_id],|r|r.get(0)).map_err(|_|reject("original_request_missing"))?;
        if serde_json::from_str::<serde_json::Value>(&trace)?
            != turn::request_envelope(0, &packet.request, &packet.request.messages, &[])
        {
            return Err(reject("request_binding_changed"));
        }
        if let Some(message_id) = packet.origin.source_message {
            let (body, attachments): (String,String) = self.db.conn().query_row("SELECT body,attachments FROM messages WHERE id=?1 AND project_id=?2 AND author='owner'",rusqlite::params![message_id,self.project_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|reject("owner_message_missing"))?;
            if body != packet.body
                || serde_json::from_str::<Vec<crate::trace::AttachRef>>(&attachments)?
                    != packet.attachments
            {
                return Err(reject("owner_message_changed"));
            }
        }
        let latest = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Recovery)?;
        if latest.payload["state"] == "recovered" {
            return Err(reject("already_recovered"));
        }
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&self.project_id),
            Some(&packet.pm_id),
            packet.run_id.as_deref(),
            Some(qid),
            "model_recovery",
            "retry_original_choice",
            started,
        );
        let saved = crate::cards::get(&self.db, qid)?.payload["saved_choice"].clone();
        if !saved.is_null() {
            let raw = saved["raw"]
                .as_str()
                .ok_or_else(|| reject("selection_missing"))?;
            let event = saved["event_id"]
                .as_i64()
                .ok_or_else(|| reject("selection_event_missing"))?;
            let witness: String = self.db.conn().query_row("SELECT payload FROM events WHERE id=?1 AND project_id=?2 AND agent_id=?3 AND stage_run_id IS ?4 AND kind='pm_routed'",rusqlite::params![event,self.project_id,packet.pm_id,packet.run_id],|r|r.get(0)).map_err(|_|reject("selection_binding_missing"))?;
            let witness: serde_json::Value = serde_json::from_str(&witness)?;
            if witness["raw"] != raw
                || witness["original_trace_id"] != packet.origin_trace_id
                || witness["model_recovery"] != qid
            {
                return Err(reject("selection_binding_changed"));
            }
            let _lock = self.lock_pm_choice_with_diag(&packet)?;
            return self.deliver_pm_choice(&packet, raw.to_string(), Some(qid), Some(event));
        }
        let provider = if packet.via == "decision" {
            self.providers.get(&packet.slot_name)
        } else {
            crate::provider_config::resolve_slot(&self.providers, &packet.slot_name)
        }
        .cloned()
        .ok_or_else(|| reject("provider_unavailable"))?;
        if provider.uses_decision_api() != packet.use_jev {
            return Err(reject("provider_kind_changed"));
        }
        if packet.via == "chat"
            && crate::provider_config::fell_back_to_default(&self.providers, &packet.slot_name)
        {
            crate::diag::slot_fallback(
                Some(&self.project_id),
                Some(&packet.pm_id),
                packet.run_id.as_deref(),
                "model_recovery",
                &packet.slot_name,
                started,
            );
        }
        self.execute_saved_pm_choice(&packet, provider, Some(qid))
    }
}

enum PmAttemptEvent {
    Admitted(String),
    Finished(Result<crate::provider::ChatResponse, crate::provider::ProviderError>),
}

fn send_pm_choice(
    provider: &dyn ModelProvider,
    packet: &ChoicePacket,
    deadline: std::time::Instant,
) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    // MR-02: fixtures and transports obey the same admission boundary.
    if remaining.is_zero() {
        return Err(crate::provider::ProviderError::Deadline);
    }
    if packet.use_jev {
        let state = crate::pm_route::choice_state(
            packet.stage_name.as_deref(),
            &packet.activation,
            if packet.origin.from_owner {
                "The owner"
            } else {
                &packet.speaker
            },
            &packet.body,
        );
        let mut options: Vec<_> = packet
            .choosable
            .iter()
            .map(|r| (r.as_str(), "eligible target"))
            .collect();
        if packet.instances.is_none() {
            options.push((crate::pm_route::HOLD_TOKEN, "wake no one for now"));
        }
        provider.decide_with_timeout(&state, &options, remaining)
    } else {
        provider.complete_with_timeout(&packet.request, remaining)
    }
}

fn pm_choice_stopped_at(db: &Db, project: &str, packet: &ChoicePacket) -> Result<bool, ApiError> {
    if orchestra::is_paused(db, project)?
        || db.active_stage_run(project)?.map(|r| r.id) != packet.run_id
    {
        return Ok(true);
    }
    let stopped = db.conn().query_row("SELECT EXISTS(SELECT 1 FROM events WHERE project_id=?1 AND kind='team_slept' AND id>?2) AND NOT EXISTS(SELECT 1 FROM agents WHERE project_id=?1 AND status!='sleeping')", rusqlite::params![project, packet.origin_trace_id], |r| r.get(0))?;
    Ok(stopped)
}
