use super::*;
use proptest::prelude::*;
use serde_json::json;
fn option(id: &str, fill: &str) -> crate::design::DesignOptionInput {
    crate::design::DesignOptionInput {
        id: id.into(),
        title: format!("Direction {id}"),
        description: "Readable dashboard".into(),
        layout: "Sidebar and content".into(),
        typography: "System sans".into(),
        palette: fill.into(),
        mockups: vec![crate::design::DesignMockupInput {
            page: "Dashboard".into(),
            mime: "image/svg+xml".into(),
            content: format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 600"><rect width="800" height="600" fill="{fill}"/></svg>"#
            ),
        }],
    }
}
fn propose(
    wb: &Workbench,
    ctx: &crate::tools::ToolContext,
    revision: i64,
    options: Vec<crate::design::DesignOptionInput>,
) -> Result<crate::design::DesignDirection, ApiError> {
    match wb.registry.call(
        &wb.db,
        ctx,
        "propose_design",
        json!({"expected_revision":revision,"options":options}),
    )? {
        crate::tools::CallOutcome::Done(_) => design_direction(&wb.db, &wb.project_id),
        _ => Err(ApiError::BadInput(
            "design proposal was not executed".into(),
        )),
    }
}
#[test]
fn design_owner_choice_binds_revision_and_wakes_role_with_durable_brief() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["UI/UX", "前端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    let requested = crate::design::require(&wb.db, &wb.project_id, "a1").unwrap();
    crate::orchestra::write_agent_status(&wb.db, &wb.project_id, "a1", true).unwrap();
    let first = propose(
        &wb,
        &ctx,
        requested.revision,
        vec![option("a", "red"), option("b", "blue")],
    )
    .unwrap();
    assert!(!crate::design::confirmed(&wb.db, &wb.project_id).unwrap());
    let next = propose(
        &wb,
        &ctx,
        first.revision,
        vec![option("a", "orange"), option("b", "green")],
    )
    .unwrap();
    assert!(choose_design_direction(
        &wb.db,
        &wb.project_id,
        first.question_id.as_deref().unwrap(),
        first.revision,
        Some("a"),
        None
    )
    .is_err());
    crate::orchestra::write_agent_status(&wb.db, &wb.project_id, "a0", true).unwrap();
    let selected = choose_design_direction(
        &wb.db,
        &wb.project_id,
        next.question_id.as_deref().unwrap(),
        next.revision,
        Some("b"),
        None,
    )
    .unwrap();
    assert_eq!(selected.selected_option.as_deref(), Some("b"));
    assert_eq!(
        selected.options[1].mockups[0].digest,
        next.options[1].mockups[0].digest
    );
    assert!(crate::design::confirmed(&wb.db, &wb.project_id).unwrap());
    assert_eq!(
        wb.db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a0'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "active"
    );
    assert!(propose(
        &wb,
        &ctx,
        next.revision,
        vec![option("a", "red"), option("b", "blue")]
    )
    .is_err());
    assert_eq!(
        wb.db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a1'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "active",
        "owner choice must also wake the implementer who requested designs"
    );
    let brief = crate::turn::build_brief_context(&wb.db, "a1", None).unwrap();
    let notice = brief
        .notices
        .iter()
        .find(|v| v["kind"] == "design_direction")
        .unwrap();
    assert_eq!(notice["payload"]["selected"]["id"], "b");
    assert!(!notice.to_string().contains("data:image"));
    let path = dir.path().join("design-reopen.db");
    wb.db
        .conn()
        .execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])
        .unwrap();
    drop(wb);
    let reopened = crate::db::Db::open(path).unwrap();
    assert_eq!(
        crate::design::read(&reopened, &selected.project_id)
            .unwrap()
            .selected_option,
        Some("b".into())
    );
}
#[test]
fn design_source_gate_allows_backend_and_owner_confirmed_existing_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["前端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    assert!(
        crate::design::guard_write(&wb.db, &ctx, "fs_write", &json!({"path":"src/server.rs"}))
            .unwrap()
            .is_none()
    );
    assert!(crate::design::guard_write(
        &wb.db,
        &ctx,
        "fs_write",
        &json!({"path":"ui/node_modules/lib/style.css"})
    )
    .unwrap()
    .is_none());
    assert!(crate::design::guard_write(
        &wb.db,
        &ctx,
        "fs_write",
        &json!({"path":"ui/src/App.tsx"})
    )
    .unwrap()
    .is_some());
    let pending = design_direction(&wb.db, &wb.project_id).unwrap();
    assert!(pending.options.is_empty());
    choose_design_direction(
        &wb.db,
        &wb.project_id,
        pending.question_id.as_deref().unwrap(),
        pending.revision,
        None,
        Some("Follow the owner's existing brand guide v3"),
    )
    .unwrap();
    assert!(crate::design::guard_write(
        &wb.db,
        &ctx,
        "fs_write",
        &json!({"path":"ui/src/App.tsx"})
    )
    .unwrap()
    .is_none());
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]
    #[test]
    fn design_rejects_active_or_external_svg_payloads(suffix in "[a-z]{1,20}", kind in 0..5usize) {
        let dir=tempfile::tempdir().unwrap();
        let wb=Workbench::for_test(dir.path(), &["UI/UX"],None).unwrap();
        let ctx=wb.ctx_for("a0",None);
        let mut bad=option("a","red");
        let active=match kind {0=>format!("<script>{suffix}</script>"),1=>format!(r#"<image href="https://example.com/{suffix}"/>"#),2=>format!("<foreignObject>{suffix}</foreignObject>"),3=>format!(r#"<rect onload="{suffix}"/>"#),_=>format!(r#"<rect fill="URL(//example.com/{suffix})"/>"#)};
        bad.mockups[0].content=bad.mockups[0].content.replace("</svg>",&format!("{active}</svg>"));
        prop_assert!(propose(&wb,&ctx,0,vec![bad,option("b","blue")]).is_err());
        prop_assert_eq!(design_direction(&wb.db,&wb.project_id).unwrap().revision,0);
    }
    #[test]
    fn design_never_accepts_an_unproposed_option(id in "z[a-z0-9]{1,20}") {
        let dir=tempfile::tempdir().unwrap();
        let wb=Workbench::for_test(dir.path(), &["前端"],None).unwrap();
        let ctx=wb.ctx_for("a0",None);
        let proposal=propose(&wb,&ctx,0,vec![option("a","red"),option("b","blue")]).unwrap();
        prop_assert!(choose_design_direction(&wb.db,&wb.project_id,proposal.question_id.as_deref().unwrap(),proposal.revision,Some(&id),None).is_err());
        prop_assert!(!crate::design::confirmed(&wb.db,&wb.project_id).unwrap());
    }
}

#[test]
#[cfg(target_os = "macos")]
fn design_shell_source_gate_preserves_dependency_and_backend_writes() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["前端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    // Spec review P2: monorepo source paths need the same real OS gate as
    // structured writes, while dependencies and compiled assets remain writable.
    for path in [
        "ui/src",
        "ui/node_modules/example",
        "ui/dist",
        "src",
        "apps/site/src",
        "apps/site/node_modules/example",
        "packages/web/src",
        "packages/web/dist",
    ] {
        std::fs::create_dir_all(ctx.repo_root.join(path)).unwrap();
    }
    let run = |cmd: &str| {
        ctx.sessions
            .run_oneshot(&wb.db, &ctx, cmd, std::time::Duration::from_secs(10), false)
            .unwrap()
    };
    assert_eq!(run("printf ok > src/server.rs")["exit_code"], 0);
    assert_eq!(
        run("printf ok > ui/node_modules/example/style.css")["exit_code"],
        0
    );
    assert_eq!(run("printf ok > ui/dist/style.css")["exit_code"], 0);
    assert_ne!(run("printf before > ui/src/App.tsx")["exit_code"], 0);
    assert!(!ctx.repo_root.join("ui/src/App.tsx").exists());
    for (cmd, path) in [
        (
            "printf before > apps/site/src/App.tsx",
            "apps/site/src/App.tsx",
        ),
        (
            "printf before > packages/web/src/App.tsx",
            "packages/web/src/App.tsx",
        ),
    ] {
        assert_ne!(run(cmd)["exit_code"], 0);
        assert!(!ctx.repo_root.join(path).exists());
    }
    assert_eq!(
        run("printf dependency > apps/site/node_modules/example/style.css")["exit_code"],
        0
    );
    assert_eq!(
        run("printf compiled > packages/web/dist/style.css")["exit_code"],
        0
    );
    let proposal = propose(&wb, &ctx, 0, vec![option("a", "red"), option("b", "blue")]).unwrap();
    choose_design_direction(
        &wb.db,
        &wb.project_id,
        proposal.question_id.as_deref().unwrap(),
        proposal.revision,
        Some("a"),
        None,
    )
    .unwrap();
    assert_eq!(run("printf after > ui/src/App.tsx")["exit_code"], 0);
    assert_eq!(
        run("printf after > packages/web/src/App.tsx")["exit_code"],
        0
    );
    assert_eq!(
        std::fs::read_to_string(ctx.repo_root.join("ui/src/App.tsx")).unwrap(),
        "after"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]
    #[test]
    fn design_visual_source_gate_never_infers_owner_approval(stem in "[A-Za-z]{1,24}", ext in prop::sample::select(vec!["tsx","jsx","css","scss","html","vue","svelte"])) {
        let dir=tempfile::tempdir().unwrap();
        let wb=Workbench::for_test(dir.path(), &["前端"],None).unwrap();
        let ctx=wb.ctx_for("a0",None);
        let input=json!({"path":format!("ui/src/{stem}.{ext}"),"owner_approved":true});
        prop_assert!(crate::design::guard_write(&wb.db,&ctx,"fs_write",&input).unwrap().is_some());
        prop_assert!(!crate::design::confirmed(&wb.db,&wb.project_id).unwrap());
        let ignored=json!({"path":format!("ui/node_modules/pkg/{stem}.{ext}")});
        prop_assert!(crate::design::guard_write(&wb.db,&ctx,"fs_write",&ignored).unwrap().is_none());
    }
}
