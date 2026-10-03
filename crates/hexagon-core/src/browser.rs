//! Managed and owner-connected extension browser sessions. Issue 12/19, 2026-10-02:
//! the model never supplies an executable, script, profile or connection target.
use crate::desktop::actions::{self, Backend, BrowserLaneRequest, NativeReply};
use crate::tools::{ToolContext, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum BrowserMode {
    Managed,
    Extension,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct BrowserLabels {
    pub element: String,
    pub region: String,
    pub done: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct BrowserSession {
    pub project_root: String,
    pub session_id: String,
    pub mode: BrowserMode,
    pub tab_id: String,
    #[ts(type = "number")]
    pub navigation_generation: u64,
    pub url: String,
    pub title: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct BrowserPreview {
    pub session_id: String,
    pub tab_id: String,
    #[ts(type = "number")]
    pub navigation_generation: u64,
    pub data_url: String,
    #[ts(type = "number")]
    pub captured_at: u64,
}

static RUNTIME: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);
struct Session {
    info: Arc<Mutex<BrowserSession>>,
    backend: Arc<Worker>,
}
struct Worker {
    tx: mpsc::SyncSender<String>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, Option<NativeReply>>>,
    seq: AtomicU64,
    busy: AtomicBool,
    local_busy: AtomicBool,
    alive: AtomicBool,
}

struct LocalRequestGuard<'a>(&'a AtomicBool);
impl Drop for LocalRequestGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Called by the shell at startup with trusted absolute runtime locations.
pub fn initialize(node: &Path, worker: &Path) -> Result<(), String> {
    let paths = (
        node.canonicalize().map_err(|e| e.to_string())?,
        worker.canonicalize().map_err(|e| e.to_string())?,
    );
    RUNTIME
        .set(paths)
        .map_err(|_| "browser runtime already initialized".into())
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}
fn root_key(root: &Path) -> Result<String, String> {
    Ok(root
        .canonicalize()
        .map_err(|e| e.to_string())?
        .display()
        .to_string())
}
fn bound_session(root: &Path, id: &str) -> Result<Arc<Session>, String> {
    let key = root_key(root)?;
    let slot = SESSION.lock().map_err(|_| "browser session unavailable")?;
    let session = slot.as_ref().ok_or("open a project browser first")?;
    let info = session
        .info
        .lock()
        .map_err(|_| "browser state unavailable")?;
    if info.project_root != key || info.session_id != id {
        return Err("browser project or session changed".into());
    }
    if !session.backend.alive.load(Ordering::SeqCst) {
        return Err("browser disconnected; explicitly open a new session".into());
    }
    Ok(Arc::clone(session))
}

pub fn status(root: &Path) -> Result<Option<BrowserSession>, String> {
    let key = root_key(root)?;
    let slot = SESSION.lock().map_err(|_| "browser session unavailable")?;
    let Some(session) = slot.as_ref() else {
        return Ok(None);
    };
    let mut info = session
        .info
        .lock()
        .map_err(|_| "browser state unavailable")?
        .clone();
    if info.project_root != key {
        return Ok(None);
    }
    info.connected &= session.backend.alive.load(Ordering::SeqCst);
    Ok(Some(info))
}

impl Worker {
    fn launch(info: Arc<Mutex<BrowserSession>>) -> Result<Arc<Self>, String> {
        let (node, script) = RUNTIME.get().ok_or("browser runtime unavailable")?;
        let mut command = Command::new(node);
        command
            .arg(script)
            .current_dir(script.parent().ok_or("invalid runtime path")?)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let bundled_browsers = script
            .parent()
            .ok_or("invalid browser path")?
            .join("browsers");
        if bundled_browsers.is_dir() {
            command.env("PLAYWRIGHT_BROWSERS_PATH", bundled_browsers);
        }
        for name in ["HOME", "PATH", "TMPDIR", "DISPLAY", "WAYLAND_DISPLAY"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        Self::spawn(command, info)
    }

    fn spawn(mut command: Command, info: Arc<Mutex<BrowserSession>>) -> Result<Arc<Self>, String> {
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("browser stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("browser stdout unavailable")?;
        let (tx, rx) = mpsc::sync_channel::<String>(2);
        let worker = Arc::new(Self {
            tx,
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            busy: AtomicBool::new(false),
            local_busy: AtomicBool::new(false),
            alive: AtomicBool::new(true),
        });
        let writer = Arc::downgrade(&worker);
        std::thread::spawn(move || {
            while let Ok(line) = rx.recv() {
                // One bounded queued role action may wait behind a local frame.
                // Start remains nonblocking under the shared controller mutex.
                let Some(worker) = writer.upgrade() else {
                    break;
                };
                while worker.busy.swap(true, Ordering::SeqCst) {
                    if !worker.alive.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                if !worker.alive.load(Ordering::SeqCst) {
                    return;
                }
                drop(worker);
                if writeln!(stdin, "{line}")
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    if let Some(worker) = writer.upgrade() {
                        worker.disconnected();
                    }
                    break;
                }
            }
        });
        let reader = Arc::downgrade(&worker);
        std::thread::spawn(move || {
            let mut reader_stream = BufReader::new(stdout);
            loop {
                // Bounded JSONL: no unbounded allocation from a compromised page.
                let mut bytes = Vec::new();
                let read = std::io::Read::take(&mut reader_stream, 8_000_001)
                    .read_until(b'\n', &mut bytes);
                if read.is_err()
                    || bytes.is_empty()
                    || bytes.len() > 8_000_000
                    || bytes.last() != Some(&b'\n')
                {
                    break;
                }
                let Some(worker) = reader.upgrade() else {
                    break;
                };
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    break;
                };
                let Some(id) = value["id"].as_u64() else {
                    break;
                };
                let Ok(reply) = serde_json::from_value::<NativeReply>(value) else {
                    break;
                };
                if reply.protocol_version != 1 {
                    break;
                }
                if let Some(result) = reply.result.as_ref() {
                    let mut state = info.lock().unwrap_or_else(|e| e.into_inner());
                    if result["session_id"].as_str() == Some(state.session_id.as_str()) {
                        if let Some(tab) = result["tab_id"].as_str() {
                            state.tab_id = tab.to_owned();
                        }
                        if let Some(generation) = result["navigation_generation"].as_u64() {
                            state.navigation_generation = generation;
                        }
                        if let Some(url) = result["url"].as_str() {
                            state.url = url.chars().take(2048).collect();
                        }
                        if let Some(title) = result["title"].as_str() {
                            state.title = title.chars().take(256).collect();
                        }
                        state.connected = result["connected"].as_bool().unwrap_or(state.connected);
                    }
                }
                let mut pending = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(slot) = pending.get_mut(&id) {
                    *slot = Some(reply);
                    worker.busy.store(false, Ordering::SeqCst);
                }
            }
            if let Some(worker) = reader.upgrade() {
                worker.disconnected();
            }
        });
        Ok(worker)
    }
    fn disconnected(&self) {
        self.alive.store(false, Ordering::SeqCst);
        for slot in self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
        {
            if slot.is_none() {
                *slot = Some(NativeReply {
                    protocol_version: 1,
                    ok: false,
                    result: None,
                    applications: None,
                    error: Some(
                        "browser worker disconnected; never replay the previous action".into(),
                    ),
                    outcome_unknown: true,
                    cancelled: true,
                    outcome: None,
                });
            }
        }
        self.busy.store(false, Ordering::SeqCst);
    }
    fn request(&self, serialized: &str, timeout: Duration) -> Result<Value, String> {
        if self
            .local_busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("local browser request already in flight".into());
        }
        let _local_guard = LocalRequestGuard(&self.local_busy);
        if !self
            .pending
            .lock()
            .map_err(|_| "browser unavailable")?
            .is_empty()
        {
            return Err("local preview yields to queued role operation".into());
        }
        let id = self.start(serialized)?;
        let started = Instant::now();
        loop {
            if let Some(reply) = self.poll(id)? {
                return if reply.ok {
                    reply.result.ok_or("browser reply omitted result".into())
                } else {
                    Err(reply.error.unwrap_or("browser request failed".into()))
                };
            }
            if started.elapsed() >= timeout {
                self.cancel(id)?;
                return Err("browser request timed out; session disconnected".into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}
impl Backend for Worker {
    fn needs_system_lease(&self) -> bool {
        true
    }
    fn start(&self, serialized: &str) -> Result<u64, String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err("browser disconnected".into());
        }
        (|| {
            let mut request: Value = serde_json::from_str(serialized).map_err(|e| e.to_string())?;
            let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
            request["id"] = json!(id);
            let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
            if line.len() > 65_536 {
                return Err("browser request exceeds limit".into());
            }
            self.pending
                .lock()
                .map_err(|_| "browser unavailable")?
                .insert(id, None);
            if self.tx.try_send(line).is_err() {
                self.pending
                    .lock()
                    .map_err(|_| "browser unavailable")?
                    .remove(&id);
                return Err("browser transport closed or queue full".into());
            }
            Ok(id)
        })()
    }
    fn poll(&self, id: u64) -> Result<Option<NativeReply>, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "browser replies unavailable")?;
        if pending.get(&id).is_some_and(Option::is_some) {
            return Ok(pending.remove(&id).flatten());
        }
        Ok(None)
    }
    fn cancel(&self, _: u64) -> Result<(), String> {
        // Terminate only our worker connection; never close a personal tab.
        let mut child = self
            .child
            .lock()
            .map_err(|_| "browser process unavailable")?;
        let _ = child.kill();
        let _ = child.wait();
        self.disconnected();
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn envelope(session: &BrowserSession, method: &str, params: &Value, timeout: u64) -> String {
    json!({"id":1,"session_id":session.session_id,"method":method,"deadline":now_ms()+timeout,"params":params}).to_string()
}

fn create_session(root: &Path, mode: BrowserMode) -> Result<Arc<Session>, String> {
    let info = Arc::new(Mutex::new(BrowserSession {
        project_root: root_key(root)?,
        session_id: nonce()?,
        mode,
        tab_id: String::new(),
        navigation_generation: 0,
        url: String::new(),
        title: String::new(),
        connected: false,
    }));
    let session = {
        let mut slot = SESSION.lock().map_err(|_| "browser session unavailable")?;
        if slot
            .as_ref()
            .is_some_and(|s| s.backend.alive.load(Ordering::SeqCst))
        {
            return Err("detach the existing browser session first".into());
        }
        let backend = Worker::launch(Arc::clone(&info))?;
        let session = Arc::new(Session { info, backend });
        *slot = Some(Arc::clone(&session));
        session
    };
    Ok(session)
}

/// Issue19: the role uses the same consent/pause/lease gates as actions. Never
/// route agent lifecycle calls through the owner's temporary launch authority.
pub(crate) fn manage(
    db: &crate::db::Db,
    ctx: &ToolContext,
    op: &str,
    id: Option<&str>,
) -> Result<Value, ToolError> {
    let started = Instant::now();
    let result = manage_inner(db, ctx, op, id);
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
        "browser_session",
        if result.is_err() {
            "session_refused"
        } else {
            op
        },
        started,
    );
    result
}
fn manage_inner(
    db: &crate::db::Db,
    ctx: &ToolContext,
    op: &str,
    id: Option<&str>,
) -> Result<Value, ToolError> {
    if ctx.subagent.is_some() {
        return Err(ToolError::NotExecuted(
            "subagents cannot manage browser sessions".into(),
        ));
    }
    if op == "status" {
        return serde_json::to_value(status(&ctx.repo_root).map_err(ToolError::NotExecuted)?)
            .map_err(|e| ToolError::BadInput(e.to_string()));
    }
    let session = if op == "open" {
        create_session(&ctx.repo_root, BrowserMode::Managed).map_err(ToolError::NotExecuted)?
    } else {
        bound_session(
            &ctx.repo_root,
            id.ok_or_else(|| ToolError::BadInput("session_id required".into()))?,
        )
        .map_err(ToolError::NotExecuted)?
    };
    let info = session
        .info
        .lock()
        .map_err(|_| ToolError::NotExecuted("browser state unavailable".into()))?
        .clone();
    // Closing a personal browser must remain an owner action in Settings. A
    // false rejection costs one click; a false acceptance disrupts personal tabs.
    if op == "close" && info.mode != BrowserMode::Managed {
        return Err(ToolError::NotExecuted(
            "personal browser connection is managed by owner in Settings".into(),
        ));
    }
    let result = actions::execute_browser(
        db,
        ctx,
        BrowserLaneRequest {
            serialized: envelope(
                &info,
                if op == "open" { "open" } else { "detach" },
                &json!({"mode":"managed","tab_id":info.tab_id,"navigation_generation":info.navigation_generation}),
                60_000,
            ),
            read: false,
            observation: false,
            bootstrap: true,
            snapshot_id: None,
        },
        session.backend.clone(),
    );
    if (op == "open" && result.is_err()) || (op == "close" && result.is_ok()) {
        let _ = session.backend.cancel(0);
    }
    result
}

/// Owner entry only. Replaces no existing live session implicitly.
pub fn open(
    root: &Path,
    mode: BrowserMode,
    labels: BrowserLabels,
) -> Result<BrowserSession, String> {
    let session = create_session(root, mode)?;
    let before = session
        .info
        .lock()
        .map_err(|_| "browser state unavailable")?
        .clone();
    if let Err(error) = actions::owner_browser_operation(
        root,
        BrowserLaneRequest {
            serialized: envelope(
                &before,
                "open",
                &json!({"mode":mode,"labels":labels}),
                60_000,
            ),
            read: false,
            observation: false,
            bootstrap: true,
            snapshot_id: None,
        },
        session.backend.clone(),
    ) {
        let _ = session.backend.cancel(0);
        return Err(error);
    }
    let result = session
        .info
        .lock()
        .map_err(|_| "browser state unavailable")?
        .clone();
    Ok(result)
}

pub(crate) fn local_request(
    root: &Path,
    id: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    if !matches!(
        method,
        "metadata"
            | "preview"
            | "focus"
            | "detach"
            | "selection.start"
            | "selection.poll"
            | "selection.stop"
    ) {
        return Err("unsupported owner browser request".into());
    }
    let session = bound_session(root, id)?;
    if session.backend.busy.load(Ordering::SeqCst)
        || !session
            .backend
            .pending
            .lock()
            .map_err(|_| "browser unavailable")?
            .is_empty()
    {
        return Err("browser busy; local preview yields to role work".into());
    }
    let info = session
        .info
        .lock()
        .map_err(|_| "browser state unavailable")?
        .clone();
    let mut params = params.clone();
    params["tab_id"] = json!(info.tab_id);
    params["navigation_generation"] = json!(info.navigation_generation);
    session.backend.request(
        &envelope(&info, method, &params, 8000),
        Duration::from_secs(8),
    )
}
pub fn detach(root: &Path, id: &str) -> Result<(), String> {
    local_request(root, id, "detach", &json!({})).map(|_| ())
}
pub fn focus(root: &Path, id: &str) -> Result<(), String> {
    let _lane = actions::reserve_owner_focus(root)?;
    local_request(root, id, "focus", &json!({})).map(|_| ())
}

pub fn preview(root: &Path, id: &str) -> Result<BrowserPreview, String> {
    let value = local_request(root, id, "preview", &json!({}))?;
    let preview: BrowserPreview = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if preview.data_url.len() > 1_400_000
        || !preview.data_url.starts_with("data:image/jpeg;base64,")
    {
        return Err("invalid browser preview".into());
    }
    Ok(preview)
}

pub(crate) fn execute(
    db: &crate::db::Db,
    ctx: &ToolContext,
    method: &str,
    params: &Value,
) -> Result<Value, ToolError> {
    let id = params["session_id"]
        .as_str()
        .ok_or_else(|| ToolError::BadInput("session_id required".into()))?;
    let session = bound_session(&ctx.repo_root, id).map_err(ToolError::NotExecuted)?;
    let info = session
        .info
        .lock()
        .map_err(|_| ToolError::NotExecuted("browser state unavailable".into()))?
        .clone();
    let mut parameters = params.clone();
    parameters["tab_id"] = json!(info.tab_id);
    parameters["navigation_generation"] = json!(info.navigation_generation);
    let read = matches!(method, "observe" | "diagnostics");
    actions::execute_browser(
        db,
        ctx,
        BrowserLaneRequest {
            serialized: envelope(&info, method, &parameters, 30_000),
            read,
            observation: method == "observe",
            bootstrap: false,
            snapshot_id: params["snapshot_id"].as_str().map(str::to_owned),
        },
        session.backend.clone(),
    )
}

/// Called when the shell leaves a project. Termination disconnects only our
/// extension transport and causes pending role work to reconcile, never replay.
pub fn close_project(root: &Path) {
    let Ok(key) = root_key(root) else { return };
    let Ok(mut slot) = SESSION.lock() else { return };
    if slot
        .as_ref()
        .is_some_and(|s| s.info.lock().is_ok_and(|i| i.project_root == key))
    {
        if let Some(session) = slot.take() {
            let _ = session.backend.cancel(0);
        }
    }
}

pub fn refresh_status(root: &Path) -> Result<Option<BrowserSession>, String> {
    if let Some(info) = status(root)? {
        if info.connected {
            let _ = local_request(root, &info.session_id, "metadata", &json!({}));
        }
    }
    status(root)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn approved_action_queues_once_behind_preview_without_rejection() {
        let info = Arc::new(Mutex::new(BrowserSession {
            project_root: "/test".into(),
            session_id: "test".into(),
            mode: BrowserMode::Managed,
            tab_id: "tab".into(),
            navigation_generation: 0,
            url: String::new(),
            title: String::new(),
            connected: true,
        }));
        // Real pipes and a slow local frame prove the same broker used by the
        // production Node process waits; no second action or replay is emitted.
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("read first; sleep 0.1; printf '%s\\n' '{\"id\":1,\"protocol_version\":1,\"ok\":true,\"result\":{\"frame\":true},\"outcome_unknown\":false,\"cancelled\":false}'; read second; printf '%s\\n' '{\"id\":2,\"protocol_version\":1,\"ok\":true,\"result\":{\"dispatched\":true},\"outcome_unknown\":false,\"cancelled\":false}'; read third").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let worker = Worker::spawn(command, info).unwrap();
        let preview_worker = Arc::clone(&worker);
        let preview = std::thread::spawn(move || {
            preview_worker.request("{\"method\":\"preview\"}", Duration::from_secs(2))
        });
        let frame = 1;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker.busy.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(worker
            .request("{\"method\":\"preview\"}", Duration::from_secs(1))
            .is_err());
        let action = worker.start("{\"method\":\"click\"}").unwrap();
        assert_ne!(frame, action);
        let mut results = Vec::new();
        while results.is_empty() && Instant::now() < deadline {
            for id in [action] {
                if let Some(result) = worker.poll(id).unwrap() {
                    results.push((id, result));
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(preview.join().unwrap().unwrap()["frame"].as_bool().unwrap());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, action);
        assert!(results
            .iter()
            .all(|(_, reply)| reply.ok && !reply.outcome_unknown));
        assert_eq!(worker.seq.load(Ordering::SeqCst), 2);
        worker.cancel(0).unwrap();
    }
}
