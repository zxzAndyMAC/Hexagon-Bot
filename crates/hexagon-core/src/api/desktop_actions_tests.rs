use super::*;
use crate::desktop::DesktopControl;
use base64::Engine as _;

#[test]
fn screenshot_consent_is_project_persistent_and_does_not_authorize_new_projects() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(first.path(), &["QA"], None).unwrap();
    let database_path = first.path().join("consent.db");
    wb.db = Db::open(&database_path).unwrap();
    wb.db
        .conn()
        .execute(
            "INSERT INTO projects(id,dir,name,mode,autonomy) VALUES(?1,?2,'desktop','pack','L0')",
            rusqlite::params![crate::PROJECT_ID, first.path().display().to_string()],
        )
        .unwrap();
    assert!(!desktop_status(&wb.db, first.path()).unwrap().enabled);
    assert!(
        desktop_control(&wb.db, first.path(), DesktopControl::Enable)
            .unwrap()
            .enabled
    );
    let saved: bool = wb.db.conn().query_row("SELECT json_extract(payload,'$.screen_to_selected_model') FROM events WHERE json_extract(payload,'$.kind')='computer_control' ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).unwrap();
    assert!(saved);
    drop(wb);
    let reopened = Db::open(&database_path).unwrap();
    assert!(desktop_status(&reopened, first.path()).unwrap().enabled);
    let independent = Workbench::for_test(second.path(), &["QA"], None).unwrap();
    assert!(
        !desktop_status(&independent.db, second.path())
            .unwrap()
            .enabled
    );
    assert!(
        !desktop_control(&reopened, first.path(), DesktopControl::Disable)
            .unwrap()
            .enabled
    );
    // Release the explicit test pause without ever invoking the native backend.
    desktop_control(&reopened, first.path(), DesktopControl::Release).unwrap();
}

#[test]
fn owner_screenshot_viewer_rejects_paths_aliases_and_cleared_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jY9sAAAAASUVORK5CYII=").unwrap();
    let evidence = dir.path().join(".hexagon/computer-use/screenshots");
    std::fs::create_dir_all(&evidence).unwrap();
    let saved = evidence
        .join("0123456789abcdef0123456789abcdef.png")
        .display()
        .to_string();
    std::fs::write(&saved, png).unwrap();
    let name = Path::new(&saved)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let expected_root = dir.path().canonicalize().unwrap().display().to_string();
    // Issue 11: a delayed old-workspace thumbnail must never read the current
    // workspace's same-named file, even though both project IDs equal p1.
    assert!(desktop_screenshot(&wb.db, dir.path(), &name, "/old-project").is_err());
    let result = desktop_screenshot(&wb.db, dir.path(), &name, &expected_root).unwrap();
    assert_eq!(result.name, name);
    assert!(result.data_url.starts_with("data:image/png;base64,"));
    for invalid in [format!("../{name}"), saved.clone(), "state.db".into()] {
        assert!(desktop_screenshot(&wb.db, dir.path(), &invalid, &expected_root).is_err());
    }
    #[cfg(unix)]
    {
        let link_name = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";
        std::os::unix::fs::symlink(&saved, Path::new(&saved).with_file_name(link_name)).unwrap();
        assert!(desktop_screenshot(&wb.db, dir.path(), link_name, &expected_root).is_err());
    }
    std::fs::remove_file(&saved).unwrap();
    assert!(desktop_screenshot(&wb.db, dir.path(), &name, &expected_root).is_err());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn queued_desktop_control_cannot_authorize_a_different_project(suffix in "[a-z]{1,20}") {
        let original = tempfile::tempdir().unwrap();
        let current = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(current.path(), &["QA"], None).unwrap();
        let status = desktop_status(&wb.db, current.path()).unwrap();
        proptest::prop_assert!(desktop_check_project(current.path(), &status.project_root).is_ok());
        for stale in [original.path().canonicalize().unwrap().display().to_string(), format!("{}/{}", status.project_root, suffix)] {
            let result = desktop_check_project(current.path(), &stale)
                .and_then(|()| desktop_control(&wb.db, current.path(), DesktopControl::Enable));
            proptest::prop_assert!(result.is_err());
            proptest::prop_assert!(!desktop_status(&wb.db, current.path()).unwrap().enabled);
        }
    }
}

