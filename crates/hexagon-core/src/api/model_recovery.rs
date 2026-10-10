//! MR-01 / ADR 0083: recover the saved closed choice, never reroute the owner
//! message through stage kickoff or replay a completed worker turn.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct ChoiceOrigin {
    pub from_owner: bool,
    pub source_message: Option<i64>,
    pub kickoff_lead: Option<String>,
    /// test3 05/06 review: the stall investigation keeps its own stall-card
    /// contract (internal decision recovery is ticket 10). Wiring it into the
    /// model-request card produced two pending items for one failure.
    #[serde(default)]
    pub stall_investigation: bool,
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
    pub partial_text: Option<String>,
    pub unfinished: bool,
    /// MR-02 progress: end of the automatic retry window while `retrying`.
    #[ts(type = "number | null")]
    pub retry_deadline_ms: Option<i64>,
}

pub(super) fn retry_budget_allows(
    elapsed: std::time::Duration,
    budget: std::time::Duration,
) -> bool {
    elapsed < budget
}

/// MR-02 failure vocabulary shared by every recovery purpose (core, UI entry
/// buttons and i18n read the same codes). Typed causes win over HTTP status:
/// the same status can mean quota or rate limiting, so a confirmed quota code
/// is never retried as a temporary limit (test3 05/06 review).
pub(super) fn failure_code(error: &crate::provider::ProviderError) -> &'static str {
    use crate::provider::ProviderError as E;
    match error.cause() {
        E::QuotaUnavailable => return "quota",
        E::Unsupported(_) => return "configuration",
        E::InvalidResponse(_) => return "invalid_response",
        _ => {}
    }
    match error.http_status() {
        Some(401 | 403) => "authentication",
        Some(402) => "quota",
        Some(429) => "rate_limited",
        Some(500..=599) => "provider_unavailable",
        Some(400..=499) => "request_rejected",
        _ => match error.cause() {
            E::Transport(_) => "transport",
            E::MissingCredential(_) => "missing_credential",
            E::CredentialStore(_) => "credential_store",
            E::BudgetUnavailable => "budget_unavailable",
            E::Interrupted => "interrupted",
            E::Deadline => "deadline",
            _ => "provider_error",
        },
    }
}

/// One persisted attempt. MR-01: provider text can contain URLs, response
/// bodies or secrets; keep structured status and safe categories only.
pub(super) fn failure_entry(
    code: &str,
    error: Option<&crate::provider::ProviderError>,
    request_id: Option<String>,
    model: Option<&str>,
    slot: &str,
) -> Value {
    let message = match error.map(|e| e.cause()) {
        Some(crate::provider::ProviderError::Transport(text))
            if text.to_lowercase().contains("unexpected eof") =>
        {
            "unexpected EOF"
        }
        Some(crate::provider::ProviderError::Transport(text))
            if text.to_lowercase().contains("connection reset") =>
        {
            "connection reset"
        }
        _ => code,
    };
    json!({"request_id":request_id,"code":code,"http_status":error.and_then(|e| e.http_status()),
        "model":model,"model_slot":slot,"message":message})
}

/// Wall-clock end of the current automatic retry window, for progress display.
pub(super) fn retry_deadline_ms(remaining: std::time::Duration) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from((now + remaining).as_millis()).unwrap_or(i64::MAX)
}

/// MR-02 budget measured on the injectable workbench clock (tests advance it;
/// production uses the system clock). Real IO deadlines derive from what remains.
pub(super) struct RetryWindow {
    clock: Arc<dyn crate::stallwatch::StallClock>,
    started: std::time::Instant,
    budget: std::time::Duration,
}

impl RetryWindow {
    pub(super) fn start(
        clock: Arc<dyn crate::stallwatch::StallClock>,
        budget: std::time::Duration,
    ) -> Self {
        let started = clock.now();
        Self {
            clock,
            started,
            budget,
        }
    }

    fn elapsed(&self) -> std::time::Duration {
        self.clock.now().saturating_duration_since(self.started)
    }

    pub(super) fn remaining(&self) -> std::time::Duration {
        self.budget.saturating_sub(self.elapsed())
    }

    pub(super) fn allows(&self) -> bool {
        retry_budget_allows(self.elapsed(), self.budget)
    }

