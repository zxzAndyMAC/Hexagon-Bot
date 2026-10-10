//! MR-02/MR-06: the model-request failure vocabulary and bounded transient
//! retry shared by every model purpose — dispatch, role turns and internal
//! judges. It lives below `api` so judge modules can use it without
//! depending on the facade (AGENTS 架构接缝: core must not `use crate::api`).
use crate::db::Db;
use crate::provider::ProviderError;
use crate::tools::ToolContext;
use serde_json::{json, Value};

/// MR-02 failure vocabulary shared by every recovery purpose (core, UI entry
/// buttons and i18n read the same codes). Typed causes win over HTTP status:
/// the same status can mean quota or rate limiting, so a confirmed quota code
/// is never retried as a temporary limit (test3 05/06 review).
pub fn failure_code(error: &crate::provider::ProviderError) -> &'static str {
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
pub fn failure_entry(
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

/// A failed internal judge request (MR-06): the persisted fact a fail-closed
/// owner card carries so it never reads like a business verdict.
pub fn judge_failure(purpose: &str, error: &ProviderError, slot: &str) -> Value {
    let mut entry = failure_entry(failure_code(error), Some(error), None, None, slot);
    entry["purpose"] = json!(purpose);
    entry
}

/// MR-06: retry only confirmed transient failures inside a dedicated window
/// no longer than the owner minute (judges keep stricter deadlines: the
/// configured wait policy may only shorten it). Every attempt goes through
/// `call`, so each actual send keeps its own ledger row. A stop/sleep ends
/// the wait; the last error is returned unchanged for the fail-closed path.
pub fn bounded_call<T>(
    db: &Db,
    ctx: &ToolContext,
    mut call: impl FnMut() -> Result<T, ProviderError>,
) -> Result<T, ProviderError> {
    let window = ctx.wait.budget.min(std::time::Duration::from_secs(60));
    let started = std::time::Instant::now();
    loop {
        let error = match call() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let next = std::time::Instant::now() + ctx.wait.probe_interval;
        if !error.retryable() || next.saturating_duration_since(started) >= window {
            return Err(error);
        }
        while std::time::Instant::now() < next {
            // Judges run for sleeping proposal authors too, so the turn
            // kernel's halt (which counts a sleeping agent) would cancel every
            // retry. The owner's stop for a judge is the project pause; an
            // unreadable pause state counts as paused (a false stop costs one
            // manual re-judge, a false go keeps sending after a stop).
            if crate::orchestra::is_paused(db, &ctx.project_id).unwrap_or(true) {
                return Err(error);
            }
            let slice = ctx.wait.tick.max(std::time::Duration::from_millis(1));
            std::thread::sleep(
                slice.min(next.saturating_duration_since(std::time::Instant::now())),
            );
        }
    }
}
