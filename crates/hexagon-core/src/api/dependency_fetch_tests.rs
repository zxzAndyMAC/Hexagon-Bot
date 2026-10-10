//! Test3/15 (spec 03, Q5/Q10, ADR 0084): controlled public fetch + offline
//! install. The real-network chain is `#[ignore]`d and must be run
//! explicitly; the default suite still proves the isolation and refusals.
use super::*;
use crate::approval_mode::ApprovalMode;
use crate::tools::CallOutcome;
use serde_json::json;

fn call(wb: &Workbench, input: serde_json::Value) -> Result<CallOutcome, crate::tools::ToolError> {
    let ctx = wb.ctx_for("a0", None);
    wb.registry.call(&wb.db, &ctx, "dependency_fetch", input)
}

#[test]
fn unsupported_specs_are_refused_before_any_network() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    for bad in [
        "git+https://github.com/x/y.git",
        "chalk@^4",
        "file:../secret",
        "https://evil.test/a.tgz",
    ] {
        let result = call(&wb, json!({"packages":[bad]}));
        assert!(
            matches!(result, Ok(CallOutcome::Denied(_)) | Err(_)),
            "{bad}: {result:?}"
        );
    }
}

#[test]
fn broad_needs_no_download_card_but_other_modes_ask() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    let input = json!({"packages":["chalk@4.1.2"]});
    let tool = crate::tools::deps::DependencyFetch;
    for (mode, auto) in [
        (ApprovalMode::Restricted, false),
        (ApprovalMode::Assisted, false),
        (ApprovalMode::Broad, true),
    ] {
        wb.set_approval_mode(mode).unwrap();
        let decision =
            crate::permissions::evaluate(&wb.db, &ctx, &tool, "dependency_fetch", &input).unwrap();
        assert_eq!(
            matches!(decision, crate::permissions::Decision::Allow { .. }),
            auto,
            "{mode:?}: {decision:?}"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_fetch_side_cannot_read_the_project() {
    if !crate::sandbox::fence_available() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("secret.txt"), "project source").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let spec = crate::sandbox::fetch_spec(scratch.path(), cache.path());
    let mut cmd = std::process::Command::new("/bin/cat");
    cmd.current_dir(scratch.path())
        .arg(project.path().join("secret.txt"));
    let out = crate::sandbox::wrap_command(&mut cmd, &spec)
        .unwrap()
        .output()
        .unwrap();
    assert!(!out.status.success(), "fetch side read the project");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("project source"));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real network: run explicitly (cargo test -- --ignored dependency_fetch_real)"]
fn dependency_fetch_real_chain_installs_offline_and_reports_scripts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"probe","version":"1.0.0","private":true}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("src.txt"), "source").unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    let CallOutcome::Done(out) =
        call(&wb, json!({"packages":["chalk@4.1.2","esbuild@0.20.2"]})).unwrap()
    else {
        panic!("no download card in Broad")
    };
    let fetched = out["fetched"].as_array().unwrap();
    assert!(
        fetched.iter().any(|p| p["name"] == "ansi-styles"),
        "transitive dependency fetched: {out}"
    );
    assert!(
        fetched.iter().any(|p| p["optional"] == true),
        "optional platform deps recorded"
    );
    assert_eq!(out["install"]["exit_code"], 0, "{out}");
    assert!(dir.path().join("node_modules/chalk/package.json").exists());
    assert!(dir.path().join("node_modules/esbuild").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src.txt")).unwrap(),
        "source"
    );
    // The installed packages actually load inside the fenced, offline shell.
    let ctx = wb.ctx_for("a0", None);
    let CallOutcome::Done(run) = wb.registry.call(&wb.db, &ctx, "bash",
        json!({"cmd":"node -e \"console.log(require('chalk').green('ok'), require('esbuild').version)\""})).unwrap() else { panic!() };
    assert_eq!(run["exit_code"], 0, "{run}");
}
