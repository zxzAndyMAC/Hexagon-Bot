//! Owner extension 14: ephemeral owner-only capture; no model, trace or lease.
use crate::db::Db;
use rusqlite::OptionalExtension as _;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct NativePreviewTarget {
    pub project_root: String,
    pub window_id: u32,
    pub process_id: i32,
    pub bundle_id: Option<String>,
    pub title: String,
    pub snapshot_id: String,
    #[serde(skip)]
    #[ts(skip)]
    generation: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct NativePreviewFrame {
    pub project_root: String,
    pub snapshot_id: String,
    pub window_id: u32,
    pub process_id: i32,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}
// Issue14 cancellation review (2026-10-02): checking an atomic before native
// start left a gap where project Stop could win, then old focus enqueue itself
// into the new Swift generation. Serialize just validation + nonblocking ABI
// start/stop; never hold this lock during polling, capture, or AppKit dispatch.
struct PreviewDispatch {
    generation: AtomicU64,
    gate: Mutex<()>,
}
impl PreviewDispatch {
    const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            gate: Mutex::new(()),
        }
    }
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    #[cfg(any(target_os = "macos", test))]
    fn start<T>(&self, expected: u64, call: impl FnOnce() -> T) -> Result<T, String> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| "preview dispatch unavailable")?;
        if self.generation() != expected {
            return Err("preview stopped".into());
        }
        Ok(call())
    }
    fn stop(&self, expected: Option<u64>, call: impl FnOnce()) -> Result<(), String> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| "preview dispatch unavailable")?;
        // A timed-out old poll must not cancel the new project's capture/focus.
        if expected.is_some_and(|generation| self.generation() != generation) {
            return Ok(());
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        call();
        Ok(())
    }
}
static DISPATCH: PreviewDispatch = PreviewDispatch::new();

