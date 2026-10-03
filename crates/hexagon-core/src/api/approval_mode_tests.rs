use super::*;
use crate::approval_mode::ApprovalMode;
use crate::permissions::{AllowVia, Decision};
use proptest::prelude::*;
use serde_json::json;

#[test]
fn approval_mode_is_project_persistent_and_new_projects_are_restricted() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let wb = Workbench::open_scoped(first.path(), "first", &[], None, false).unwrap();
    assert_eq!(wb.approval_mode().unwrap().mode, ApprovalMode::Restricted);
    let before = crate::autonomy::rank(&wb.db, &wb.project_id).unwrap();
    wb.set_approval_mode(ApprovalMode::Broad).unwrap();
    assert_eq!(
        crate::autonomy::rank(&wb.db, &wb.project_id).unwrap(),
        before
    );
    assert_eq!(
        approval_mode(&wb.db, &wb.project_id).unwrap().mode,
        ApprovalMode::Broad
    );
    let changed: i64 = wb.db.conn().query_row(
        "SELECT COUNT(*) FROM events WHERE json_extract(payload,'$.kind')='approval_mode_changed'", [], |row| row.get(0)
    ).unwrap();
    assert_eq!(changed, 1);
    drop(wb);
    assert_eq!(
        Workbench::open_scoped(first.path(), "first", &[], None, false)
            .unwrap()
            .approval_mode()
            .unwrap()
            .mode,
        ApprovalMode::Broad
    );
    assert_eq!(
        Workbench::open_scoped(second.path(), "second", &[], None, false)
            .unwrap()
            .approval_mode()
            .unwrap()
            .mode,
        ApprovalMode::Restricted
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn approval_mode_preserves_credentials_ownership_and_critical_confirmation(
        mode in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
        cmd in prop::sample::select(vec!["git push origin main", "rm -rf src", "npm publish", "stripe payments create", "sendmail user@example.com"]),
        remembered in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        wb.set_approval_mode(mode).unwrap();
        let mut ctx = wb.ctx_for("a0", None);
        ctx.owned_globs = vec!["src/**".into()];
        if remembered {
            wb.db.conn().execute("INSERT INTO permission_rules(id,project_id,tool,shape,effect,scope) VALUES('r',?1,'bash','*','allow','project')", [&wb.project_id]).unwrap();
        }
        let critical = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::Bash, "bash", &json!({"cmd":cmd})).unwrap();
        prop_assert!(matches!(critical, Decision::Ask { safety_net: true, .. }), "{mode:?}: {cmd}: {critical:?}");
        let secret = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::FsRead, "fs_read", &json!({"path":".env"})).unwrap();
        prop_assert!(matches!(secret, Decision::Deny { .. }), "credentials must stay denied");
        let foreign = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::FsWrite, "fs_write", &json!({"path":"other/a.ts","content":"x"})).unwrap();
        prop_assert!(matches!(foreign, Decision::Deny { .. }), "ownership must stay denied");
    }

    #[test]
    fn approval_mode_unknown_shell_never_inherits_coordinator_autonomy(
        mode in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
        cmd in prop::sample::select(vec!["npm test", "python3 script.py", "curl https://example.com", "pwd && echo unsafe", "git -c alias.x=push x"]),
        child in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        wb.set_approval_mode(mode).unwrap();
        let mut ctx = wb.ctx_for("a0", None);
        if child {
            ctx.subagent = Some(crate::subagent::Scope {
        approval_mode: Default::default(),
        permission_rules: Default::default(),
                halt: Default::default(), answer: Default::default(),
                mcp: Default::default(), reads: Default::default(),
            });
        }
        let result = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::Bash, "bash", &json!({"cmd":cmd})).unwrap();
        prop_assert!(matches!(result, Decision::Ask { .. }), "{mode:?}: {cmd}: {result:?}");
    }
}

