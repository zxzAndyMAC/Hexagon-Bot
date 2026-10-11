//! Test3/21: delegated install and grant are two separate decisions with
//! their own effects; neither stands in for the other, and missing secrets
//! or capabilities end incomplete rather than queueing an owner card.
use super::*;
use crate::provider::ScriptedProvider;
use crate::turn::text_response;

fn replies(wb: &mut Workbench, texts: &[&str]) -> Arc<ScriptedProvider> {
    let chat = Arc::new(ScriptedProvider::new(
        texts.iter().map(|t| text_response(t)).collect(),
    ));
    wb.register_provider("default", chat.clone());
    chat
}

fn grant_rows(wb: &Workbench) -> i64 {
    wb.db
        .conn()
        .query_row("SELECT COUNT(*) FROM grants", [], |r| r.get(0))
        .unwrap()
}

fn local_skill(dir: &Path) -> String {
    let name = format!("managed-skill-{}", std::process::id());
    std::fs::create_dir_all(dir.join(&name)).unwrap();
    std::fs::write(dir.join(&name).join("SKILL.md"), "# managed skill").unwrap();
    name
}

#[test]
fn install_is_a_real_decision_and_the_installed_skill_needs_no_grant_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    let name = local_skill(dir.path());
    wb.request_install(&name).unwrap();
    replies(&mut wb, &["install"]);
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    let first = wb.managed_tick().unwrap();
    assert_eq!(first[0].action, "install", "{first:?}");
    assert!(dir
        .path()
        .join(".hexagon/skills")
        .join(&name)
        .join("SKILL.md")
        .exists());
    assert_eq!(grant_rows(&wb), 0, "an install is not a grant");
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    let installed = wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::InstallCompleted]))
        .unwrap();
    assert_eq!(installed[0].payload["via"], actor.as_str());
    assert_eq!(installed[0].payload["scope"], "project");
    // Ticket 01 (ADR0087) changed the second step: this used to be a
    // separate delegated grant decision that wrote a grants row. Now an
    // installed capability is used directly while delegation is active —
    // still no grant row, so turning delegation off restores the grant card.
    let used = wb.request_grant("a0", "skill", &name).unwrap();
    assert_eq!(
        (used.granted, used.via.as_str()),
        (true, "managed_installed")
    );
    assert!(wb.managed_tick().unwrap().is_empty(), "no grant card");
    assert_eq!(grant_rows(&wb), 0);
    if let Some(home) = std::env::var_os("HOME") {
        assert!(
            !Path::new(&home)
                .join(".hexagon/skills")
                .join(&name)
                .exists(),
            "never global"
        );
    }
}

#[test]
fn a_grant_for_a_missing_capability_ends_incomplete_without_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    wb.request_grant("a0", "skill", "not-installed").unwrap();
    let chat = replies(&mut wb, &["allow"]);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("incomplete", "capability_not_installed")
    );
    assert!(chat.recorded().is_empty());
    assert_eq!(
        grant_rows(&wb),
        0,
        "a grant never stands in for availability"
    );
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
}

#[test]
fn an_install_needing_secrets_ends_incomplete_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    wb.request_install(
        r#"{"name":"vault","command":"node","args":["server.js"],"env":{"API_TOKEN":""}}"#,
    )
    .unwrap();
    let card = &crate::cards::queued(&wb.db, crate::PROJECT_ID).unwrap()[0];
    assert_eq!(card.payload["plan_kind"], "config");
    let chat = replies(&mut wb, &["install"]);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("incomplete", "credentials_or_login_required")
    );
    assert!(chat.recorded().is_empty());
    assert!(!dir.path().join(".hexagon/mcp.json").exists());
}

#[test]
fn an_install_without_a_model_is_rejected_not_queued() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    let name = local_skill(dir.path());
    wb.request_install(&name).unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "reject");
    assert!(!dir.path().join(".hexagon/skills").join(&name).exists());
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
}
