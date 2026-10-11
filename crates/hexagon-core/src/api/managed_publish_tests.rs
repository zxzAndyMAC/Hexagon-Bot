//! Test3/22: delegated publish to a local bare remote (never a real user
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
    let pack: PackDef = serde_json::from_value(json!({"name":"publish","version":1,
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
        .set("publish/origin", "redacted-test-token")
        .unwrap();
    fx.wb.request_publish("origin").unwrap();
    let status = crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].via.as_str()),
        ("publish", "host"),
        "{done:?}"
    );
    assert_eq!(remote_head(&fx), Some(head(&fx)), "the real push happened");
    let confirmed = fx
        .wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::PublishConfirmed]))
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
fn missing_credential_unaccepted_delivery_or_a_changed_version_never_publish() {
    // Missing credential.
    let mut fx = accepted_project(true);
    fx.wb.request_publish("origin").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("incomplete", "credential_missing")
    );
    assert_eq!(remote_head(&fx), None);
    // Delivery not accepted.
    let mut fx = accepted_project(false);
    fx.wb.creds.set("publish/origin", "t").unwrap();
    fx.wb.request_publish("origin").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].reason, "delivery_not_accepted");
    assert_eq!(remote_head(&fx), None);
    // The delivered commit changed after the request.
    let mut fx = accepted_project(true);
    fx.wb.creds.set("publish/origin", "t").unwrap();
    fx.wb.request_publish("origin").unwrap();
    std::fs::write(fx.work.path().join("f.txt"), "changed").unwrap();
    crate::git::commit_all(fx.work.path(), "later").unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].reason, "delivery_changed");
    assert_eq!(remote_head(&fx), None);
    // A target that is not a configured remote.
    let mut fx = accepted_project(true);
    fx.wb.creds.set("publish/elsewhere", "t").unwrap();
    fx.wb.request_publish("elsewhere").unwrap();
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
    fx.wb.creds.set("publish/origin", "t").unwrap();
    fx.wb.request_publish("origin").unwrap();
    let gone = fx.remote.path().join("missing.git");
    crate::git::run(
        fx.work.path(),
        &["remote", "set-url", "origin", &gone.to_string_lossy()],
    )
    .unwrap();
    crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let done = fx.wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "publish");
    assert!(managed_history(&fx.wb.db, crate::PROJECT_ID, 1).unwrap()[0]
        .outcome
        .starts_with("failed"));
    assert_eq!(
        fx.wb
            .db
            .events(crate::PROJECT_ID, Some(&[EventKind::PublishFailed]))
            .unwrap()
            .len(),
        1
    );
    assert!(
        fx.wb.managed_tick().unwrap().is_empty(),
        "no automatic re-send"
    );
}