#[test]
fn approval_mode_assisted_read_and_broad_network_are_separate() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    for mode in [
        ApprovalMode::Restricted,
        ApprovalMode::Assisted,
        ApprovalMode::Broad,
    ] {
        wb.set_approval_mode(mode).unwrap();
        let read = crate::permissions::evaluate(
            &wb.db,
            &ctx,
            &crate::tools::Bash,
            "bash",
            &json!({"cmd":"pwd"}),
        )
        .unwrap();
        assert_eq!(
            matches!(
                read,
                Decision::Allow {
                    via: AllowVia::ApprovalMode { .. }
                }
            ),
            mode != ApprovalMode::Restricted
        );
        let web = crate::permissions::evaluate(
            &wb.db,
            &ctx,
            &crate::tools::WebFetch,
            "web_fetch",
            &json!({"url":"https://example.com"}),
        )
        .unwrap();
        assert_eq!(
            matches!(
                web,
                Decision::Allow {
                    via: AllowVia::ApprovalMode { .. }
                }
            ),
            mode == ApprovalMode::Broad
        );
    }
}

#[test]
fn approval_mode_failed_receipt_does_not_leave_broad_access_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    wb.db
        .conn()
        .execute_batch(
            "CREATE TEMP TRIGGER reject_mode_receipt BEFORE INSERT ON events
         WHEN json_extract(NEW.payload,'$.kind')='approval_mode_changed'
         BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END;",
        )
        .unwrap();
    assert!(wb.set_approval_mode(ApprovalMode::Broad).is_err());
    assert_eq!(wb.approval_mode().unwrap().mode, ApprovalMode::Restricted);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn approval_mode_never_silently_installs_or_grants_unknown_capabilities(
        mode in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
        kind in prop::sample::select(vec!["skill", "mcp"]),
        allow in any::<bool>(),
    ) {
        // Ticket 05/Q10: fixed L4 used to skip both owner review paths. This does
        // not affect Q7 automatic discovery/loading of already installed skills.
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        wb.set_approval_mode(mode).unwrap();
        let grant = wb.request_grant("a0", kind, "new-capability").unwrap();
        prop_assert!(!grant.granted);
        let count: i64 = wb.db.conn().query_row("SELECT COUNT(*) FROM grants", [], |r| r.get(0)).unwrap();
        prop_assert_eq!(count, 0);
        let confirmed = wb.confirm_grant(grant.question_id.as_deref().unwrap(), allow).unwrap();
        prop_assert_eq!(confirmed.granted, allow);
        let count: i64 = wb.db.conn().query_row("SELECT COUNT(*) FROM grants", [], |r| r.get(0)).unwrap();
        prop_assert_eq!(count, i64::from(allow));
        let install = wb.request_install("npx @mcp/new-capability").unwrap();
        prop_assert!(!dir.path().join(".hexagon/mcp.json").exists());
        let result = crate::install::resolve_install(&wb.db, "p1", dir.path(), &install, allow).unwrap();
        prop_assert_eq!(result.installed, allow);
        prop_assert_eq!(dir.path().join(".hexagon/mcp.json").exists(), allow);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn delayed_approval_mode_never_mutates_a_different_workspace(
        mode in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
    ) {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(second.path(), &["后端"], None).unwrap();
        let expected = first.path().canonicalize().unwrap().to_string_lossy().into_owned();
        let before = wb.message_high_water().unwrap();
        prop_assert!(set_project_approval_mode(&wb.db, &wb.project_id, second.path(), &expected, mode).is_err());
        prop_assert_eq!(wb.approval_mode().unwrap().mode, ApprovalMode::Restricted);
        let changed: i64 = wb.db.conn().query_row(
            "SELECT COUNT(*) FROM events WHERE json_extract(payload,'$.kind')='approval_mode_changed'", [], |r| r.get(0),
        ).unwrap();
        prop_assert_eq!(changed, 0);
        prop_assert_eq!(wb.message_high_water().unwrap(), before);
        let current = second.path().canonicalize().unwrap().to_string_lossy().into_owned();
        let saved = set_project_approval_mode(&wb.db, &wb.project_id, second.path(), &current, mode).unwrap();
        prop_assert_eq!(saved.mode, mode);
        prop_assert_eq!(saved.project_root.as_deref(), Some(current.as_str()));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn child_capabilities_intersect_dispatch_and_current_parent_mode(
        frozen in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
        current in prop::sample::select(vec![ApprovalMode::Restricted, ApprovalMode::Assisted, ApprovalMode::Broad]),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
        wb.set_approval_mode(current).unwrap();
        let parent = wb.ctx_for("a0", None);
        let mut child = wb.ctx_for("a0", None);
        child.subagent = Some(crate::subagent::Scope { approval_mode:frozen, permission_rules:Default::default(), halt:Default::default(), answer:Default::default(), mcp:Default::default(), reads:Default::default() });
        let input = json!({"query":"permission fixture"});
        let parent_result = crate::permissions::evaluate(&wb.db, &parent, &crate::websearch::WebSearch, "web_search", &input).unwrap();
        let child_result = crate::permissions::evaluate(&wb.db, &child, &crate::websearch::WebSearch, "web_search", &input).unwrap();
        let allowed = matches!(child_result, Decision::Allow { .. });
        prop_assert_eq!(allowed, frozen != ApprovalMode::Restricted && current != ApprovalMode::Restricted);
        prop_assert!(!allowed || matches!(parent_result, Decision::Allow { .. }), "child must be parent subset");
    }

    #[test]
    fn remembered_shell_cannot_expand_execution_scope(network in any::<bool>(), background in any::<bool>(), session in any::<bool>(), pattern in any::<bool>()) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
        let ctx = wb.ctx_for("a0", None);
        let shape = if pattern { "npm test *" } else { "npm test --unit" };
        let approved = json!({"cmd":"npm test --unit"});
        crate::permissions::persist_rule(&wb.db, &ctx, "bash", &approved, Some(shape), "project", None).unwrap();
        let mut changed = json!({"cmd":"npm test --unit", "net":network, "background":background});
        if session { changed["session"] = json!("persistent"); }
        let result = crate::permissions::evaluate(&wb.db, &ctx, &crate::tools::Bash, "bash", &changed).unwrap();
        prop_assert_eq!(matches!(result, Decision::Allow { .. }), !network && !background && !session);
        let rules = crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap();
        prop_assert!(!rules[0].network_allowed && !rules[0].background_allowed && rules[0].session_name.is_none());
    }
}

