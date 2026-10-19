//! Test3/24 golden path for project delegation, on one real project on disk
//! (git repo + local bare remote, never a user remote): permission →
//! unknown recovery → host acceptance → push to the configured target →
//! reopen → off. No step leaves an owner card; every effect is real.
use super::*;
use crate::provider::ScriptedProvider;
use crate::turn::{text_response, tool_response};
use serde_json::json;

fn script(
    wb: &mut Workbench,
    replies: Vec<crate::provider::ChatResponse>,
) -> Arc<ScriptedProvider> {
    let chat = Arc::new(ScriptedProvider::new(replies));
    wb.register_provider("default", chat.clone());
    chat
}

fn queued(wb: &Workbench) -> usize {
    crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .len()
}

#[cfg(unix)]
#[test]
fn delegation_golden_path_from_permission_to_push_and_reopen() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::git::init(root, "main").unwrap();
    std::fs::write(root.join("README.md"), "golden").unwrap();
    std::fs::write(root.join("locked.txt"), "original").unwrap();
    crate::git::commit_all(root, "seed").unwrap();
    std::fs::set_permissions(
        root.join("locked.txt"),
        std::fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::git::run(remote.path(), &["init", "--bare", "-b", "main"]).unwrap();
    crate::git::run(
        root,
        &["remote", "add", "origin", &remote.path().to_string_lossy()],
    )
    .unwrap();
    let pack: PackDef = serde_json::from_value(json!({"name":"golden","version":1,
        "stages":[{"name":"build","roles":["worker"],"due":[],"checks":["test -f built.txt"],"stamp_point":true}]}))
    .unwrap();
    let roles = [("a0".to_string(), "worker".to_string())];
    let mut wb = Workbench::open_scoped(root, "golden", &roles, Some(pack.clone()), false).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    wb.open_stage(0).unwrap();
    let grant = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let actor = format!(
        "managed:{}:{}",
        grant.delegation_id.unwrap(),
        grant.generation
    );

    // 1. Permission: a real task asks; delegation allows; the effect is real
    //    and the original task continues once.
    script(
        &mut wb,
        vec![tool_response(vec![(
            "c1",
            "bash",
            json!({"cmd":"printf ok > built.txt"}),
        )])],
    );
    let TurnOutcome::AwaitingPermission(_) = wb.run_instance("a0", "build it").unwrap() else {
        panic!("the build asks for permission")
    };
    let chat = script(
        &mut wb,
        vec![text_response("allow"), text_response("built")],
    );
    assert_eq!(wb.managed_tick().unwrap()[0].action, "allow");
    assert_eq!(
        std::fs::read_to_string(root.join("built.txt")).unwrap(),
        "ok"
    );
    assert_eq!(chat.recorded().len(), 2, "decision + one continuation");

    // 2. Unknown outcome: the write to a read-only file fails mid-effect; the
    //    processor closes it (model: abandon) and the task continues.
    script(
        &mut wb,
        vec![
            tool_response(vec![("r", "fs_read", json!({"path":"locked.txt"}))]),
            tool_response(vec![(
                "w",
                "fs_write",
                json!({"path":"locked.txt","content":"edited"}),
            )]),
        ],
    );
    assert!(wb.run_instance("a0", "edit the locked note").is_err());
    let chat = script(
        &mut wb,
        vec![text_response("abandon"), text_response("moved on")],
    );
    let mut actions = vec![];
    for _ in 0..3 {
        actions.extend(wb.managed_tick().unwrap().into_iter().map(|d| d.action));
    }
    assert!(actions.contains(&"abandon".to_string()), "{actions:?}");
    assert!(actions.contains(&"continue".to_string()), "{actions:?}");
    assert!(chat.recorded().len() >= 2);

    // 3. Acceptance: complete current evidence only, by host rule.
    wb.run_checks().unwrap();
    wb.advance().unwrap();
    let accepted = wb.managed_tick().unwrap();
    assert!(
        accepted.iter().any(|d| d.action == "accept"),
        "{accepted:?}"
    );
    let stamped = wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::Stamped]))
        .unwrap();
    assert_eq!(stamped.last().unwrap().payload["by"], actor.as_str());

    // 4. Push to the configured target with its project credential. Stored
    //    under the pre-ADR-0093 name on purpose: legacy credentials keep working.
    crate::git::commit_all(root, "delivery").unwrap();
    wb.creds
        .set("publish/origin", "redacted-test-token")
        .unwrap();
    wb.request_push("origin").unwrap();
    let pushed = wb.managed_tick().unwrap();
    assert_eq!(pushed[0].action, "push", "{pushed:?}");
    let head = crate::git::run(root, &["rev-parse", "main"]).unwrap();
    assert_eq!(
        crate::git::run(remote.path(), &["rev-parse", "main"]).unwrap(),
        head
    );
    assert_eq!(queued(&wb), 0, "no owner card at any step");

    // 5. Reopen: delegation persists; turning it off hands control back.
    drop(wb);
    let wb = Workbench::open_scoped(root, "golden", &roles, Some(pack), false).unwrap();
    assert!(crate::delegation::current(&wb.db, crate::PROJECT_ID).is_some());
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(managed_processing(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
    let report = managed_history(&wb.db, crate::PROJECT_ID, 50).unwrap();
    assert!(report.iter().all(|r| r.actor.starts_with("managed:")));
    assert!(report.iter().any(|r| r.result == "done"));
    let trace = serde_json::to_string(&wb.db.events(crate::PROJECT_ID, None).unwrap()).unwrap();
    assert!(
        !trace.contains("redacted-test-token"),
        "secrets never reach the trace"
    );
}
