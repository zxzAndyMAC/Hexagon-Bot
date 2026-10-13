//! P1-3 runtime health through the real owner beat: an owned MCP worker that
//! exits is observed by `McpHost::status`, the SessionHost spends one budget
//! slot per beat, and a live worker restores health. No side effect is
//! retried; the beat only records state.
use super::*;
use crate::session_host::{CapabilityHealth, SessionStatus, Transition};
use std::time::{Duration, Instant};

const EXITING_PEER: &str = r#"
const rl=require('readline').createInterface({input:process.stdin});
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
rl.on('line',line=>{const r=JSON.parse(line);if(r.id===undefined)return;
if(r.method==='initialize')send(r.id,{protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'w',version:'1'}});
else if(r.method==='tools/list')send(r.id,{tools:[]});
});
if(process.env.EXIT_WHEN)setInterval(()=>{if(require('fs').existsSync(process.env.EXIT_WHEN))process.exit(0);},20);
"#;

/// `exit_marker`: the worker exits once this file exists, so a test can
/// observe it healthy first and then lose it (a real worker exit).
fn workbench_with_worker(dir: &Path, exit_marker: Option<&Path>) -> (Workbench, String) {
    let script = dir.join("worker.cjs");
    std::fs::write(&script, EXITING_PEER).unwrap();
    std::fs::create_dir_all(dir.join(".hexagon")).unwrap();
    let mut env = serde_json::Map::new();
    if let Some(marker) = exit_marker {
        env.insert(
            "EXIT_WHEN".into(),
            marker.to_string_lossy().into_owned().into(),
        );
    }
    std::fs::write(
        dir.join(".hexagon/mcp.json"),
        serde_json::json!([{ "name": "worker", "command": "node", "args": [script], "env": env }])
            .to_string(),
    )
    .unwrap();
    let wb = Workbench::for_test(dir, &["worker"], None).unwrap();
    let session =
        crate::session_host::ensure_current(&wb.db, "p1", &dir.to_string_lossy(), "main", 3)
            .unwrap();
    crate::session_host::transition(&wb.db, &session.session_id, Transition::Start, 0).unwrap();
    crate::session_host::bind_turn(&wb.db, &session.session_id, "7").unwrap();
    (wb, session.session_id)
}

fn wait_for(wb: &Workbench, status: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if wb
            .mcp_host
            .as_ref()
            .is_some_and(|host| host.status().iter().all(|row| row.status == status))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("MCP worker never reached {status}");
}

#[test]
fn worker_exit_spends_one_budget_slot_per_beat_until_failed() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("exit-now");
    let (wb, session_id) = workbench_with_worker(dir.path(), Some(&marker));
    // Deterministic exhaustion: no wall-clock window between budget slots.
    wb.runtime_observed.lock().unwrap().reconnect_window = Duration::ZERO;
    wait_for(&wb, "up");
    wb.session_runtime_beat().unwrap();
    assert_eq!(
        crate::session_host::get(&wb.db, &session_id)
            .unwrap()
            .status,
        SessionStatus::Running
    );
    std::fs::write(&marker, "").unwrap();
    wait_for(&wb, "down");
    let mut seen = vec![];
    for _ in 0..3 {
        wb.session_runtime_beat().unwrap();
        let current = crate::session_host::get(&wb.db, &session_id).unwrap();
        seen.push((current.status, current.reconnect_attempts));
        let runtime = current.runtime.unwrap();
        assert!(runtime.last_error.is_some());
        let mcp = runtime
            .capabilities
            .iter()
            .find(|cap| cap.id == "worker")
            .unwrap();
        assert_eq!(mcp.health, CapabilityHealth::Unavailable);
        assert!(!mcp.granted, "liveness is never authorization");
    }
    assert_eq!(
        seen,
        vec![
            (SessionStatus::Reconnecting, 1),
            (SessionStatus::Reconnecting, 2),
            (SessionStatus::Failed, 3),
        ]
    );
    let failed = crate::session_host::get(&wb.db, &session_id).unwrap();
    assert_eq!(
        failed.last_reason_code.as_deref(),
        Some("reconnect_budget_exhausted")
    );
    // A further beat on a failed host records health but never revives it.
    wb.session_runtime_beat().unwrap();
    assert_eq!(
        crate::session_host::get(&wb.db, &session_id)
            .unwrap()
            .status,
        SessionStatus::Failed
    );
}