#[test]
fn legacy_memory_is_offline_and_child_memory_is_frozen_but_revocable() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let parent = wb.ctx_for("a0", None);
    wb.db.conn().execute("INSERT INTO permission_rules(id,project_id,tool,shape,effect,scope) VALUES('legacy',?1,'bash','npm test','allow','project')", [&wb.project_id]).unwrap();
    let online = crate::permissions::evaluate(
        &wb.db,
        &parent,
        &crate::tools::Bash,
        "bash",
        &json!({"cmd":"npm test","net":true}),
    )
    .unwrap();
    assert!(matches!(online, Decision::Ask { .. }));
    let mut child = wb.ctx_for("a0", None);
    child.subagent = Some(crate::subagent::Scope {
        approval_mode: ApprovalMode::Restricted,
        permission_rules: std::sync::Arc::new(std::collections::HashSet::from(["legacy".into()])),
        halt: Default::default(),
        answer: Default::default(),
        mcp: Default::default(),
        reads: Default::default(),
    });
    let offline = json!({"cmd":"npm test"});
    assert!(matches!(
        crate::permissions::evaluate(&wb.db, &child, &crate::tools::Bash, "bash", &offline)
            .unwrap(),
        Decision::Allow { .. }
    ));
    wb.db
        .conn()
        .execute("DELETE FROM permission_rules WHERE id='legacy'", [])
        .unwrap();
    assert!(matches!(
        crate::permissions::evaluate(&wb.db, &child, &crate::tools::Bash, "bash", &offline)
            .unwrap(),
        Decision::Ask { .. }
    ));
    crate::permissions::persist_rule(
        &wb.db,
        &parent,
        "bash",
        &offline,
        Some("npm test"),
        "project",
        None,
    )
    .unwrap();
    assert!(matches!(
        crate::permissions::evaluate(&wb.db, &parent, &crate::tools::Bash, "bash", &offline)
            .unwrap(),
        Decision::Allow { .. }
    ));
    assert!(matches!(
        crate::permissions::evaluate(&wb.db, &child, &crate::tools::Bash, "bash", &offline)
            .unwrap(),
        Decision::Ask { .. }
    ));
}
