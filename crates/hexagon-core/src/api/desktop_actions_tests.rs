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