/// ui-redesign gap D6 (2026-10-16): the computer_action card carries the
/// model's reason and a reference to the observation it was planned on. Both
/// are display-only: absent or stale evidence leaves the card asking, never
/// blocked and never auto-approved (safety net asks on the tool name alone).
#[test]
fn computer_action_card_carries_reason_and_planned_on_screenshot() {
    let card = |seed_observation: bool, keep_file: bool, snapshot: &str, reason: Option<&str>| {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["QA"], None).unwrap();
        let ctx = wb.ctx_for("a0", None);
        let evidence = dir.path().join(".hexagon/computer-use/screenshots");
        std::fs::create_dir_all(&evidence).unwrap();
        let saved = evidence.join("0123456789abcdef0123456789abcdef.png");
        std::fs::write(&saved, b"png").unwrap();
        let path = saved.display().to_string();
        if seed_observation {
            let output = json!({"ok":true,"result":{"snapshot_id":"snap-1","screenshot_path":path,
                "title":"Privacy","bundle_id":"com.apple.systempreferences"},
                "image":{"media_type":"image/png","omitted":"local screenshot; observe again for fresh pixels"}});
            wb.db.conn().execute(
                "INSERT INTO tool_actions(id,identity,project_id,agent_id,tool_call_id,tool,input_json,input_digest,state,output_json)
                 VALUES('obs','obs',?1,'a0','call','computer_observe','{}','d','succeeded',?2)",
                rusqlite::params![crate::PROJECT_ID, output.to_string()],
            ).unwrap();
        }
        if !keep_file {
            std::fs::remove_file(&saved).unwrap();
        }
        let mut input = json!({"op":"click","snapshot_id":snapshot,"x":412,"y":288});
        if let Some(reason) = reason {
            input["reason"] = json!(reason);
        }
        let outcome = wb
            .registry
            .call(&wb.db, &ctx, "computer_action", input)
            .unwrap();
        let crate::tools::CallOutcome::Asked(qid) = outcome else {
            panic!("computer_action must always ask: {outcome:?}");
        };
        (crate::cards::get(&wb.db, &qid).unwrap().payload, path)
    };

    let (payload, path) = card(
        true,
        true,
        "snap-1",
        Some("verify the screen-recording prompt"),
    );
    assert_eq!(payload["screenshot"]["screenshot_path"], json!(path));
    assert_eq!(
        payload["screenshot"]["bundle_id"],
        "com.apple.systempreferences"
    );
    assert_eq!(
        payload["input"]["reason"],
        "verify the screen-recording prompt"
    );
    // The permission layer's own reason is unchanged by the model's reason.
    assert_eq!(
        payload["reason"],
        "safety net: desktop action with unverified external effects"
    );
    assert_eq!(payload["safety_net"], true);

    // Fail toward "no thumbnail": no observation, a different snapshot, or
    // cleared evidence still asks — the card is never blocked by missing pixels.
    for (seed, keep, snapshot) in [
        (false, true, "snap-1"),
        (true, true, "snap-2"),
        (true, false, "snap-1"),
    ] {
        let (payload, _) = card(seed, keep, snapshot, None);
        assert!(payload["screenshot"].is_null(), "{seed} {keep} {snapshot}");
        assert!(payload["input"].get("reason").is_none());
    }
}

#[test]
fn computer_action_reason_never_reaches_the_native_request() {
    let tool = crate::tools::desktop::ComputerAction;
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["QA"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    // NativeRequest is deny_unknown_fields: accepting the call proves the
    // reason was stripped before the ABI request was built.
    assert!(crate::tools::Tool::precondition(
        &tool,
        &json!({"op":"activate","app_id":"com.apple.Safari","reason":"open the docs"}),
        &ctx
    )
    .is_ok());
    // Read tools never declare it.
    let schema = crate::tools::Tool::input_schema(&crate::tools::desktop::ComputerObserve);
    assert!(schema["properties"].get("reason").is_none());
}
