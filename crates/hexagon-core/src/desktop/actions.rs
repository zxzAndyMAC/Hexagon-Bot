//! Ticket 07 / owner 2026-10-01: one desktop, one role, explicit owner pause.
//! Native input delivery is not proof of application success. Unknown delivery
//! never gains an idempotency contract or permits replay of a stale screenshot.
use crate::{db::Db, tools::ToolError, trace::EventKind};
use base64::Engine as _;
use rusqlite::OptionalExtension as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesktopStatus {
    pub project_root: String,
    pub enabled: bool,
    pub active_project: Option<String>,
    pub active_agent: Option<String>,
    pub busy: bool,
    pub paused: bool,
    pub outcome_unknown: bool,
    pub screenshot_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub capture_preparation: Option<super::CapturePreparationState>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DesktopScreenshot {
    pub name: String,
    pub data_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum DesktopControl {
    Enable,
    Disable,
    Pause,
    Resume,
    Release,
    ClearScreenshots,
    PrepareCapture,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub click_type: Option<String>,
}

impl NativeRequest {
    pub(crate) fn validate(&self) -> Result<(), ToolError> {
        let bad = |message: &str| ToolError::BadInput(message.into());
        if self.window_id == Some(0) || self.display_index.is_some_and(|v| v > 15) {
            return Err(bad("invalid desktop window or display"));
        }
        if !self.is_read()
            && self.op != "activate"
            && self
                .snapshot_id
                .as_ref()
                .is_none_or(|s| s.is_empty() || s.len() > 256)
        {
            return Err(bad("a fresh snapshot_id is required"));
        }
        match self.op.as_str() {
            "list_apps" | "observe" | "observe_screen" | "release" => {}
            "activate" => {
                if self
                    .app_id
                    .as_ref()
                    .is_none_or(|s| s.is_empty() || s.len() > 128)
                {
                    return Err(bad("activate requires a discovered app_id"));
                }
            }
            "click" | "drag" => {
                let points = if self.op == "drag" {
                    vec![self.x, self.y, self.to_x, self.to_y]
                } else {
                    vec![self.x, self.y]
                };
                if points
                    .iter()
                    .any(|v| v.is_none_or(|v| !v.is_finite() || v < 0.0))
                {
                    return Err(bad("click/drag requires finite image coordinates"));
                }
                if !matches!(
                    self.click_type.as_deref(),
                    None | Some("single" | "double" | "right")
                ) || self.duration_ms.is_some_and(|v| !(100..=2000).contains(&v))
                {
                    return Err(bad("invalid pointer options"));
                }
            }
            "type" => {
                if self.text.as_ref().is_none_or(|s| s.len() > 16_384) {
                    return Err(bad("literal text is required (maximum 16 KiB)"));
                }
            }
            "key" => {
                let parts: Vec<_> = self.keys.as_deref().unwrap_or("").split(',').collect();
                let named = [
                    "cmd",
                    "shift",
                    "alt",
                    "ctrl",
                    "space",
                    "return",
                    "tab",
                    "escape",
                    "delete",
                    "arrow_up",
                    "arrow_down",
                    "arrow_left",
                    "arrow_right",
                    "home",
                    "end",
                    "pageup",
                    "pagedown",
                ];
                if parts.len() > 4
                    || parts.iter().any(|key| {
                        !named.contains(key)
                            && !(key.len() == 1
                                && key
                                    .bytes()
                                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
                    })
                {
                    return Err(bad("invalid keyboard chord"));
                }
            }
            "scroll" => {
                if !matches!(
                    self.direction.as_deref(),
                    Some("up" | "down" | "left" | "right")
                ) || self.amount.is_none_or(|v| !(1..=20).contains(&v))
                    || self
                        .element_id
                        .as_ref()
                        .is_none_or(|v| v.is_empty() || v.len() > 256)
                {
                    return Err(bad(
                        "scroll requires an observed element, direction and amount 1–20",
                    ));
                }
            }
            _ => return Err(bad("unknown desktop operation")),
        }
        Ok(())
    }

    pub fn is_read(&self) -> bool {
        matches!(
            self.op.as_str(),
            "list_apps" | "observe" | "observe_screen" | "release"
        )
    }
    fn is_observation(&self) -> bool {
        matches!(self.op.as_str(), "observe" | "observe_screen")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativeReply {
    pub protocol_version: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applications: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub outcome_unknown: bool,
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Value>,
}

mod lease;

pub(crate) trait Backend: Send + Sync {
    fn needs_system_lease(&self) -> bool {
        false
    }
    fn start(&self, request: &str) -> Result<u64, String>;
    fn poll(&self, id: u64) -> Result<Option<NativeReply>, String>;
    fn cancel(&self, id: u64) -> Result<(), String>;
}

/// Host-only dispatch metadata; the browser adapter derives these flags from its
/// closed operation enum, never from model-supplied JSON (extension issue 12).
pub(crate) struct BrowserLaneRequest {
    pub serialized: String,
    pub read: bool,
    pub observation: bool,
    pub bootstrap: bool,
    pub snapshot_id: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Executor {
    #[default]
    Native,
    Browser,
}

#[derive(Clone)]
struct LaneRequest {
    executor: Executor,
    read: bool,
    observation: bool,
    bootstrap: bool,
    snapshot_id: Option<String>,
}

impl From<&NativeRequest> for LaneRequest {
    fn from(request: &NativeRequest) -> Self {
        Self {
            executor: Executor::Native,
            read: request.is_read(),
            observation: request.is_observation(),
            bootstrap: request.op == "activate",
            snapshot_id: request.snapshot_id.clone(),
        }
    }
}

struct NativeBackend;

#[cfg(target_os = "macos")]
type StartFn = unsafe extern "C" fn(*const std::ffi::c_char) -> u64;
#[cfg(target_os = "macos")]
type PollFn = unsafe extern "C" fn(u64) -> *mut std::ffi::c_char;
#[cfg(target_os = "macos")]
type FreeFn = unsafe extern "C" fn(*mut std::ffi::c_char);
#[cfg(target_os = "macos")]
type CancelFn = unsafe extern "C" fn(u64);

#[cfg(target_os = "macos")]
fn bindings() -> Result<(StartFn, PollFn, FreeFn, CancelFn), String> {
    let guard = super::LIBRARY
        .get()
        .ok_or("computer-use component not initialized")?
        .lock()
        .map_err(|_| "computer-use loader unavailable")?;
    let lib = guard
        .as_ref()
        .ok_or("computer-use component not initialized")?;
    // Fixed ABI, process-lifetime retained library. Copy pointers before calling
    // Swift: AppKit can reenter the event loop and must never hold this mutex.
    unsafe {
        Ok((
            *lib.get::<StartFn>(b"hexagon_computer_start_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<PollFn>(b"hexagon_computer_poll_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<FreeFn>(b"hexagon_computer_free_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<CancelFn>(b"hexagon_computer_cancel_v1\0")
                .map_err(|e| e.to_string())?,
        ))
    }
}

impl Backend for NativeBackend {
    fn needs_system_lease(&self) -> bool {
        true
    }
    fn start(&self, request: &str) -> Result<u64, String> {
        #[cfg(target_os = "macos")]
        {
            let (start, _, _, _) = bindings()?;
            let json = std::ffi::CString::new(request).map_err(|_| "invalid native request")?;
            Ok(unsafe { start(json.as_ptr()) })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            Err("macOS is required".into())
        }
    }
    fn poll(&self, id: u64) -> Result<Option<NativeReply>, String> {
        #[cfg(target_os = "macos")]
        {
            let (_, poll, free, _) = bindings()?;
            let ptr = unsafe { poll(id) };
            if ptr.is_null() {
                return Ok(None);
            }
            // The component promises valid allocation; bound decoding before
            // copying. Always free through its allocator, including bad replies.
            let len = unsafe { libc::strnlen(ptr, 8 * 1024 * 1024 + 1) };
            let result = if len > 8 * 1024 * 1024 {
                Err("native reply exceeds limit".into())
            } else {
                serde_json::from_slice::<NativeReply>(unsafe {
                    std::slice::from_raw_parts(ptr.cast::<u8>(), len)
                })
                .map_err(|_| "invalid native reply".into())
                .and_then(|reply| {
                    if reply.protocol_version == 1 {
                        Ok(Some(reply))
                    } else {
                        Err("incompatible native action protocol".into())
                    }
                })
            };
            unsafe { free(ptr) };
            result
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = id;
            Err("macOS is required".into())
        }
    }
    fn cancel(&self, id: u64) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let (_, _, _, cancel) = bindings()?;
            unsafe { cancel(id) };
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = id;
            Err("macOS is required".into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Owner {
    project: PathBuf,
    agent: String,
}

#[derive(Default)]
struct State {
    system_lease: Option<std::fs::File>,
    owner: Option<Owner>,
    paused: bool,
    busy: bool,
    active: Option<u64>,
    active_backend: Option<Arc<dyn Backend>>,
    executor: Executor,
    outcome_unknown: bool,
    snapshot: Option<String>,
}

struct Controller {
    backend: Arc<dyn Backend>,
    state: Mutex<State>,
}
static CONTROLLER: OnceLock<Arc<Controller>> = OnceLock::new();
static EVIDENCE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn controller() -> &'static Arc<Controller> {
    CONTROLLER.get_or_init(|| {
        Arc::new(Controller {
            backend: Arc::new(NativeBackend),
            state: Mutex::new(State::default()),
        })
    })
}

/// Shell supplies a bundled absolute path at startup; never accept a model path.
/// Missing system grants are reported by permissions(), not initialization errors.
pub fn initialize(library: &Path) -> Result<(), String> {
    let raw = super::native_permissions(library)?;
    if raw & !7 != 0x1000 {
        return Err("incompatible computer-use component".into());
    }
    #[cfg(target_os = "macos")]
    {
        bindings()?;
    }
    Ok(())
}

impl State {
    fn close_project(&mut self, project: &Path) -> Result<(), String> {
        let started = std::time::Instant::now();
        let agent = self.owner.as_ref().map(|owner| owner.agent.clone());
        let activation = self.active.map(|id| id.to_string());
        let mut reason = "owner_released";
        let result = (|| {
            // Benchmark I1 / 2026-10-06: preview Stop never reached a role's
            // active executor. A false rejection costs retrying Close; accepting an
            // undrained operation allows a later input into another project.
            if self
                .owner
                .as_ref()
                .is_some_and(|owner| owner.project != project)
            {
                reason = "foreign_owner_untouched";
                return Ok(());
            }
            let Some(owner) = self.owner.clone() else {
                reason = if self.busy {
                    "focus_drain_pending"
                } else {
                    "no_owner"
                };
                return if self.busy {
                    Err("computer focus has not stopped yet".into())
                } else {
                    Ok(())
                };
            };
            self.paused = true;
            self.snapshot = None;
            if let Some(id) = self.active {
                self.cancel_active(id).inspect_err(|_| {
                    reason = "native_cancel_failed";
                })?;
            }
            if self.busy {
                reason = "native_drain_pending";
                return Err("computer operation has not stopped yet; retry after it drains".into());
            }
            // Existing session release preserves pause/unknown and drops the lease.
            // Owner Release resets unknown, so it cannot stand in for verified drain.
            self.session(&owner, "release")
                .map(|_| ())
                .inspect_err(|_| {
                    reason = "owner_release_failed";
                })
        })();
        // AGENTS / benchmark I1: cancellation asks the executor to stop; this
        // record identifies the actual branch without claiming it has drained.
        crate::diag::note(
            if result.is_err() {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_HOST
            },
            result.is_err(),
            Some(crate::PROJECT_ID),
            agent.as_deref(),
            None,
            activation.as_deref(),
            "desktop_project_close",
            reason,
            started,
        );
        result
    }

    fn cancel_active(&self, id: u64) -> Result<(), String> {
        if let Some(backend) = self.active_backend.as_ref() {
            backend.cancel(id)?;
        }
        Ok(())
    }

    fn control(&mut self, project: &Path, action: DesktopControl) -> Result<Option<u64>, String> {
        if self.owner.as_ref().is_some_and(|v| v.project != project)
            && matches!(
                action,
                DesktopControl::Resume | DesktopControl::Release | DesktopControl::PrepareCapture
            )
        {
            return Err("desktop is held by another project".into());
        }
        match action {
            DesktopControl::Pause => {
                self.paused = true;
                self.snapshot = None;
                return Ok(self.active);
            }
            DesktopControl::Disable => {
                if self.owner.as_ref().is_none_or(|v| v.project == project) {
                    self.paused = true;
                    self.snapshot = None;
                    return Ok(self.active);
                }
            }
            DesktopControl::Resume => {
                if self.busy {
                    return Err("native operation has not stopped yet".into());
                }
                self.paused = false;
                self.snapshot = None;
            }
            DesktopControl::Release => {
                if self.busy {
                    return Err(
                        "pause and wait for the native operation before releasing control".into(),
                    );
                }
                *self = State::default();
            }
            DesktopControl::ClearScreenshots if self.busy => {
                return Err("wait for the current capture before clearing screenshots".into());
            }
            DesktopControl::PrepareCapture if self.busy => {
                return Err(
                    "wait for the current operation before checking capture readiness".into(),
                );
            }
            DesktopControl::Enable
            | DesktopControl::ClearScreenshots
            | DesktopControl::PrepareCapture => {}
        }
        Ok(None)
    }

    #[cfg(test)]
    fn claim(&mut self, owner: &Owner, request: &NativeRequest) -> Result<(), String> {
        self.claim_lane(owner, &LaneRequest::from(request))
    }

    // Owner issue19 / 2026-10-02: session management never resumes an owner's
    // pause. False negative costs a retry; false positive steals another role's
    // desktop. Refuse busy/foreign ownership and preserve reconciliation state.
    fn session(&mut self, owner: &Owner, op: &str) -> Result<Value, String> {
        if op == "status" {
            return Ok(json!({"held_by_caller":self.owner.as_ref()==Some(owner),
                "occupied":self.owner.is_some(),"busy":self.busy,"paused":self.paused,
                "outcome_unknown":self.outcome_unknown}));
        }
        if self.busy || self.owner.as_ref().is_some_and(|active| active != owner) {
            return Err("desktop session busy or held by another project or role".into());
        }
        match op {
            "acquire" => {
                if self.paused {
                    return Err("desktop is paused by owner".into());
                }
                self.owner = Some(owner.clone());
                Ok(json!({"acquired":true,"requires_observation":true}))
            }
            "release" => {
                let paused = self.paused;
                let unknown = self.outcome_unknown;
                *self = State::default();
                self.paused = paused;
                self.outcome_unknown = unknown;
                Ok(json!({"released":true}))
            }
            _ => Err("unsupported computer session operation".into()),
        }
    }

    fn claim_lane(&mut self, owner: &Owner, request: &LaneRequest) -> Result<(), String> {
        if self.busy {
            return Err("desktop operation still running".into());
        }
        if self.paused {
            return Err("desktop is paused by owner".into());
        }
        if self.owner.as_ref().is_some_and(|active| active != owner) {
            return Err("desktop is held by another project or role".into());
        }
        // Issue 12: browser and native observations are different authorities.
        // A missed match costs one fresh observation; a false match can spend an
        // unreviewed action against a different target. Fail closed across lanes.
        if !request.read
            && (self.outcome_unknown
                || (!request.bootstrap
                    && (request.executor != self.executor
                        || request.snapshot_id != self.snapshot
                        || self.snapshot.is_none())))
        {
            return Err("fresh desktop observation required; never replay the prior action".into());
        }
        self.owner = Some(owner.clone());
        self.busy = true;
        if !request.read || request.executor != self.executor {
            self.snapshot = None;
        }
        self.executor = request.executor;
        Ok(())
    }
    #[cfg(test)]
    fn completed(&mut self, request: &NativeRequest, reply: &NativeReply) {
        self.completed_lane(&LaneRequest::from(request), reply);
    }

    fn completed_lane(&mut self, request: &LaneRequest, reply: &NativeReply) {
        self.busy = false;
        self.active = None;
        self.active_backend = None;
        if !reply.ok {
            // Ticket 07: permission revocation, a locked session and lost target
            // require owner reconciliation. Human input itself never pauses us.
            self.paused = true;
            self.snapshot = None;
            self.outcome_unknown |= reply.outcome_unknown;
            return;
        }
        if reply.ok && request.observation {
            self.snapshot = reply
                .result
                .as_ref()
                .and_then(|v| v["snapshot_id"].as_str())
                .map(str::to_owned);
            self.outcome_unknown = self.snapshot.is_none();
        } else if !request.read {
            self.outcome_unknown = reply.outcome_unknown;
        }
    }
}

impl Controller {
    fn run(
        self: &Arc<Self>,
        owner: Owner,
        request: &NativeRequest,
        timeout: Duration,
    ) -> Result<NativeReply, ToolError> {
        let serialized =
            serde_json::to_string(request).map_err(|e| ToolError::BadInput(e.to_string()))?;
        self.run_with_backend(
            owner,
            LaneRequest::from(request),
            &serialized,
            timeout,
            Arc::clone(&self.backend),
        )
    }

    fn run_with_backend(
        self: &Arc<Self>,
        owner: Owner,
        request: LaneRequest,
        serialized: &str,
        timeout: Duration,
        backend: Arc<dyn Backend>,
    ) -> Result<NativeReply, ToolError> {
        let started = Instant::now();
        if timeout.is_zero() {
            return Err(ToolError::NotExecuted(
                "operation deadline expired before dispatch".into(),
            ));
        }
        if serialized.len() > 65_536 {
            return Err(ToolError::BadInput("desktop request exceeds limit".into()));
        }
        // Ticket 07 owner review: pause and start share one linearization lock.
        // The start ABI only enqueues native work, never runs AppKit synchronously.
        // A pause winning this lock prevents dispatch; a start winning it exposes
        // its operation ID before pause can cancel. No claim/start gap remains.
        let operation = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| ToolError::NotExecuted("desktop state unavailable".into()))?;
            let previous_owner = state.owner.clone();
            state
                .claim_lane(&owner, &request)
                .map_err(ToolError::NotExecuted)?;
            if backend.needs_system_lease() && state.system_lease.is_none() {
                match lease::acquire() {
                    Ok(file) => state.system_lease = Some(file),
                    Err(error) => {
                        state.busy = false;
                        state.owner = previous_owner;
                        return Err(ToolError::NotExecuted(error));
                    }
                }
            }
            match backend.start(serialized) {
                Ok(id) if id != 0 => {
                    state.active = Some(id);
                    state.active_backend = Some(Arc::clone(&backend));
                    id
                }
                result => {
                    state.busy = false;
                    return Err(ToolError::NotExecuted(result.err().unwrap_or_else(|| {
                        "native executor busy or invalid request".into()
                    })));
                }
            }
        };
        loop {
            match backend.poll(operation) {
                Ok(Some(reply)) => {
                    self.state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .completed_lane(&request, &reply);
                    crate::diag::note(
                        crate::diag::CLASS_JUDGE,
                        !reply.ok,
                        None,
                        Some(&owner.agent),
                        None,
                        None,
                        "computer_use",
                        if reply.ok {
                            "native_completed"
                        } else {
                            "native_failed"
                        },
                        started,
                    );
                    return Ok(reply);
                }
                Ok(None) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                other => {
                    let _ = backend.cancel(operation);
                    {
                        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.outcome_unknown = true;
                        state.snapshot = None;
                        state.paused = true;
                    }
                    // A timeout does not terminate noncooperative AX. Drain in
                    // background while retaining the global lane; no second role
                    // can dispatch until native completion is actually observed.
                    let pending = Arc::clone(self);
                    let request = request.clone();
                    std::thread::spawn(move || loop {
                        if let Ok(Some(reply)) = backend.poll(operation) {
                            let mut state = pending.state.lock().unwrap_or_else(|e| e.into_inner());
                            state.completed_lane(&request, &reply);
                            state.outcome_unknown = true;
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    });
                    return Err(ToolError::OutcomeUnknown(other.err().unwrap_or_else(
                        || "native desktop operation timed out; paused until reconciled".into(),
                    )));
                }
            }
        }
    }
}

/// Host project-leave boundary. Retain the old root while its executor drains;
/// cancellation is a request, never evidence that an operation already stopped.
pub fn close_project(root: &Path) -> Result<(), String> {
    // The shell retained the canonical root when it opened the project. It may
    // have been removed since then; leaving must not reopen that filesystem path.
    let project = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    controller()
        .state
        .lock()
        .map_err(|_| "desktop state unavailable")?
        .close_project(&project)
}

fn project_key(root: &Path) -> Result<PathBuf, String> {
    root.canonicalize().map_err(|e| e.to_string())
}

/// Ticket 07: queued UI commands carry the workspace which authorized them.
/// A stale project costs a retry; accepting it can enable another project's
/// screen transmission. Compare the host canonical path without opening any
/// client-supplied path, and fail closed while the shell holds its project lock.
pub fn verify_project(root: &Path, expected: &str) -> Result<(), String> {
    let started = Instant::now();
    let matches = project_key(root)?.display().to_string() == expected;
    crate::diag::note(
        if matches {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !matches,
        Some(crate::PROJECT_ID),
        Some("owner"),
        None,
        None,
        "computer_project_binding",
        if matches {
            "matched"
        } else {
            "project_changed"
        },
        started,
    );
    if matches {
        Ok(())
    } else {
        Err("workspace changed; retry this computer control in the current project".into())
    }
}

pub fn enabled(db: &Db) -> Result<bool, String> {
    db.conn().query_row("SELECT json_extract(payload,'$.enabled')=1 AND json_extract(payload,'$.actor')='owner' AND json_extract(payload,'$.screen_to_selected_model')=1 FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')='computer_control' AND json_extract(payload,'$.action') IN ('enable','disable') ORDER BY id DESC LIMIT 1",
        [crate::PROJECT_ID], |row| row.get::<_, bool>(0)).optional().map(|v| v.unwrap_or(false)).map_err(|e| e.to_string())
}

pub fn status(db: &Db, root: &Path) -> Result<DesktopStatus, String> {
    let state = controller()
        .state
        .lock()
        .map_err(|_| "desktop state unavailable")?;
    Ok(DesktopStatus {
        project_root: project_key(root)?.display().to_string(),
        enabled: enabled(db)?,
        active_project: state
            .owner
            .as_ref()
            .map(|v| v.project.display().to_string()),
        active_agent: state.owner.as_ref().map(|v| v.agent.clone()),
        busy: state.busy,
        paused: state.paused,
        outcome_unknown: state.outcome_unknown,
        screenshot_count: screenshot_files(root)?.len().try_into().unwrap_or(u32::MAX),
        capture_preparation: Some(super::capture_preparation(false)),
    })
}

/// Owner-only local evidence viewer. This does not grant a model file access.
pub fn screenshot(root: &Path, name: &str) -> Result<DesktopScreenshot, String> {
    use std::io::Read as _;
    if !name
        .strip_suffix(".png")
        .is_some_and(|stem| stem.len() == 32 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("invalid screenshot name".into());
    }
    let path = screenshot_files(root)?
        .into_iter()
        .find(|path| path.file_name().is_some_and(|file| file == name))
        .ok_or("screenshot was cleared or is unavailable")?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|_| "screenshot was cleared or is unavailable")?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 5_000_000 {
        return Err("invalid screenshot file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err("screenshot aliases are not allowed".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(5_000_001)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 5_000_000 || crate::tools::sniff_image(&bytes) != Some("image/png") {
        return Err("invalid screenshot content".into());
    }
    Ok(DesktopScreenshot {
        name: name.into(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
    })
}

pub fn control(db: &Db, root: &Path, action: DesktopControl) -> Result<DesktopStatus, String> {
    let started = Instant::now();
    let result = control_inner(db, root, action);
    let label = serde_json::to_value(action).unwrap_or(Value::Null);
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        Some("owner"),
        None,
        None,
        "computer_control",
        match result.as_ref().err().map(String::as_str) {
            Some("enable computer access before preparing screenshots") => "prepare_not_enabled",
            Some("wait for the current operation before checking capture readiness") => {
                "prepare_busy"
            }
            Some("desktop is held by another project") => "foreign_project",
            Some(_) => "control_failed",
            None => label.as_str().unwrap_or("invalid"),
        },
        started,
    );
    result
}

fn control_inner(db: &Db, root: &Path, action: DesktopControl) -> Result<DesktopStatus, String> {
    let project = project_key(root)?;
    if matches!(action, DesktopControl::PrepareCapture) && !enabled(db)? {
        return Err("enable computer access before preparing screenshots".into());
    }
    if matches!(action, DesktopControl::Disable) {
        super::preview::stop()?;
    }
    let started = Instant::now();
    {
        let mut state = controller()
            .state
            .lock()
            .map_err(|_| "desktop state unavailable")?;
        if let Some(id) = state.control(&project, action)? {
            // Pause/revoke must cancel the executor which owns this operation,
            // not whichever adapter happened to be the original native default.
            state.cancel_active(id)?;
        }
        if matches!(action, DesktopControl::ClearScreenshots) {
            EVIDENCE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            for file in screenshot_files(root)? {
                std::fs::remove_file(file).map_err(|e| e.to_string())?;
            }
        }
    }
    let label = serde_json::to_value(action).map_err(|e| e.to_string())?;
    db.append_event(crate::PROJECT_ID, EventKind::System,
        json!({"kind":"computer_control","actor":"owner","action":label,"enabled":matches!(action,DesktopControl::Enable),"screen_to_selected_model":matches!(action,DesktopControl::Enable)}), None, None).map_err(|e| e.to_string())?;
    if matches!(
        action,
        DesktopControl::Enable | DesktopControl::Resume | DesktopControl::PrepareCapture
    ) && enabled(db)?
    {
        // Owner QA 2026-10-06: prewarming only prepares SDK capability. No lane
        // claim, screenshot or task replay; paused/unknown delivery stays intact.
        let preparation = super::capture_preparation(true);
        crate::diag::note(
            if matches!(
                preparation,
                super::CapturePreparationState::Failed
                    | super::CapturePreparationState::Unavailable
            ) {
                crate::diag::CLASS_REJECT
            } else {
                crate::diag::CLASS_HOST
            },
            matches!(
                preparation,
                super::CapturePreparationState::Failed
                    | super::CapturePreparationState::Unavailable
            ),
            Some(crate::PROJECT_ID),
            Some("owner"),
            None,
            None,
            "computer_use",
            match preparation {
                super::CapturePreparationState::Failed => "capture_preparation_failed",
                super::CapturePreparationState::Unavailable => "capture_preparation_unavailable",
                _ => "capture_preparation_started",
            },
            started,
        );
    }
    status(db, root)
}

/// Explicit lifecycle for the shared physical desktop lane. No app is launched
/// or closed, and acquiring control does not itself capture the screen.
pub(crate) fn session(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    op: &str,
) -> Result<Value, ToolError> {
    let started = Instant::now();
    let result = (|| {
        if ctx.subagent.is_some() {
            return Err(ToolError::NotExecuted(
                "subagents cannot manage desktop sessions".into(),
            ));
        }
        if op == "acquire" && !enabled(db).map_err(ToolError::NotExecuted)? {
            return Err(ToolError::NotExecuted(
                "owner must enable Computer Use and screenshot sharing for this project".into(),
            ));
        }
        let owner = Owner {
            project: project_key(&ctx.repo_root).map_err(ToolError::NotExecuted)?,
            agent: ctx.agent_id.clone(),
        };
        let mut state = controller()
            .state
            .lock()
            .map_err(|_| ToolError::NotExecuted("desktop state unavailable".into()))?;
        // Acquire the process-wide lock only after the same project/role checks;
        // restore the prior owner if another Hexagon process owns that lock.
        let previous = state.owner.clone();
        let value = state.session(&owner, op).map_err(ToolError::NotExecuted)?;
        if op == "acquire" && state.system_lease.is_none() {
            match lease::acquire() {
                Ok(file) => state.system_lease = Some(file),
                Err(error) => {
                    state.owner = previous;
                    return Err(ToolError::NotExecuted(error));
                }
            }
        }
        Ok(value)
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        ctx.action_key.as_deref(),
        "computer_session",
        if result.is_err() {
            "session_refused"
        } else {
            op
        },
        started,
    );
    result
}

pub fn execute(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    request: NativeRequest,
) -> Result<Value, ToolError> {
    let started = Instant::now();
    let result = execute_inner(db, ctx, request);
    let code = match &result {
        Ok(value) if value["requires_observation"] == true => "dispatched_requires_observation",
        Ok(_) => "completed",
        Err(ToolError::OutcomeUnknown(_)) => "outcome_unknown_paused",
        Err(ToolError::BadInput(_)) => "invalid_request",
        Err(_) => "not_executed_or_unavailable",
    };
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        ctx.action_key.as_deref(),
        "computer_use",
        code,
        started,
    );
    result
}

fn execute_inner(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    request: NativeRequest,
) -> Result<Value, ToolError> {
    request.validate()?;
    if !enabled(db).map_err(ToolError::NotExecuted)? {
        return Err(ToolError::NotExecuted(
            "owner must enable Computer Use and screenshot sharing for this project".into(),
        ));
    }
    if ctx.subagent.is_some() {
        return Err(ToolError::NotExecuted(
            "subagents cannot acquire the physical desktop".into(),
        ));
    }
    let owner = Owner {
        project: project_key(&ctx.repo_root).map_err(ToolError::NotExecuted)?,
        agent: ctx.agent_id.clone(),
    };
    if request.op == "release" {
        return session(db, ctx, "release");
    }
    if request.is_observation() && !ctx.caps.contains("vision") {
        return Err(ToolError::NotExecuted(
            "selected model requires vision capability for desktop screenshots".into(),
        ));
    }
    let timeout = ctx
        .deadline
        .map(|d| d.saturating_duration_since(Instant::now()))
        .unwrap_or(Duration::from_secs(120))
        .min(Duration::from_secs(120));
    let evidence_generation = EVIDENCE_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    let reply = controller().run(owner, &request, timeout)?;
    finish_reply(ctx, reply, evidence_generation)
}

/// Browser operations share the exact desktop owner lane, revocation gate and
/// screenshot persistence policy. The backend only queues on start; blocking
/// Node I/O here would prevent the owner's pause from acquiring the state lock.
pub(crate) fn execute_browser(
    db: &Db,
    ctx: &crate::tools::ToolContext,
    request: BrowserLaneRequest,
    backend: Arc<dyn Backend>,
) -> Result<Value, ToolError> {
    let started = Instant::now();
    let result = (|| {
        if !enabled(db).map_err(ToolError::NotExecuted)? {
            return Err(ToolError::NotExecuted(
                "owner must enable Computer Use and screenshot sharing for this project".into(),
            ));
        }
        if ctx.subagent.is_some() {
            return Err(ToolError::NotExecuted(
                "subagents cannot acquire the browser lane".into(),
            ));
        }
        if request.observation && !ctx.caps.contains("vision") {
            return Err(ToolError::NotExecuted(
                "selected model requires vision capability for browser screenshots".into(),
            ));
        }
        let owner = Owner {
            project: project_key(&ctx.repo_root).map_err(ToolError::NotExecuted)?,
            agent: ctx.agent_id.clone(),
        };
        let timeout = ctx
            .deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(120))
            .min(Duration::from_secs(120));
        let evidence_generation = EVIDENCE_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
        let reply = controller().run_with_backend(
            owner,
            LaneRequest {
                executor: Executor::Browser,
                read: request.read,
                observation: request.observation,
                bootstrap: request.bootstrap,
                snapshot_id: request.snapshot_id,
            },
            &request.serialized,
            timeout,
            backend,
        )?;
        finish_reply(ctx, reply, evidence_generation)
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        ctx.action_key.as_deref(),
        "browser_use",
        match &result {
            Ok(_) => "completed",
            Err(ToolError::OutcomeUnknown(_)) => "outcome_unknown_paused",
            Err(_) => "not_executed_or_unavailable",
        },
        started,
    );
    result
}

fn finish_reply(
    ctx: &crate::tools::ToolContext,
    reply: NativeReply,
    evidence_generation: u64,
) -> Result<Value, ToolError> {
    if !reply.ok {
        let reason = reply
            .error
            .unwrap_or_else(|| "native desktop action failed".into());
        return Err(if reply.outcome_unknown {
            ToolError::OutcomeUnknown(reason)
        } else {
            ToolError::NotExecuted(reason)
        });
    }
    let mut value = serde_json::to_value(reply).map_err(|e| ToolError::Exec(e.to_string()))?;
    if let Some(image) = value["result"]
        .as_object_mut()
        .and_then(|map| map.remove("image_base64"))
    {
        let image = image
            .as_str()
            .ok_or_else(|| ToolError::Exec("invalid screenshot encoding".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image)
            .map_err(|_| ToolError::Exec("invalid screenshot encoding".into()))?;
        if bytes.len() > 5_000_000 || crate::tools::sniff_image(&bytes) != Some("image/png") {
            return Err(ToolError::Exec("invalid screenshot content".into()));
        }
        let path = persist_capture(&ctx.repo_root, &bytes, evidence_generation)?;
        value["result"]["screenshot_path"] = json!(path);
        value["image"] = json!({"media_type":"image/png","data":image});
    }
    value["requires_observation"] = json!(value["outcome_unknown"] == true);
    Ok(value)
}

/// Persist the evidence path, never a second full screenshot in events/actions.
/// Immediate tool output retains its image block for the selected vision model.
pub(crate) fn persisted_result(tool: &str, value: &Value) -> Value {
    let mut result = value.clone();
    if matches!(
        tool,
        "computer_observe" | "computer_action" | "browser_observe" | "browser_action"
    ) && result.get("image").is_some()
    {
        result["image"] = json!({"media_type":"image/png","omitted":"local screenshot; observe again for fresh pixels"});
    }
    result
}

fn screenshot_dir(root: &Path, create: bool) -> Result<PathBuf, String> {
    let mut dir = project_key(root)?;
    for component in [".hexagon", "computer-use", "screenshots"] {
        dir.push(component);
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                return Err("unsafe screenshot storage path".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                        .map_err(|e| e.to_string())?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(dir),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(dir)
}

fn screenshot_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = screenshot_dir(root, false)?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|entry| {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let valid = name.strip_suffix(".png").is_some_and(|name| {
                name.len() == 32 && name.bytes().all(|c| c.is_ascii_hexdigit())
            });
            Ok(
                (valid && entry.file_type().map_err(|e| e.to_string())?.is_file())
                    .then(|| entry.path()),
            )
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|entries| entries.into_iter().flatten().collect())
}

/// Selection captures share the screenshot clear epoch with native observations.
pub(crate) fn capture_generation() -> u64 {
    EVIDENCE_GENERATION.load(std::sync::atomic::Ordering::SeqCst)
}
pub(crate) fn persist_browser_capture(
    root: &Path,
    bytes: &[u8],
    generation: u64,
) -> Result<String, String> {
    persist_capture(root, bytes, generation).map_err(|error| error.to_string())
}

fn persist_capture(root: &Path, bytes: &[u8], generation: u64) -> Result<String, ToolError> {
    // Ticket 07: Clear may run after native completion but before PNG write.
    // Serialize persistence with Clear and reject captures from its old epoch.
    let _state = controller()
        .state
        .lock()
        .map_err(|_| ToolError::Exec("desktop state unavailable".into()))?;
    if EVIDENCE_GENERATION.load(std::sync::atomic::Ordering::SeqCst) != generation {
        return Err(ToolError::NotExecuted(
            "screenshot evidence was cleared; observe again".into(),
        ));
    }
    save_screenshot(root, bytes).map_err(ToolError::Exec)
}

fn save_screenshot(root: &Path, bytes: &[u8]) -> Result<String, String> {
    use std::io::Write as _;
    let dir = screenshot_dir(root, true)?;
    // Ticket 07: local evidence must not be swept into a generated project commit.
    let mut ignore = std::fs::OpenOptions::new();
    ignore.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        ignore.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    match ignore.open(dir.join(".gitignore")) {
        Ok(mut file) => file.write_all(b"*\n").map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(dir.join(".gitignore"))
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
                || std::fs::read(dir.join(".gitignore")).map_err(|e| e.to_string())? != b"*\n"
            {
                return Err("unsafe screenshot ignore file".into());
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let name: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{name}.png"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests;

/// Owner-created browser sessions/focus share the input lane without assigning
/// a permanent role lease to the UI. Issue 12: a separate owner launch channel
/// once allowed a window activation to race a native click. Use the same claim
/// lock, preserve an existing same-project role, and never implicitly resume.
pub(crate) fn owner_browser_operation(
    root: &Path,
    request: BrowserLaneRequest,
    backend: Arc<dyn Backend>,
) -> Result<Value, String> {
    let project = project_key(root)?;
    let (owner, temporary) = {
        let state = controller()
            .state
            .lock()
            .map_err(|_| "desktop state unavailable")?;
        if state
            .owner
            .as_ref()
            .is_some_and(|owner| owner.project != project)
        {
            return Err("computer control belongs to another project".into());
        }
        (
            state.owner.clone().unwrap_or(Owner {
                project,
                agent: "owner".into(),
            }),
            state.owner.is_none(),
        )
    };
    let result = controller()
        .run_with_backend(
            owner.clone(),
            LaneRequest {
                executor: Executor::Browser,
                read: request.read,
                observation: request.observation,
                bootstrap: request.bootstrap,
                snapshot_id: request.snapshot_id,
            },
            &request.serialized,
            Duration::from_secs(60),
            backend,
        )
        .map_err(|e| e.to_string());
    if temporary {
        let mut state = controller()
            .state
            .lock()
            .map_err(|_| "desktop state unavailable")?;
        if !state.busy && state.owner.as_ref() == Some(&owner) {
            state.owner = None;
            state.system_lease = None;
            state.snapshot = None;
        }
    }
    let reply = result?;
    if !reply.ok {
        return Err(reply
            .error
            .unwrap_or_else(|| "owner browser operation failed".into()));
    }
    reply
        .result
        .ok_or_else(|| "browser reply omitted result".into())
}

/// Owner foregrounding reserves the same input lane, but is allowed while role
/// takeover is paused. No mutex remains held during platform/browser I/O.
pub(crate) struct OwnerFocusGuard {
    controller: Arc<Controller>,
}
impl Drop for OwnerFocusGuard {
    fn drop(&mut self) {
        let mut state = self
            .controller
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.busy = false;
        if state.owner.is_none() {
            state.system_lease = None;
        }
    }
}
impl State {
    fn owner_focus_allowed(&self, project: &Path) -> Result<(), String> {
        // Issue 14 / 2026-10-02: false negative costs an owner retry; false
        // positive retargets a pending native click. Paused is intentionally
        // allowed for human inspection, busy/foreign ownership fail closed.
        if self.busy {
            return Err("wait for the current computer operation before focusing".into());
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| owner.project != project)
        {
            return Err("computer control belongs to another project".into());
        }
        Ok(())
    }
}
impl Controller {
    fn reserve_owner_focus(self: &Arc<Self>, project: &Path) -> Result<OwnerFocusGuard, String> {
        {
            let mut state = self.state.lock().map_err(|_| "desktop state unavailable")?;
            state.owner_focus_allowed(project)?;
            if self.backend.needs_system_lease() && state.system_lease.is_none() {
                state.system_lease = Some(lease::acquire()?);
            }
            state.snapshot = None;
            state.busy = true;
        }
        Ok(OwnerFocusGuard {
            controller: Arc::clone(self),
        })
    }
}
pub(crate) fn reserve_owner_focus(root: &Path) -> Result<OwnerFocusGuard, String> {
    let started = Instant::now();
    let result = project_key(root).and_then(|project| controller().reserve_owner_focus(&project));
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        Some("owner"),
        None,
        None,
        "owner_focus",
        if result.is_ok() {
            "lane_reserved"
        } else {
            "busy_or_foreign_target"
        },
        started,
    );
    result
}