#[test]
fn a_live_worker_restores_a_reconnecting_host() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, session_id) = workbench_with_worker(dir.path(), None);
    wait_for(&wb, "up");
    crate::session_host::transition(&wb.db, &session_id, Transition::RuntimeFailed, 0).unwrap();
    wb.session_runtime_beat().unwrap();
    let current = crate::session_host::get(&wb.db, &session_id).unwrap();
    assert_eq!(current.status, SessionStatus::Running);
    assert_eq!(current.last_reason_code.as_deref(), Some("health_restored"));
    let runtime = current.runtime.unwrap();
    assert_eq!(runtime.health, CapabilityHealth::Healthy);
    assert_eq!(runtime.last_error, None);
}

#[test]
fn an_unchanged_beat_records_nothing_new() {
    let dir = tempfile::tempdir().unwrap();
    let (wb, _) = workbench_with_worker(dir.path(), None);
    wait_for(&wb, "up");
    let count = || {
        wb.db
            .events("p1", Some(&[crate::trace::EventKind::System]))
            .unwrap()
            .iter()
            .filter(|event| event.payload["kind"] == "runtime_health")
            .count()
    };
    wb.session_runtime_beat().unwrap();
    let first = count();
    wb.session_runtime_beat().unwrap();
    wb.session_runtime_beat().unwrap();
    assert_eq!(count(), first, "periodic beats must not spam the trace");
}

/// Golden-path regression (2026-10-13): a configured MCP server that never
/// came up is an unavailable capability, not a lost worker; it must not
/// spend the reconnect budget of an unrelated running session.
#[test]
fn a_worker_that_never_came_up_does_not_fail_the_session() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".hexagon")).unwrap();
    std::fs::write(
        dir.path().join(".hexagon/mcp.json"),
        serde_json::json!([{ "name": "broken", "command": "/nonexistent/mcp-server" }]).to_string(),
    )
    .unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let session =
        crate::session_host::ensure_current(&wb.db, "p1", &dir.path().to_string_lossy(), "main", 3)
            .unwrap();
    crate::session_host::transition(&wb.db, &session.session_id, Transition::Start, 0).unwrap();
    crate::session_host::bind_turn(&wb.db, &session.session_id, "7").unwrap();
    wait_for(&wb, "down");
    for _ in 0..4 {
        wb.session_runtime_beat().unwrap();
    }
    let current = crate::session_host::get(&wb.db, &session.session_id).unwrap();
    assert_eq!(current.status, SessionStatus::Running);
    assert_eq!(current.reconnect_attempts, 0);
    let runtime = current.runtime.unwrap();
    assert_eq!(runtime.health, CapabilityHealth::Degraded);
    assert_eq!(
        runtime.last_error.as_deref(),
        Some("capability_unavailable")
    );
}

/// The owner gets a window to bring the worker back (reopen the project):
/// while reconnecting, consecutive beats inside the window spend nothing.
#[test]
fn reconnecting_spends_at_most_one_slot_per_window() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("exit-now");
    let (wb, session_id) = workbench_with_worker(dir.path(), Some(&marker));
    wait_for(&wb, "up");
    wb.session_runtime_beat().unwrap();
    std::fs::write(&marker, "").unwrap();
    wait_for(&wb, "down");
    for _ in 0..5 {
        wb.session_runtime_beat().unwrap();
    }
    let current = crate::session_host::get(&wb.db, &session_id).unwrap();
    assert_eq!(current.status, SessionStatus::Reconnecting);
    assert_eq!(current.reconnect_attempts, 1);
}
