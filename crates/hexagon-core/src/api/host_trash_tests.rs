//! Managed-followups ticket 02 (ADR0087 本机删除只进废纸篓): real-process
//! probes for host_fs_trash. The item really lands in the system Trash and
//! can be put back; protected targets and failures change nothing; the
//! delegated host shell still cannot delete and says which tool to use.
use super::*;
use crate::approval_mode::ApprovalMode;
use crate::tools::CallOutcome;
use proptest::prelude::*;
use serde_json::json;

fn broad(project: &Path) -> Workbench {
    let wb = Workbench::for_test(project, &["后端"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    wb
}

fn trash(wb: &Workbench, path: &Path) -> Result<CallOutcome, crate::tools::ToolError> {
    let ctx = wb.ctx_for("a0", None);
    wb.registry
        .call(&wb.db, &ctx, "host_fs_trash", json!({"path": path}))
}

fn trashed_facts(wb: &Workbench) -> Vec<serde_json::Value> {
    wb.db
        .events(crate::PROJECT_ID, Some(&[EventKind::System]))
        .unwrap()
        .into_iter()
        .map(|event| event.payload)
        .filter(|payload| payload["kind"] == "host_trashed")
        .collect()
}

/// Put the item back where it came from (the owner's restore), so tests
/// never leave entries in the real Trash.
fn put_back(fact: &serde_json::Value) {
    let from = Path::new(fact["trash_path"].as_str().unwrap());
    let to = Path::new(fact["path"].as_str().unwrap());
    assert!(from.exists(), "the item is in the Trash");
    std::fs::rename(from, to).unwrap();
}

fn refused(outcome: Result<CallOutcome, crate::tools::ToolError>) -> bool {
    matches!(
        outcome,
        Ok(CallOutcome::Denied(_)) | Err(crate::tools::ToolError::NotExecuted(_))
    )
}

#[cfg(target_os = "macos")]
#[test]
fn an_approved_trash_moves_the_item_to_the_system_trash_and_it_can_be_put_back() {
    let project = tempfile::tempdir().unwrap();
    let wb = broad(project.path());
    let outside = tempfile::tempdir().unwrap();
    let file = outside
        .path()
        .join(format!("hexagon-trash-{}.txt", std::process::id()));
    std::fs::write(&file, "keep me").unwrap();
    let file = file.canonicalize().unwrap();
    let Ok(CallOutcome::Asked(question)) = trash(&wb, &file) else {
        panic!("a deletion always asks, even in Broad")
    };
    assert!(file.exists(), "nothing moves before approval");
    wb.answer_permission(&question, true, None, "once").unwrap();
    assert!(!file.exists(), "the original path is gone");
    let facts = trashed_facts(&wb);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0]["path"], file.to_string_lossy().as_ref());
    let in_trash = Path::new(facts[0]["trash_path"].as_str().unwrap());
    assert!(in_trash.exists());
    assert!(
        in_trash.to_string_lossy().contains(".Trash"),
        "{in_trash:?}"
    );
    assert!(facts[0].get("content").is_none(), "paths only, no contents");
    put_back(&facts[0]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep me");
}

#[cfg(target_os = "macos")]
#[test]
fn a_delegated_trash_is_decided_by_the_grant_and_recorded_under_it() {
    use crate::provider::ScriptedProvider;
    let project = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(project.path(), &["后端"], None).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = outside.path().join("old-renders");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("frame.png"), "x").unwrap();
    let dir = dir.canonicalize().unwrap();
    // The card comes from the role's real task, which delegation continues.
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![crate::turn::tool_response(
            vec![("c1", "host_fs_trash", json!({"path": dir}))],
        )])),
    );
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let TurnOutcome::AwaitingPermission(_) = wb.run_instance("a0", "clean old renders").unwrap()
    else {
        panic!("delegation decides the trash card")
    };
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![
            crate::turn::text_response("allow"),
            crate::turn::text_response("done"),
        ])),
    );
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "allow", "{done:?}");
    let history = managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap();
    assert!(!dir.exists(), "{history:?}");
    let facts = trashed_facts(&wb);
    assert_eq!(
        facts[0]["managed_actor"],
        format!(
            "managed:{}:{}",
            status.delegation_id.unwrap(),
            status.generation
        )
    );
    put_back(&facts[0]);
    assert!(dir.join("frame.png").exists());
}

