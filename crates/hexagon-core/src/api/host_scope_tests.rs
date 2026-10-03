use super::*;
use crate::approval_mode::ApprovalMode;
use crate::tools::CallOutcome;
use proptest::prelude::*;
use serde_json::json;
fn done(value: CallOutcome) -> serde_json::Value {
    match value {
        CallOutcome::Done(value) => value,
        other => panic!("expected completed action: {other:?}"),
    }
}
#[test]
fn broad_host_files_work_and_replacement_requires_owner_with_fresh_bytes() {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["后端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    let file = external.path().join("note.txt");
    let input = json!({"path":file,"content":"first"});
    assert!(matches!(
        wb.registry
            .call(&wb.db, &ctx, "host_fs_write", input.clone())
            .unwrap(),
        CallOutcome::Denied(_)
    ));
    assert!(!file.exists());
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    done(
        wb.registry
            .call(&wb.db, &ctx, "host_fs_write", input)
            .unwrap(),
    );
    let read = done(
        wb.registry
            .call(&wb.db, &ctx, "host_fs_read", json!({"path":file}))
            .unwrap(),
    );
    assert_eq!(read["content"], "first");
    let replace = json!({"path":file,"content":"second","expected_sha256":read["sha256"]});
    let CallOutcome::Asked(question) = wb
        .registry
        .call(&wb.db, &ctx, "host_fs_write", replace)
        .unwrap()
    else {
        panic!("replacement must ask")
    };
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first");
    wb.answer_permission(&question, true, None, "once").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "second");
    let stale =
        json!({"path":file,"content":"overwrite changed data","expected_sha256":read["sha256"]});
    let CallOutcome::Asked(question) = wb
        .registry
        .call(&wb.db, &ctx, "host_fs_write", stale)
        .unwrap()
    else {
        panic!("replacement must ask")
    };
    assert!(wb.answer_permission(&question, true, None, "once").is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "second");
}
#[test]
fn broad_host_paths_keep_current_and_other_project_ownership_and_revocation() {
    let project = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["后端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    std::fs::create_dir_all(other.path().join(".hexagon")).unwrap();
    for path in [
        project.path().join("foreign.rs"),
        other.path().join("foreign.rs"),
    ] {
        // Structured preconditions may reject before the permission evaluator;
        // either form must prove no host side effect and preserve project ownership.
        let rejected = wb.registry.call(
            &wb.db,
            &ctx,
            "host_fs_write",
            json!({"path":path,"content":"bad"}),
        );
        assert!(matches!(
            rejected,
            Err(crate::tools::ToolError::NotExecuted(_)) | Ok(CallOutcome::Denied(_))
        ));
        assert!(!path.exists());
    }
    assert!(wb
        .registry
        .subagent_scope(&[])
        .get("host_fs_read")
        .is_none());
    assert!(wb.registry.subagent_scope(&[]).get("host_bash").is_none());
    let external = tempfile::tempdir().unwrap();
    let path = external.path().join("note.txt");
    std::fs::write(&path, "old").unwrap();
    let read = done(
        wb.registry
            .call(&wb.db, &ctx, "host_fs_read", json!({"path":path}))
            .unwrap(),
    );
    let CallOutcome::Asked(question) = wb
        .registry
        .call(
            &wb.db,
            &ctx,
            "host_fs_write",
            json!({"path":path,"content":"new","expected_sha256":read["sha256"]}),
        )
        .unwrap()
    else {
        panic!("must ask")
    };
    wb.set_approval_mode(ApprovalMode::Restricted).unwrap();
    let _ = wb.answer_permission(&question, true, None, "once");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "old");
}
#[test]
#[cfg(target_os = "macos")]
fn broad_host_shell_is_real_scoped_and_never_silently_approved() {
    let project = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(project.path(), &["后端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    let nested = external.path().join("nested");
    std::fs::create_dir_all(nested.join(".hexagon")).unwrap();
    let input = json!({"cwd":external.path(),"cmd":"printf ok > ordinary.txt; printf denied > nested/foreign.rs; test -f ordinary.txt"});
    let CallOutcome::Asked(question) = wb.registry.call(&wb.db, &ctx, "host_bash", input).unwrap()
    else {
        panic!("every host command must ask")
    };
    assert!(!external.path().join("ordinary.txt").exists());
    wb.answer_permission(&question, true, None, "once").unwrap();
    assert_eq!(
        std::fs::read_to_string(external.path().join("ordinary.txt")).unwrap(),
        "ok"
    );
    assert!(!nested.join("foreign.rs").exists());
    assert!(!external.path().join(".hexagon").exists());
    let unsafe_scope = json!({"cwd":project.path().parent().unwrap(),"cmd":"pwd"});
    assert!(matches!(
        wb.registry
            .call(&wb.db, &ctx, "host_bash", unsafe_scope)
            .unwrap(),
        CallOutcome::Denied(_)
    ));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]
    #[test]
    fn host_modes_never_release_credential_or_policy_paths(mode in prop::sample::select(vec![ApprovalMode::Restricted,ApprovalMode::Assisted,ApprovalMode::Broad]), name in prop::sample::select(vec![".env","credentials.json","id_rsa","AGENTS.md","SKILL.md","permission_rules.toml","Cookies","auth.json",".git-credentials"])) {
        let project=tempfile::tempdir().unwrap();let external=tempfile::tempdir().unwrap();
        let wb=Workbench::for_test(project.path(), &["后端"],None).unwrap();let ctx=wb.ctx_for("a0",None);
        wb.set_approval_mode(mode).unwrap();let path=external.path().join(name);
        std::fs::write(&path,"secret or policy").unwrap();
        let result=wb.registry.call(&wb.db,&ctx,"host_fs_read",json!({"path":path})).unwrap();
        prop_assert!(matches!(result,CallOutcome::Denied(_)));
        prop_assert_eq!(std::fs::read_to_string(path).unwrap(),"secret or policy");
    }
}
