//! Test3/14 (spec 03, Q1/Q6): broad access runs ordinary project commands on
//! real processes under the deletion fence. These are real sandbox probes
//! (macOS seatbelt); other platforms keep asking and only check that.
use super::*;
use crate::approval_mode::ApprovalMode;
use crate::tools::CallOutcome;
use serde_json::json;

fn broad_wb(dir: &Path) -> Workbench {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/keep.txt"), "source").unwrap();
    let wb = Workbench::for_test(dir, &["worker"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    wb
}

fn bash(wb: &Workbench, input: serde_json::Value) -> CallOutcome {
    let ctx = wb.ctx_for("a0", None);
    wb.registry.call(&wb.db, &ctx, "bash", input).unwrap()
}

fn available(bin: &str) -> bool {
    std::process::Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn ineligible_shapes_keep_asking_in_every_mode() {
    let dir = tempfile::tempdir().unwrap();
    let wb = broad_wb(dir.path());
    for input in [
        json!({"cmd":"echo hi","net":true}),
        json!({"cmd":"echo hi","session":"s1"}),
        json!({"cmd":"echo hi","background":true}),
    ] {
        let CallOutcome::Asked(qid) = bash(&wb, input.clone()) else {
            panic!("{input}")
        };
        let reason = crate::cards::get(&wb.db, &qid).unwrap().payload["reason"].to_string();
        if crate::sandbox::fence_available() {
            assert!(
                reason.contains("broad access:"),
                "the card says why: {reason}"
            );
        }
    }
    wb.set_approval_mode(ApprovalMode::Assisted).unwrap();
    assert!(matches!(
        bash(&wb, json!({"cmd":"echo hi"})),
        CallOutcome::Asked(_)
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn broad_runs_python_and_node_but_the_fence_blocks_deletion_and_network() {
    if !crate::sandbox::fence_available() || !available("python3") || !available("node") {
        eprintln!("skipped: seatbelt/python3/node unavailable");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wb = broad_wb(dir.path());
    let script = "import os, tempfile\n\
open('build.out','w').write('built')\n\
t = tempfile.mkstemp()[1]; os.remove(t)\n\
open('src/keep.txt','w').write('edited')\n\
print('ok', os.environ['TMPDIR'])\n";
    std::fs::write(dir.path().join("build.py"), script).unwrap();
    let CallOutcome::Done(out) = bash(&wb, json!({"cmd":"python3 build.py"})) else {
        panic!("no card")
    };
    assert_eq!(out["exit_code"], 0, "{out}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("build.out")).unwrap(),
        "built"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/keep.txt")).unwrap(),
        "edited",
        "editing is allowed"
    );
    assert_eq!(out["broad_access"]["scratch_cleaned"], true);
    let scratch = out.to_string();
    assert!(
        scratch.contains("hexagon-exec"),
        "TMPDIR is the host scratch"
    );

    // A visible `rm` is a safety-net command: it still asks the owner.
    assert!(matches!(
        bash(&wb, json!({"cmd":"rm src/keep.txt"})),
        CallOutcome::Asked(_)
    ));
    // Deletion hidden inside Python/Node is not recognised by name; the OS
    // fence blocks it on the real process.
    for cmd in [
        "python3 -c \"import os; os.remove('src/keep.txt')\"",
        "node -e \"require('fs').unlinkSync('src/keep.txt')\"",
        "python3 -c \"import os; open('x','w').write('x'); os.replace('x','src/keep.txt')\"",
        "python3 -c \"import shutil; shutil.rmtree('src')\"",
    ] {
        let CallOutcome::Done(out) = bash(&wb, json!({"cmd":cmd})) else {
            panic!("{cmd} asked")
        };
        assert_ne!(out["exit_code"], 0, "{cmd}: {out}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/keep.txt")).unwrap(),
            "edited",
            "{cmd}"
        );
    }
    let CallOutcome::Done(out) = bash(
        &wb,
        json!({"cmd":"python3 -c \"import socket; socket.create_connection(('1.1.1.1',80),timeout=3)\""}),
    ) else {
        panic!()
    };
    assert_ne!(out["exit_code"], 0, "offline: {out}");
    let CallOutcome::Done(out) = bash(&wb, json!({"cmd":"python3 -c \"import os; os.setsid()\""}))
    else {
        panic!()
    };
    assert_ne!(out["exit_code"], 0, "no detached session: {out}");
    let CallOutcome::Done(out) = bash(&wb, json!({"cmd":"echo x > .hexagon/forged"})) else {
        panic!()
    };
    assert_ne!(out["exit_code"], 0, "host state stays closed: {out}");
    let cleaned: i64 = wb.db.conn().query_row(
        "SELECT COUNT(*) FROM events WHERE json_extract(payload,'$.kind')='fenced_exec' AND json_extract(payload,'$.scratch_cleaned')=1",
        [], |r| r.get(0)).unwrap();
    assert!(cleaned >= 1);
}

#[test]
fn safety_net_and_deny_rules_still_win_over_broad() {
    let dir = tempfile::tempdir().unwrap();
    let wb = broad_wb(dir.path());
    assert!(matches!(
        bash(&wb, json!({"cmd":"git push origin main"})),
        CallOutcome::Asked(_) | CallOutcome::Denied(_)
    ));
}
