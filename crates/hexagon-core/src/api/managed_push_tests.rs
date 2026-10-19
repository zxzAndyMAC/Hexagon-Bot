//! Test3/22: delegated push (ex-publish, ADR 0093) to a local bare remote (never a real user
//! remote). Host facts decide; the push result, not a model's approval, is
//! what the record shows, and a failure is never silently re-sent.
use super::*;
use serde_json::json;

struct Fixture {
    work: tempfile::TempDir,
    remote: tempfile::TempDir,
    wb: Workbench,
}

fn accepted_project(accept: bool) -> Fixture {
    let work = tempfile::tempdir().unwrap();
    crate::git::init(work.path(), "main").unwrap();
    std::fs::write(work.path().join("f.txt"), "x").unwrap();
    crate::git::commit_all(work.path(), "init").unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::git::run(remote.path(), &["init", "--bare", "-b", "main"]).unwrap();
    crate::git::run(
        work.path(),
        &["remote", "add", "origin", &remote.path().to_string_lossy()],
    )
    .unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"push","version":1,
        "stages":[{"name":"ship","roles":["worker"],"due":[],"checks":["true"],"stamp_point":true}]}))
    .unwrap();
    let wb = Workbench::for_test(work.path(), &["worker"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    if accept {
        wb.run_checks().unwrap();
        wb.advance().unwrap();
        wb.stamp().unwrap();
    }
    Fixture { work, remote, wb }
}

fn remote_head(fx: &Fixture) -> Option<String> {
    crate::git::run(fx.remote.path(), &["rev-parse", "main"])
        .ok()
        .map(|s| s.trim().to_string())
}

fn head(fx: &Fixture) -> String {
    crate::git::run(fx.work.path(), &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn an_accepted_delivery_is_pushed_to_the_configured_target() {
    let mut fx = accepted_project(true);
    fx.wb
        .creds
        .set("push/origin", "redacted-test-token")
        .unwrap();
    fx.wb.request_push("origin").unwrap();
    let status = crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].via.as_str()),
        ("push", "host"),
        "{done:?}"
    );
    assert_eq!(remote_head(&fx), Some(head(&fx)), "the real push happened");
    let confirmed = fx
        .wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::PushConfirmed]))
        .unwrap();
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(confirmed[0].payload["by"], actor.as_str());
    let log = serde_json::to_string(&fx.wb.db.events(crate::PROJECT_ID, None).unwrap()).unwrap();
    assert!(
        !log.contains("redacted-test-token"),
        "the secret never reaches the trace"
    );
}

#[test]
fn missing_credential_unaccepted_delivery_or_a_changed_version_never_push() {
    // Missing credential.
    let mut fx = accepted_project(true);
    fx.wb.request_push("origin").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("incomplete", "credential_missing")
    );
    assert_eq!(remote_head(&fx), None);
    // Delivery not accepted.
    let mut fx = accepted_project(false);
    fx.wb.creds.set("push/origin", "t").unwrap();
    fx.wb.request_push("origin").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].reason, "delivery_not_accepted");
    assert_eq!(remote_head(&fx), None);
    // The delivered commit changed after the request.
    let mut fx = accepted_project(true);
    fx.wb.creds.set("push/origin", "t").unwrap();
    fx.wb.request_push("origin").unwrap();
    std::fs::write(fx.work.path().join("f.txt"), "changed").unwrap();
    crate::git::commit_all(fx.work.path(), "later").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].reason, "delivery_changed");
    assert_eq!(remote_head(&fx), None);
    // A target that is not a configured remote.
    let mut fx = accepted_project(true);
    fx.wb.creds.set("push/elsewhere", "t").unwrap();
    fx.wb.request_push("elsewhere").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    assert_eq!(fx.wb.managed_tick().unwrap()[0].reason, "target_missing");
    assert!(
        crate::cards::queued(&fx.wb.db, crate::PROJECT_ID)
            .unwrap()
            .is_empty(),
        "no owner card"
    );
}

#[test]
fn a_failed_push_is_recorded_and_never_silently_resent() {
    let mut fx = accepted_project(true);
    fx.wb.creds.set("push/origin", "t").unwrap();
    fx.wb.request_push("origin").unwrap();
    let gone = fx.remote.path().join("missing.git");
    crate::git::run(
        fx.work.path(),
        &["remote", "set-url", "origin", &gone.to_string_lossy()],
    )
    .unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "push");
    assert!(managed_history(&fx.wb.db, crate::PROJECT_ID, 1).unwrap()[0]
        .outcome
        .starts_with("failed"));
    assert_eq!(
        fx.wb
            .db
            .events(crate::PROJECT_ID, Some(&[EventKind::PushFailed]))
            .unwrap()
            .len(),
        1
    );
    assert!(
        fx.wb.managed_tick().unwrap().is_empty(),
        "no automatic re-send"
    );
}

/// ADR 0093 / migration 0064: managed records and budgets written before
/// the rename read back as push — same result, same evidence, same budget.
/// (Card kinds and event kinds: `db::tests::adr0093_publish_rows_migrate_to_push`.)
#[test]
fn adr0093_pre_rename_managed_records_read_back_as_push() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let old = crate::db::Db::create_migrated_until(&path, "0063_agent_removed");
    old.execute_batch(
        r#"INSERT INTO projects (id, dir, name, mode) VALUES ('p', '/tmp/x', 'x', 'pack');
        INSERT INTO events (project_id, kind, payload) VALUES
          ('p', 'system', '{"kind":"managed_decision","card_id":"q2","card_kind":"publish","action":"publish","reason":"configured_target_accepted_delivery","via":"host","actor":"managed:g:1","outcome":"published"}'),
          ('p', 'system', '{"kind":"managed_decision","card_id":"q2","card_kind":"publish","action":"incomplete","reason":"target_missing","via":"host","actor":"managed:g:1","outcome":"not_published"}'),
          ('p', 'system', '{"kind":"managed_decision","card_id":"q3","card_kind":"permission","action":"allow","reason":"in_scope","via":"host","actor":"managed:g:1","outcome":"allowed"}');
        INSERT INTO managed_budgets (project_id, decision_key, rounds) VALUES ('p', 'publish:origin:abc', 2);"#,
    )
    .unwrap();
    drop(old);

    let db = crate::db::Db::open(&path).unwrap();
    let history = managed_history(&db, "p", 10).unwrap();
    let push: Vec<_> = history.iter().filter(|r| r.card_kind == "push").collect();
    assert_eq!(push.len(), 2, "{history:?}");
    let landed = push.iter().find(|r| r.action == "push").unwrap();
    assert_eq!(
        (landed.outcome.as_str(), landed.result.as_str()),
        ("pushed", "done")
    );
    assert_eq!(landed.evidence, report::ManagedEvidence::HostVerified);
    let refused = push.iter().find(|r| r.action == "incomplete").unwrap();
    assert_eq!(
        (refused.outcome.as_str(), refused.result.as_str()),
        ("not_pushed", "incomplete")
    );
    assert!(history
        .iter()
        .any(|r| r.card_kind == "permission" && r.action == "allow"));
    assert_eq!(crate::delegation::rounds(&db, "p", "push:origin:abc"), 2);
    assert_eq!(crate::delegation::rounds(&db, "p", "publish:origin:abc"), 0);
}
