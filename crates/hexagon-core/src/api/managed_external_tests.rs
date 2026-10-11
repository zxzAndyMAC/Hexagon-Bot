//! Test3/23 (Q19–Q21): real-process probes for the delegated host-shell
//! fence. External files may be created, edited and overwritten; they may
//! not be deleted, renamed away/over, or have their directories removed —
//! whatever the shell, Python or Node program tries.
use super::*;
use crate::tools::Tool;
use serde_json::json;

fn delegated(dir: &Path) -> Workbench {
    let wb = Workbench::for_test(dir, &["worker"], None).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    wb
}

fn host_bash(wb: &Workbench, cwd: &Path, cmd: &str) -> serde_json::Value {
    let ctx = wb.ctx_for("a0", None);
    crate::tools::host::HostBash
        .exec(
            &wb.db,
            &json!({"cwd": cwd.to_string_lossy(), "cmd": cmd}),
            &ctx,
        )
        .unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn delegated_host_shell_edits_external_files_but_cannot_delete_them() {
    if !crate::sandbox::fence_available() {
        eprintln!("skipped: seatbelt unavailable");
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let wb = delegated(project.path());
    let outside = tempfile::tempdir().unwrap();
    let ext = outside.path();
    std::fs::write(ext.join("keep.txt"), "original").unwrap();
    std::fs::create_dir(ext.join("empty")).unwrap();
    let out = host_bash(
        &wb,
        ext,
        "printf made > new.txt && printf overwritten > keep.txt && echo ok",
    );
    assert_eq!(out["exit_code"], 0, "{out}");
    assert_eq!(
        std::fs::read_to_string(ext.join("new.txt")).unwrap(),
        "made"
    );
    assert_eq!(
        std::fs::read_to_string(ext.join("keep.txt")).unwrap(),
        "overwritten"
    );
    for attempt in [
        "rm keep.txt",
        "rm -f new.txt",
        "mv keep.txt renamed.txt",
        "printf x > other.txt; mv other.txt keep.txt",
        "rmdir empty",
        "rm -rf empty",
        "mv keep.txt ~/.Trash/",
    ] {
        host_bash(&wb, ext, attempt);
        assert!(ext.join("keep.txt").exists(), "{attempt} removed keep.txt");
        assert!(ext.join("new.txt").exists(), "{attempt} removed new.txt");
        assert!(
            ext.join("empty").is_dir(),
            "{attempt} removed the directory"
        );
    }
    assert!(!ext.join("renamed.txt").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn delegated_python_and_node_cannot_delete_external_files_either() {
    let available = |bin: &str| {
        std::process::Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !crate::sandbox::fence_available() || !available("python3") || !available("node") {
        eprintln!("skipped: seatbelt/python3/node unavailable");
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let wb = delegated(project.path());
    let outside = tempfile::tempdir().unwrap();
    let ext = outside.path();
    std::fs::write(ext.join("doc.txt"), "v1").unwrap();
    let py = host_bash(
        &wb,
        ext,
        "python3 -c \"open('doc.txt','a').write('+py'); import os\ntry:\n os.remove('doc.txt')\nexcept OSError: print('denied')\ntry:\n os.replace('doc.txt','x.txt')\nexcept OSError: print('denied')\"",
    );
    assert!(
        py["stdout"]
            .as_str()
            .unwrap_or_default()
            .matches("denied")
            .count()
            >= 2,
        "{py}"
    );
    let node = host_bash(
        &wb,
        ext,
        "node -e \"const fs=require('fs');fs.appendFileSync('doc.txt','+node');try{fs.unlinkSync('doc.txt')}catch(e){console.log('denied')}\"",
    );
    assert!(
        node["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("denied"),
        "{node}"
    );
    assert_eq!(
        std::fs::read_to_string(ext.join("doc.txt")).unwrap(),
        "v1+py+node"
    );
}

#[test]
fn without_delegation_the_owner_broad_scope_is_unchanged() {
    let project = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["worker"], None).unwrap();
    wb.set_approval_mode(crate::approval_mode::ApprovalMode::Broad)
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("scratch.txt"), "x").unwrap();
    let out = host_bash(&wb, outside.path(), "rm scratch.txt && echo removed");
    if crate::sandbox::fence_available() {
        assert_eq!(out["exit_code"], 0, "{out}");
        assert!(
            !outside.path().join("scratch.txt").exists(),
            "owner-approved deletion still works"
        );
    }
}
