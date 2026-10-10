//! Test3/16 (spec 04, ADR 0082): real-browser verification of the actual
//! delivery entry. The real-Firefox cases are `#[ignore]`d and must be run
//! explicitly with HEXAGON_BROWSER_RUNTIME naming the verified runtime.
use super::*;
use crate::approval_mode::ApprovalMode;
use crate::tools::CallOutcome;
use serde_json::json;

/// The real-browser cases assert "no browser processes remain" machine-wide,
/// so they must not overlap each other.
static REAL_BROWSER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn project() -> (tempfile::TempDir, Workbench) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("ux")).unwrap();
    std::fs::write(
        dir.path().join("ux/page.html"),
        "<h1>Prototype</h1><label>Name<input id=\"name\"></label><p id=\"state\">S-1</p>\
         <script>addEventListener('keydown',e=>{if(e.key==='ArrowRight')document.getElementById('state').textContent='S-2'})</script>",
    )
    .unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    (dir, wb)
}

fn verify(wb: &Workbench, input: serde_json::Value) -> serde_json::Value {
    let ctx = wb.ctx_for("a0", None);
    let CallOutcome::Done(out) = wb
        .registry
        .call(&wb.db, &ctx, "browser_verify", input)
        .unwrap()
    else {
        panic!("Broad needs no card for a sandboxed verification")
    };
    out
}

#[test]
fn chrome_is_reported_unverified_never_substituted() {
    let (_dir, wb) = project();
    let out = verify(&wb, json!({"entry":"ux/page.html","browser":"chrome"}));
    assert_eq!(out["status"], "unverified");
    assert_eq!(out["reason"], "chrome_not_supported_in_sandbox");
}

#[test]
fn a_missing_runtime_is_unverified_not_passed() {
    if crate::tools::verify_browser::browser_runtime().is_some() {
        return; // a developer configured the real runtime; covered below
    }
    let (_dir, wb) = project();
    let out = verify(&wb, json!({"entry":"ux/page.html"}));
    assert_eq!(out["status"], "unverified");
    assert_eq!(out["reason"], "browser_runtime_unavailable");
}

#[test]
fn entries_outside_the_project_are_refused_and_other_modes_ask() {
    let (_dir, wb) = project();
    let ctx = wb.ctx_for("a0", None);
    let refused = wb.registry.call(
        &wb.db,
        &ctx,
        "browser_verify",
        json!({"entry":"file:///etc/passwd"}),
    );
    assert!(
        matches!(refused, Err(crate::tools::ToolError::BadInput(_))),
        "{refused:?}"
    );
    wb.set_approval_mode(ApprovalMode::Restricted).unwrap();
    let asked = wb
        .registry
        .call(
            &wb.db,
            &ctx,
            "browser_verify",
            json!({"entry":"ux/page.html"}),
        )
        .unwrap();
    assert!(matches!(asked, CallOutcome::Asked(_)));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real Firefox: HEXAGON_BROWSER_RUNTIME=<verified runtime> cargo test -- --ignored browser_verify_real"]
fn browser_verify_real_file_entry_dom_keyboard_and_failure() {
    let _serial = REAL_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        crate::tools::verify_browser::browser_runtime().is_some(),
        "HEXAGON_BROWSER_RUNTIME required"
    );
    let (_dir, wb) = project();
    let ok = verify(
        &wb,
        json!({"entry":"ux/page.html",
        "type":[{"selector":"#name","text":"real-browser"}],
        "keys":["ArrowRight"],
        "checks":[{"selector":"h1","text":"Prototype"},{"selector":"#state","text":"S-2"}]}),
    );
    assert_eq!(ok["status"], "verified", "{ok}");
    assert_eq!(ok["stages"]["launch"]["ok"], true);
    assert_eq!(ok["stages"]["type"][0]["value"], "real-browser");
    let failed = verify(
        &wb,
        json!({"entry":"ux/page.html","checks":[{"selector":"#missing"}]}),
    );
    assert_eq!(failed["status"], "failed", "{failed}");
    assert_eq!(failed["stages"]["checks"][0]["ok"], false);
    // No Firefox descendants outlive the verification (the tool returns only
    // after the whole process group is gone).
    let ps = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid,command"])
        .output()
        .unwrap();
    let runtime = crate::tools::verify_browser::browser_runtime().unwrap();
    let left: Vec<String> = String::from_utf8_lossy(&ps.stdout)
        .lines()
        // The executable itself must live in the runtime; a shell whose
        // command line merely mentions the path is not a browser process.
        .filter(|l| {
            l.trim_start()
                .split_once(' ')
                .is_some_and(|(_, cmd)| cmd.starts_with(&*runtime.to_string_lossy()))
        })
        .map(str::to_string)
        .collect();
    assert!(
        left.is_empty(),
        "browser descendants left behind: {left:#?}"
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real Firefox: HEXAGON_BROWSER_RUNTIME=<verified runtime> cargo test -- --ignored browser_verify_real"]
fn browser_verify_real_hung_page_times_out_without_leftovers() {
    let _serial = REAL_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        crate::tools::verify_browser::browser_runtime().is_some(),
        "HEXAGON_BROWSER_RUNTIME required"
    );
    let (dir, wb) = project();
    std::fs::write(
        dir.path().join("ux/hang.html"),
        "<script>while(true){}</script>",
    )
    .unwrap();
    let out = verify(
        &wb,
        json!({"entry":"ux/hang.html","checks":[{"selector":"body"}]}),
    );
    assert_eq!(out["status"], "failed", "{out}");
    let ps = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid,command"])
        .output()
        .unwrap();
    let runtime = crate::tools::verify_browser::browser_runtime().unwrap();
    let left = String::from_utf8_lossy(&ps.stdout)
        .lines()
        .filter(|l| {
            l.trim_start()
                .split_once(' ')
                .is_some_and(|(_, c)| c.starts_with(&*runtime.to_string_lossy()))
        })
        .count();
    assert_eq!(left, 0, "timed-out run left browser processes");
}