    pub(super) fn io_deadline(&self) -> std::time::Instant {
        std::time::Instant::now() + self.remaining()
    }
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
    {
        return Err(ApiError::BadInput("model recovery scope invalid".into()));
    }
    let purpose = card.payload["purpose"].as_str().unwrap_or_default();
    let (project_root, agent_id, run_id, target_role, partial_text, unfinished) = if purpose
        == "turn"
    {
        let packet: TurnRecoveryPacket = serde_json::from_value(card.payload["packet"].clone())?;
        (
            packet.root,
            packet.agent_id,
            packet.run_id,
            Some(packet.role),
            packet.partial_text,
            true,
        )
    } else if purpose == "pm_route" {
        let packet: ChoicePacket = serde_json::from_value(card.payload["packet"].clone())?;
        (
            packet.root,
            packet.pm_id,
            packet.run_id,
            packet
                .origin
                .kickoff_lead
                .or_else(|| packet.activation.first().cloned()),
            None,
            false,
        )
    } else {
        return Err(ApiError::BadInput("model recovery purpose invalid".into()));
    };
    Ok(ModelRequestRecovery {
        recovery_id: card.id,
        project_root: project_root.to_string_lossy().into_owned(),
        purpose: purpose.into(),
        agent_id,
        stage_run_id: run_id,
        target_role,
        state: card.payload["state"].as_str().unwrap_or("failed").into(),
        history: serde_json::from_value(card.payload["history"].clone())?,
        // test3 05/06 review: a retrying card is already running; offering the
        // shortcut only produced an already_running error toast.
        can_retry: card.state == crate::cards::CardState::Queued
            && card.payload["state"] != "retrying",
        unfinished: partial_text.is_some() && unfinished,
        partial_text,
        retry_deadline_ms: card.payload["retry_deadline_ms"].as_i64(),
    })
}

/// Who owns a recovery item: one card per original request (dedupe key).
pub(super) struct RecoveryScope<'a> {
    pub purpose: &'static str,
    pub agent_id: &'a str,
    pub run_id: Option<&'a str>,
    pub dedupe: String,
    pub packet: Value,
}