#[test]
fn without_broad_access_or_delegation_the_tool_is_refused() {
    let project = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["后端"], None).unwrap();
    wb.set_approval_mode(ApprovalMode::Assisted).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("a.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(refused(trash(&wb, &file)));
    assert!(file.exists());
}

#[test]
fn protected_or_missing_targets_are_refused_and_nothing_moves() {
    let project = tempfile::tempdir().unwrap();
    let wb = broad(project.path());
    let outside = tempfile::tempdir().unwrap();
    let base = outside.path();
    std::fs::write(base.join(".env"), "SECRET=1").unwrap();
    std::fs::create_dir_all(base.join("other/.hexagon")).unwrap();
    std::fs::create_dir_all(base.join("tree/deep/.ssh")).unwrap();
    std::fs::write(base.join("tree/deep/.ssh/id_rsa"), "key").unwrap();
    std::fs::write(base.join("real.txt"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(base.join("real.txt"), base.join("link.txt")).unwrap();
    let home = std::env::var("HOME").unwrap();
    for target in [
        base.join(".env"),
        base.join("other"),
        base.join("tree"),
        base.join("missing.txt"),
        base.join("link.txt"),
        project.path().join("src.rs"),
        project.path().to_path_buf(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/Applications"),
        PathBuf::from(&home),
        Path::new(&home).join("Library/Preferences"),
        Path::new(&home).join(".Trash"),
        PathBuf::from("relative.txt"),
    ] {
        assert!(refused(trash(&wb, &target)), "{target:?} was not refused");
    }
    assert!(base.join(".env").exists());
    assert!(base.join("other/.hexagon").is_dir());
    assert!(base.join("tree/deep/.ssh/id_rsa").exists());
    assert!(base.join("real.txt").exists());
    assert!(cards_queued(&wb) == 0, "refusals never become cards");
}

fn cards_queued(wb: &Workbench) -> usize {
    crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .len()
}

#[cfg(not(target_os = "macos"))]
#[test]
fn other_platforms_report_the_tool_as_unavailable() {
    let project = tempfile::tempdir().unwrap();
    let wb = broad(project.path());
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("a.txt");
    std::fs::write(&file, "x").unwrap();
    let Ok(CallOutcome::Asked(question)) = trash(&wb, &file) else {
        panic!("asks first")
    };
    let _ = wb.answer_permission(&question, true, None, "once");
    assert!(file.exists());
    assert!(trashed_facts(&wb).is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn the_delegated_shell_still_cannot_delete_and_points_to_the_trash_tool() {
    use crate::tools::Tool;
    if !crate::sandbox::fence_available() {
        eprintln!("skipped: seatbelt unavailable");
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["worker"], None).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("doc.txt"), "v1").unwrap();
    let ctx = wb.ctx_for("a0", None);
    for cmd in [
        "rm doc.txt",
        "python3 -c \"import os; os.remove('doc.txt')\"",
        "node -e \"require('fs').unlinkSync('doc.txt')\"",
    ] {
        let out = crate::tools::host::HostBash
            .exec(
                &wb.db,
                &json!({"cwd": outside.path().to_string_lossy(), "cmd": cmd}),
                &ctx,
            )
            .unwrap();
        assert!(outside.path().join("doc.txt").exists(), "{cmd} deleted");
        assert!(
            out["deletion_fence"]
                .as_str()
                .unwrap_or_default()
                .contains("host_fs_trash"),
            "{out}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // D12 for the new trash judge: whatever ordinary path wraps it, a target
    // that is or contains credentials, policy, agent state or a project is
    // refused and stays where it was.
    #[test]
    fn protected_state_is_never_trashed(
        protected in prop::sample::select(vec![".hexagon", ".claude", ".codex", "AGENTS.md", "auth.json", ".git-credentials", ".kube", "Cookies", ".env", "id_rsa"]),
        prefix in "[a-z]{1,8}",
        wrap in any::<bool>(),
    ) {
        let project = tempfile::tempdir().unwrap();
        let wb = broad(project.path());
        let outside = tempfile::tempdir().unwrap();
        let holder = outside.path().join(&prefix);
        std::fs::create_dir_all(&holder).unwrap();
        let item = holder.join(protected);
        std::fs::write(&item, "state").unwrap();
        let target = if wrap { holder.clone() } else { item.clone() };
        prop_assert!(refused(trash(&wb, &target)));
        prop_assert!(item.exists());
    }
}
