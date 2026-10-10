//! MR-07 (test3 11): wizard generation requests (brief questions, project
//! instructions, role drafts, flow draft, role duty). No project exists yet,
//! so nothing is written to a project trace or card: the failure goes back to
//! the wizard step that asked, which keeps the owner's input and drafts.
//!
//! One owner-started request gets one minute in total (every send and every
//! wait); only confirmed transient failures retry. One request per kind is in
//! flight; an explicit stop ends the wait and discards any late result.
use crate::provider::{ChatRequest, ChatResponse, ModelProvider};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A typed generation failure. IPC code is `model_<code>` so the wizard can
/// show the matching remedy without parsing provider prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationFailure {
    pub code: String,
    pub attempts: u32,
}

impl std::fmt::Display for GenerationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model generation failed: {} after {} attempt(s)",
            self.code, self.attempts
        )
    }
}

impl std::error::Error for GenerationFailure {}

impl crate::errcode::ErrorCode for GenerationFailure {
    fn code(&self) -> String {
        format!("model_{}", self.code)
    }
}

const BUDGET: Duration = Duration::from_secs(60);

#[cfg(not(test))]
const PROBE: Duration = Duration::from_secs(2);
#[cfg(test)]
const PROBE: Duration = Duration::from_millis(5);

static STOP_EPOCH: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static IN_FLIGHT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The wizard is one per process, so stop and single-flight are process
/// wide. Test threads run in parallel in one process; scope them per thread
/// so one test's Stop or in-flight claim cannot leak into another.
#[cfg(not(test))]
fn scope() -> String {
    String::new()
}
#[cfg(test)]
fn scope() -> String {
    format!("{:?}", std::thread::current().id())
}

fn stop_epoch() -> u64 {
    let guard = STOP_EPOCH.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|map| map.get(&scope()).copied())
        .unwrap_or(0)
}

/// Stop every wizard generation in flight (the owner pressed Stop, changed
/// the step, or closed the wizard). Late results are discarded.
pub fn stop_generation() {
    let mut guard = STOP_EPOCH.lock().unwrap_or_else(|e| e.into_inner());
    *guard
        .get_or_insert_with(HashMap::new)
        .entry(scope())
        .or_insert(0) += 1;
}

struct Claim(String);

impl Claim {
    fn take(kind: &'static str) -> Option<Self> {
        let key = format!("{}:{kind}", scope());
        let mut guard = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        // Lazy on purpose: an eagerly built Claim would be dropped (and its
        // Drop would re-lock IN_FLIGHT) while this guard is held — deadlock.
        let inserted = guard.get_or_insert_with(HashSet::new).insert(key.clone());
        inserted.then(|| Self(key))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut guard = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = guard.as_mut() {
            set.remove(&self.0);
        }
    }
}

fn fail(code: &str, attempts: u32) -> super::SetupError {
    super::SetupError::Generation(GenerationFailure {
        code: code.into(),
        attempts,
    })
}

/// Run one wizard generation request under the MR-07 contract.
pub(super) fn generate(
    kind: &'static str,
    provider: &dyn ModelProvider,
    req: &ChatRequest,
) -> Result<ChatResponse, super::SetupError> {
    let _claim = Claim::take(kind).ok_or_else(|| fail("already_running", 0))?;
    let epoch = stop_epoch();
    let stopped = || stop_epoch() != epoch;
    let started = Instant::now();
    let mut attempts = 0;
    loop {
        let remaining = BUDGET.saturating_sub(started.elapsed());
        if stopped() {
            return Err(fail("interrupted", attempts));
        }
        if remaining.is_zero() {
            return Err(fail("deadline", attempts));
        }
        attempts += 1;
        let error = match provider.complete_with_timeout(req, remaining) {
            // A result that arrives after Stop never overwrites newer input.
            Ok(_) if stopped() => return Err(fail("interrupted", attempts)),
            Ok(response) => return Ok(response),
            Err(error) => error,
        };
        let next = Instant::now() + PROBE;
        if !error.retryable() || next.saturating_duration_since(started) >= BUDGET {
            return Err(fail(crate::model_failure::failure_code(&error), attempts));
        }
        while Instant::now() < next {
            if stopped() {
                return Err(fail("interrupted", attempts));
            }
            std::thread::sleep(Duration::from_millis(5).min(next - Instant::now()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderError, ScriptedProvider};
    use crate::turn::text_response;

    struct Flaky(Mutex<Vec<Result<ChatResponse, ProviderError>>>, Mutex<u32>);

    impl ModelProvider for Flaky {
        fn is_scripted(&self) -> bool {
            true
        }
        fn complete(&self, _: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            *self.1.lock().unwrap() += 1;
            self.0.lock().unwrap().remove(0)
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            model_slot: "default".into(),
            messages: vec![],
            tools: vec![],
        }
    }

    #[test]
    fn transient_failure_retries_once_and_permanent_failure_is_typed() {
        let flaky = Flaky(
            Mutex::new(vec![
                Err(ProviderError::Transport("unexpected EOF".into())),
                Ok(text_response("ok")),
            ]),
            Mutex::new(0),
        );
        assert!(generate("t_retry", &flaky, &req()).is_ok());
        assert_eq!(*flaky.1.lock().unwrap(), 2);
        let auth = Flaky(
            Mutex::new(vec![Err(ProviderError::HttpStatus(
                Box::new(ProviderError::Refused("HTTP 401".into())),
                401,
                None,
            ))]),
            Mutex::new(0),
        );
        let error = generate("t_auth", &auth, &req()).unwrap_err();
        assert_eq!(
            crate::errcode::ErrorCode::code(&error),
            "model_authentication"
        );
        assert_eq!(*auth.1.lock().unwrap(), 1, "authentication never waits");
        assert!(!error.to_string().contains("HTTP 401 body"));
    }

    #[test]
    fn one_request_per_kind_is_in_flight() {
        let _held = Claim::take("t_single").unwrap();
        let provider = ScriptedProvider::new(vec![text_response("ok")]);
        let error = generate("t_single", &provider, &req()).unwrap_err();
        assert_eq!(
            crate::errcode::ErrorCode::code(&error),
            "model_already_running"
        );
        assert!(provider.recorded().is_empty());
    }

    #[test]
    fn stop_discards_a_late_result() {
        struct StopsDuring;
        impl ModelProvider for StopsDuring {
            fn is_scripted(&self) -> bool {
                true
            }
            fn complete(&self, _: &ChatRequest) -> Result<ChatResponse, ProviderError> {
                stop_generation();
                Ok(text_response("late"))
            }
        }
        let error = generate("t_stop", &StopsDuring, &req()).unwrap_err();
        assert_eq!(crate::errcode::ErrorCode::code(&error), "model_interrupted");
    }
}