impl Workbench {
    /// MR-01/MR-02: every failed attempt updates the same item (or creates it)
    /// in one writer transaction, then republishes the view and timeline row.
    pub(super) fn record_failed_attempt(
        &self,
        scope: &RecoveryScope<'_>,
        recovery_id: Option<&str>,
        entry: Value,
        extra: &[(&str, Value)],
    ) -> Result<String, ApiError> {
        let started = std::time::Instant::now();
        let code = entry["code"]
            .as_str()
            .unwrap_or("provider_error")
            .to_string();
        let mut history = match recovery_id {
            Some(qid) => crate::cards::get(&self.db, qid)?.payload["history"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            None => vec![],
        };
        history.push(entry);
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let mut fields = vec![
            ("state", json!("failed")),
            ("history", json!(history)),
            ("retry_deadline_ms", Value::Null),
        ];
        fields.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
        let qid = if let Some(qid) = recovery_id {
            crate::cards::annotate(&self.db, qid, &fields)?;
            qid.to_string()
        } else {
            let mut payload =
                json!({"sub":"model_request","purpose":scope.purpose,"packet":scope.packet});
            for (key, value) in &fields {
                payload[*key] = value.clone();
            }
            crate::cards::enqueue(
                &self.db,
                &self.project_id,
                Some(scope.agent_id),
                crate::cards::CardKind::Recovery,
                payload,
                Some(&scope.dedupe),
            )?
        };
        self.publish_recovery_view(&qid, scope.agent_id, scope.run_id, "model_request_failed")?;
        tx.commit()?;
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&self.project_id),
            Some(scope.agent_id),
            scope.run_id,
            Some(&qid),
            "model_recovery",
            &code,
            started,
        );
        Ok(qid)
    }

    /// Store the derived view on the card and append the matching timeline row.
    pub(super) fn publish_recovery_view(
        &self,
        qid: &str,
        agent_id: &str,
        run_id: Option<&str>,
        kind: &str,
    ) -> Result<(), ApiError> {
        let view = model_request_recovery(&self.db, &self.project_id, qid)?;
        crate::cards::annotate(&self.db, qid, &[("view", serde_json::to_value(&view)?)])?;
        let mut event = serde_json::to_value(view)?;
        event["kind"] = json!(kind);
        self.db.append_event(
            &self.project_id,
            EventKind::System,
            event,
            Some(agent_id),
            run_id,
        )?;
        Ok(())
    }

    /// Move an item between non-terminal states (retrying/stopped/failed).
    pub(super) fn publish_recovery_state(
        &self,
        qid: &str,
        agent_id: &str,
        run_id: Option<&str>,
        state: &str,
        retry_deadline_ms: Option<i64>,
    ) -> Result<(), ApiError> {
        crate::cards::annotate(
            &self.db,
            qid,
            &[
                ("state", json!(state)),
                ("retry_deadline_ms", json!(retry_deadline_ms)),
            ],
        )?;
        let kind = if state == "retrying" {
            "model_request_retrying"
        } else {
            "model_request_failed"
        };
        self.publish_recovery_view(qid, agent_id, run_id, kind)
    }

    /// Success closes the item; earlier failures stay in its history.
    pub(super) fn finish_recovery(
        &self,
        qid: &str,
        agent_id: &str,
        run_id: Option<&str>,
    ) -> Result<(), ApiError> {
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?;
        crate::cards::annotate(
            &self.db,
            qid,
            &[
                ("state", json!("recovered")),
                ("retry_deadline_ms", Value::Null),
            ],
        )?;
        crate::cards::answer(&self.db, qid, "owner")?;
        self.publish_recovery_view(qid, agent_id, run_id, "model_request_recovered")?;
        tx.commit()?;
        Ok(())
    }

    pub fn retry_model_request(&self, qid: &str) -> Result<UnnamedRoute, ApiError> {
        let started = std::time::Instant::now();
        // MR-01: a false negative costs one owner retry; a false positive can
        // dispatch an unreviewed superseded task. Refuse missing scope evidence.
        let _in_flight = InFlight::enter(&self.turn_depth);
        let card = crate::cards::get_queued(&self.db, qid, crate::cards::CardKind::Recovery)?;
        if card.project_id != self.project_id || card.payload["sub"] != "model_request" {
            return Err(self.reject_recovery(&card, "scope_invalid", started));
        }
        match card.payload["purpose"].as_str() {
            Some("pm_route") => self.retry_pm_choice(&card, started),
            Some("turn") => self.retry_turn_model_request(&card, started),
            _ => Err(self.reject_recovery(&card, "scope_invalid", started)),
        }
    }

    /// Every refused recovery is a Warn diagnostic with a reason code only.
    pub(super) fn reject_recovery(
        &self,
        card: &crate::cards::Card,
        code: &str,
        started: std::time::Instant,
    ) -> ApiError {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&self.project_id),
            card.agent_id.as_deref(),
            None,
            Some(&card.id),
            "model_recovery",
            code,
            started,
        );
        ApiError::BadInput(format!("cannot recover model request: {code}"))
    }

    /// MR-02: one in-flight attempt per original request. Automatic and manual
    /// recovery lock the original request key, not just its later card; a
    /// detached accounting thread keeps the lock until IO ends.
    pub(super) fn lock_request(
        &self,
        key: &str,
        agent_id: &str,
        run_id: Option<&str>,
    ) -> Result<std::fs::File, ApiError> {
        let started = std::time::Instant::now();
        let refuse = |code: &'static str| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project_id),
                Some(agent_id),
                run_id,
                Some(key),
                "model_recovery",
                code,
                started,
            );
            ApiError::BadInput(code.into())
        };
        let dir = self.repo_root.join(".hexagon/request-locks");
        let path = dir.join(format!("{key}.lock"));
        let is_link = |p: &Path| {
            p.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        };
        if is_link(&dir) || is_link(&path) {
            return Err(refuse("lock_path_invalid"));
        }
        std::fs::create_dir_all(&dir).map_err(|_| refuse("lock_unavailable"))?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| refuse("lock_unavailable"))?;
        lock.try_lock().map_err(|_| refuse("already_running"))?;
        Ok(lock)
    }

    /// MR-02: one-minute owner/manual and five-minute background windows,
    /// never longer than the configured wait policy.
    pub(super) fn request_budget(&self, interactive: bool) -> std::time::Duration {
        self.wait_policy
            .budget
            .min(std::time::Duration::from_secs(if interactive {
                60
            } else {
                300
            }))
    }
}

mod pm;
mod role_turn;
pub(in crate::api) use role_turn::TurnRecoveryPacket;