pub fn target(db: &Db, root: &Path) -> Result<Option<NativePreviewTarget>, String> {
    if !super::actions::enabled(db)? {
        return Ok(None);
    }
    let value: Option<String> = db.conn().query_row(
        "SELECT json_extract(result.payload,'$.result.output.result') FROM events result JOIN events called ON called.project_id=result.project_id AND called.kind='tool_called' AND json_extract(called.payload,'$.action_id')=json_extract(result.payload,'$.action_id') WHERE result.project_id=?1 AND result.kind='tool_result' AND json_extract(called.payload,'$.tool')='computer_observe' AND json_extract(result.payload,'$.ok')=1 AND json_extract(result.payload,'$.result.output.ok')=1 ORDER BY result.id DESC LIMIT 1",
        [crate::PROJECT_ID], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
    Ok(value
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| parse_target(root, &value)))
}
fn parse_target(root: &Path, value: &serde_json::Value) -> Option<NativePreviewTarget> {
    // False negative costs a new observation; false positive reveals an unrelated
    // window. Missing/screen-wide receipts never become preview authority.
    let window_id = u32::try_from(value["window_id"].as_u64()?).ok()?;
    let process_id = i32::try_from(value["process_id"].as_i64()?).ok()?;
    let snapshot_id = value["snapshot_id"].as_str()?;
    if window_id == 0 || process_id <= 0 || snapshot_id.is_empty() || snapshot_id.len() > 256 {
        return None;
    }
    Some(NativePreviewTarget {
        project_root: root.to_string_lossy().into_owned(),
        window_id,
        process_id,
        bundle_id: value["bundle_id"].as_str().map(str::to_owned),
        title: value["title"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect(),
        snapshot_id: snapshot_id.into(),
        generation: DISPATCH.generation(),
    })
}
pub fn require_target(
    db: &Db,
    root: &Path,
    expected_root: &str,
    window: u32,
    process: i32,
) -> Result<NativePreviewTarget, String> {
    let started = Instant::now();
    let result = super::actions::verify_project(root, expected_root).and_then(|()| {
        target(db, root)?
            .filter(|value| value.window_id == window && value.process_id == process)
            .ok_or_else(|| "preview target changed or consent revoked".into())
    });
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_HOST
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "desktop_preview",
        if result.is_ok() {
            "target_validated"
        } else {
            "target_rejected"
        },
        started,
    );
    result
}
pub fn stop() -> Result<(), String> {
    DISPATCH.stop(None, || {
        #[cfg(target_os = "macos")]
        if let Ok((_, _, _, stop)) = bindings() {
            unsafe { stop() };
        }
    })
}
#[derive(Deserialize)]
struct Reply {
    protocol_version: u32,
    ok: bool,
    error: Option<String>,
    data_url: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    window_id: Option<u32>,
    process_id: Option<i32>,
}
fn valid_frame(reply: &Reply, target: &NativePreviewTarget) -> bool {
    reply.protocol_version == 1
        && reply.ok
        && reply.window_id == Some(target.window_id)
        && reply.process_id == Some(target.process_id)
        && reply.width.is_some_and(|v| (1..=960).contains(&v))
        && reply.height.is_some_and(|v| (1..=960).contains(&v))
        && reply
            .data_url
            .as_ref()
            .is_some_and(|url| url.starts_with("data:image/jpeg;base64,") && url.len() <= 1_398_126)
}
pub fn frame(target: &NativePreviewTarget) -> Result<NativePreviewFrame, String> {
    ensure_current(target)?;
    let reply = execute(target, false)?;
    if !valid_frame(&reply, target) {
        return Err("invalid preview frame".into());
    }
    Ok(NativePreviewFrame {
        project_root: target.project_root.clone(),
        snapshot_id: target.snapshot_id.clone(),
        window_id: target.window_id,
        process_id: target.process_id,
        data_url: reply.data_url.unwrap_or_default(),
        width: reply.width.unwrap_or_default(),
        height: reply.height.unwrap_or_default(),
    })
}
fn ensure_current(target: &NativePreviewTarget) -> Result<(), String> {
    if target.generation != DISPATCH.generation() {
        return Err("preview stopped".into());
    }
    Ok(())
}
pub fn focus(target: &NativePreviewTarget) -> Result<(), String> {
    ensure_current(target)?;
    // Issue12/14 review: foregrounding an old observed app during another
    // native input can redirect its keystrokes. Owner focus shares the input
    // reservation (including while paused); passive preview frames do not.
    with_focus_reservation(
        Path::new(&target.project_root),
        super::actions::reserve_owner_focus,
        || execute(target, true).map(|_| ()),
    )
}
fn with_focus_reservation<Guard>(
    root: &Path,
    reserve: impl FnOnce(&Path) -> Result<Guard, String>,
    foreground: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _lane = reserve(root)?;
    foreground()
}
#[cfg(target_os = "macos")]
type StartFn = unsafe extern "C" fn(*const std::ffi::c_char) -> u64;
#[cfg(target_os = "macos")]
type PollFn = unsafe extern "C" fn(u64) -> *mut std::ffi::c_char;
#[cfg(target_os = "macos")]
type FreeFn = unsafe extern "C" fn(*mut std::ffi::c_char);
#[cfg(target_os = "macos")]
type StopFn = unsafe extern "C" fn();
#[cfg(target_os = "macos")]
fn bindings() -> Result<(StartFn, PollFn, FreeFn, StopFn), String> {
    let guard = super::LIBRARY
        .get()
        .ok_or("preview component unavailable")?
        .lock()
        .map_err(|_| "preview loader unavailable")?;
    let lib = guard.as_ref().ok_or("preview component unavailable")?;
    unsafe {
        Ok((
            *lib.get::<StartFn>(b"hexagon_preview_start_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<PollFn>(b"hexagon_preview_poll_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<FreeFn>(b"hexagon_computer_free_v1\0")
                .map_err(|e| e.to_string())?,
            *lib.get::<StopFn>(b"hexagon_preview_stop_v1\0")
                .map_err(|e| e.to_string())?,
        ))
    }
}
#[cfg(not(target_os = "macos"))]
fn execute(_: &NativePreviewTarget, _: bool) -> Result<Reply, String> {
    Err("macOS is required".into())
}
#[cfg(target_os = "macos")]
fn execute(target: &NativePreviewTarget, focus: bool) -> Result<Reply, String> {
    let generation = target.generation;
    if DISPATCH.generation() != generation {
        return Err("preview stopped".into());
    }
    let (start, poll, free, stop) = bindings()?;
    let request = std::ffi::CString::new(serde_json::json!({"snapshot_id":target.snapshot_id,"window_id":target.window_id,"process_id":target.process_id,"focus":focus}).to_string()).map_err(|_|"invalid preview target")?;
    let waiting = Instant::now();
    let id = loop {
        let id = DISPATCH.start(generation, || unsafe { start(request.as_ptr()) })?;
        if id != 0 {
            break id;
        }
        // Owner focus waits only for this independent local capture lane;
        // never acquire or block the agent/control lease. Hide/project swap
        // invalidates this ticket while waiting, before any later dispatch.
        if !focus || waiting.elapsed() >= Duration::from_secs(2) {
            return Err("preview busy or rate limited".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let began = Instant::now();
    loop {
        if DISPATCH.generation() != generation {
            // The winning Stop already cancelled this generation. Calling the
            // global ABI again here could cancel a newer project's request.
            return Err("preview stopped".into());
        }
        let raw = unsafe { poll(id) };
        if !raw.is_null() {
            let len = unsafe { libc::strnlen(raw, 1_500_001) };
            let decoded: Result<Reply, String> = if len > 1_500_000 {
                Err("preview reply too large".into())
            } else {
                serde_json::from_slice::<Reply>(unsafe {
                    std::slice::from_raw_parts(raw.cast::<u8>(), len)
                })
                .map_err(|_| "invalid preview reply".into())
            };
            unsafe { free(raw) };
            let reply = decoded?;
            if DISPATCH.generation() != generation {
                return Err("preview stopped".into());
            }
            if !reply.ok
                || reply.protocol_version != 1
                || reply.window_id != Some(target.window_id)
                || reply.process_id != Some(target.process_id)
            {
                return Err(reply.error.unwrap_or_else(|| "preview unavailable".into()));
            }
            return Ok(reply);
        }
        if began.elapsed() > Duration::from_secs(5) {
            DISPATCH.stop(Some(generation), || unsafe { stop() })?;
            return Err("preview timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn stop_between_validation_and_native_start_prevents_dispatch() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        let dispatch = Arc::new(PreviewDispatch::new());
        let started = Arc::new(AtomicBool::new(false));
        let (validated, ready) = mpsc::channel();
        let (resume, proceed) = mpsc::channel();
        let worker_dispatch = dispatch.clone();
        let worker_started = started.clone();
        let worker = std::thread::spawn(move || {
            let ticket = worker_dispatch.generation();
            validated.send(()).unwrap();
            // Reproduces the old gap (binding load / scheduling) after the
            // optimistic generation check but before the native start ABI.
            proceed.recv().unwrap();
            worker_dispatch.start(ticket, || worker_started.store(true, Ordering::SeqCst))
        });
        ready.recv().unwrap();
        dispatch.stop(None, || {}).unwrap();
        resume.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap_err(), "preview stopped");
        assert!(!started.load(Ordering::SeqCst));
    }

    #[test]
    fn start_and_stop_serialize_only_the_abi_and_old_timeout_cannot_cancel_new_work() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        let dispatch = Arc::new(PreviewDispatch::new());
        let old = dispatch.generation();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (entered, ready) = mpsc::channel();
        let (release, proceed) = mpsc::channel();
        let starter = dispatch.clone();
        let worker = std::thread::spawn(move || {
            starter.start(old, || {
                entered.send(()).unwrap();
                proceed.recv().unwrap();
                1
            })
        });
        ready.recv().unwrap();
        // A winning start holds the short gate until the ID has been returned.
        assert!(dispatch.gate.try_lock().is_err());
        let stopper = dispatch.clone();
        let stopped = cancelled.clone();
        let stop = std::thread::spawn(move || {
            stopper.stop(None, || stopped.store(true, Ordering::SeqCst))
        });
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), 1);
        stop.join().unwrap().unwrap();
        assert!(cancelled.load(Ordering::SeqCst));
        let current = dispatch.generation();
        dispatch.start(current, || {}).unwrap();
        // An old polling timeout must not issue a global native Stop now.
        dispatch
            .stop(Some(old), || panic!("cancelled a newer preview"))
            .unwrap();
        assert_eq!(dispatch.generation(), current);
    }

    #[test]
    fn native_focus_keeps_shared_reservation_until_dispatch_finishes_even_on_error() {
        use std::sync::{atomic::AtomicBool, Arc};
        struct Held(Arc<AtomicBool>);
        impl Drop for Held {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let held = Arc::new(AtomicBool::new(false));
        for fail in [false, true] {
            let result = with_focus_reservation(
                Path::new("/selected"),
                |root| {
                    assert_eq!(root, Path::new("/selected"));
                    held.store(true, Ordering::SeqCst);
                    Ok(Held(held.clone()))
                },
                || {
                    assert!(held.load(Ordering::SeqCst));
                    if fail {
                        Err("native focus failed".into())
                    } else {
                        Ok(())
                    }
                },
            );
            assert_eq!(result.is_err(), fail);
            assert!(!held.load(Ordering::SeqCst));
        }
    }
    #[test]
    fn native_focus_never_dispatches_when_an_input_already_owns_the_lane() {
        let mut called = false;
        let result = with_focus_reservation::<()>(
            Path::new("/selected"),
            |_| Err("input busy".into()),
            || {
                called = true;
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err(), "input busy");
        assert!(!called);
    }

    struct ObservedWindow;
    impl crate::tools::Tool for ObservedWindow {
        fn name(&self) -> &str {
            "computer_observe"
        }
        fn description(&self) -> &str {
            "test native observation boundary"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object"})
        }
        fn risk(&self) -> crate::tools::RiskClass {
            crate::tools::RiskClass::Read
        }
        fn exec(
            &self,
            _: &Db,
            input: &serde_json::Value,
            _: &crate::tools::ToolContext,
        ) -> Result<serde_json::Value, crate::tools::ToolError> {
            Ok(serde_json::json!({"ok":true,"result":input}))
        }
    }
    #[test]
    fn target_requires_current_project_consent_and_successful_window_observation() {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["QA"], None).unwrap();
        let registry = crate::tools::Registry::builtin();
        registry.register(ObservedWindow);
        // Regression event4635: real tool_result has no `tool` field. Use the
        // genuine dispatch seam; join its action_id to same-project tool_called.
        let result = registry.call(&wb.db, &wb.ctx_for("a0", None), "computer_observe",
            serde_json::json!({"snapshot_id":"live-receipt","window_id":12,"process_id":42,"bundle_id":"test.app","title":"test"})).unwrap();
        assert!(matches!(result, crate::tools::CallOutcome::Done(_)));
        assert!(target(&wb.db, dir.path()).unwrap().is_none());
        wb.db.append_event(crate::PROJECT_ID, crate::trace::EventKind::System,
            serde_json::json!({"kind":"computer_control","action":"enable","actor":"owner","enabled":true,"screen_to_selected_model":true}), None, None).unwrap();
        let selected = target(&wb.db, dir.path()).unwrap().unwrap();
        assert_eq!(selected.window_id, 12);
        assert!(require_target(&wb.db, dir.path(), &selected.project_root, 13, 42).is_err());
        assert!(require_target(&wb.db, dir.path(), "/unrelated-project", 12, 42).is_err());
        registry
            .call(
                &wb.db,
                &wb.ctx_for("a0", None),
                "computer_observe",
                serde_json::json!({"snapshot_id":"screen","scope":"screen"}),
            )
            .unwrap();
        // A screen-wide result must not fall back to a previous window receipt.
        assert!(target(&wb.db, dir.path()).unwrap().is_none());
        wb.db.append_event(crate::PROJECT_ID, crate::trace::EventKind::System,
            serde_json::json!({"kind":"computer_control","action":"disable","actor":"owner","enabled":false,"screen_to_selected_model":true}), None, None).unwrap();
        assert!(target(&wb.db, dir.path()).unwrap().is_none());
    }
    proptest! {
        #[test]
        fn cancelled_tickets_never_start(cancellations in 1usize..20) {
            let dispatch = PreviewDispatch::new();
            let ticket = dispatch.generation();
            for _ in 0..cancellations { dispatch.stop(None, || {}).unwrap(); }
            let mut called = false;
            prop_assert!(dispatch.start(ticket, || { called = true; }).is_err(), "cancelled ticket must not start");
            prop_assert!(!called);
            prop_assert_eq!(dispatch.generation(), cancellations as u64);
        }
        #[test]
        fn preview_never_adopts_invalid_ids(window in any::<u64>(), process in any::<i64>()) {
            let value = serde_json::json!({"window_id":window,"process_id":process,"snapshot_id":"s"});
            let result = parse_target(Path::new("/p"), &value);
            prop_assert_eq!(result.is_some(), window > 0 && window <= u32::MAX as u64 && process > 0 && process <= i32::MAX as i64);
        }
        #[test]
        fn frames_never_cross_target_or_exceed_bounds(width in 0u32..2000, height in 0u32..2000, window in any::<u32>()) {
            let target = parse_target(Path::new("/p"), &serde_json::json!({"window_id":1,"process_id":2,"snapshot_id":"s"})).unwrap();
            let reply = Reply { protocol_version:1,ok:true,error:None,data_url:Some("data:image/jpeg;base64,AA==".into()),width:Some(width),height:Some(height),window_id:Some(window),process_id:Some(2) };
            prop_assert_eq!(valid_frame(&reply,&target), window == 1 && (1..=960).contains(&width) && (1..=960).contains(&height));
        }
    }
}
