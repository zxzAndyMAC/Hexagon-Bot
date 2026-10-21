//! owner-followups-1015 #13 (ADR 0093 follow-up ruling 2026-10-17): the
//! Agent's terminal `git push` and the owner's ⌘K push share one push card
//! and one remembered rule per (remote, branch); force pushes stay a safety
//! net (always asked, never remembered, never answered by delegation).
//! Remotes are local bare repositories — never a real user remote.
use super::*;
use serde_json::json;

struct Fixture {
    work: tempfile::TempDir,
    origin: tempfile::TempDir,
    _backup: tempfile::TempDir,
    wb: Workbench,
}

fn bare(work: &std::path::Path, name: &str) -> tempfile::TempDir {
    let remote = tempfile::tempdir().unwrap();
    crate::git::run(remote.path(), &["init", "--bare", "-b", "main"]).unwrap();
    crate::git::run(
        work,
        &["remote", "add", name, &remote.path().to_string_lossy()],
    )
    .unwrap();
    remote
}

fn fixture() -> Fixture {
    let work = tempfile::tempdir().unwrap();
    crate::git::init(work.path(), "main").unwrap();
    std::fs::write(work.path().join("f.txt"), "x").unwrap();
    crate::git::commit_all(work.path(), "init").unwrap();
    let origin = bare(work.path(), "origin");
    let backup = bare(work.path(), "backup");
    let wb = Workbench::for_test(work.path(), &["后端"], None).unwrap();
    wb.creds.set("push/origin", "redacted-test-token").unwrap();
    wb.creds.set("push/backup", "redacted-test-token").unwrap();
    Fixture {
        work,
        origin,
        _backup: backup,
        wb,
    }
}

fn agent_push(wb: &Workbench, cmd: &str) -> crate::tools::CallOutcome {
    wb.registry
        .call(&wb.db, &wb.ctx_for("a0", None), "bash", json!({"cmd": cmd}))
        .unwrap()
}

fn asked(wb: &Workbench, cmd: &str) -> Option<crate::cards::Card> {
    match agent_push(wb, cmd) {
        crate::tools::CallOutcome::Asked(id) => Some(crate::cards::get(&wb.db, &id).unwrap()),
        _ => None,
    }
}

fn remote_main(dir: &std::path::Path) -> Option<String> {
    crate::git::run(dir, &["rev-parse", "main"])
        .ok()
        .map(|s| s.trim().to_string())
}

#[test]
fn agent_push_is_a_push_card_remembered_per_remote_and_branch() {
    let fx = fixture();
    let wb = &fx.wb;
    let card = asked(wb, "git push origin main").expect("an unremembered push asks");
    assert_eq!(
        card.payload["safety_net"], false,
        "ordinary push is not a safety net"
    );
    assert_eq!(card.payload["reason"], "push: origin main");
    assert_eq!(
        card.payload["push"],
        json!({"force": false, "remote": "origin", "branch": "main"})
    );
    let proposal = permission_shape_suggestion(&wb.db, &wb.project_id, &card.id)
        .unwrap()
        .expect("an ordinary push can be allowed for the project");
    assert_eq!(proposal.shape, "push:origin main");
    wb.allow_project_permission(&card.id).unwrap();

    // Same remote, same branch: no more card (options do not change the key).
    assert!(asked(wb, "git push origin main").is_none());
    assert!(asked(wb, "git push -u origin HEAD:main").is_none());
    // A different branch or remote still asks.
    assert!(asked(wb, "git push origin feature").is_some());
    assert!(asked(wb, "git push backup main").is_some());
    // The owner's ⌘K push to the same target uses the same rule: no card, pushed.
    let req = wb.request_push("origin").unwrap();
    assert!(req.pushed.is_some(), "{req:?}");
    assert_eq!(
        remote_main(fx.origin.path()),
        remote_main(fx.work.path()),
        "the real push happened"
    );
    let confirmed = wb
        .db
        .events(&wb.project_id, Some(&[EventKind::PushConfirmed]))
        .unwrap();
    assert!(confirmed[0].payload["by"]
        .as_str()
        .unwrap()
        .starts_with("project_permission:"));
}

#[test]
fn owner_remembers_on_the_push_card_for_cmdk_and_agent_alike() {
    let fx = fixture();
    let wb = &fx.wb;
    let first = wb.request_push("origin").unwrap();
    assert!(first.pushed.is_none(), "default: every push asks");
    wb.confirm_push(&first.question_id, true).unwrap();
    assert_eq!(remote_main(fx.origin.path()), remote_main(fx.work.path()));
    let rules = crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        (rules[0].shape.as_str(), rules[0].project_shared),
        ("push:origin main", true)
    );
    assert_eq!(
        wb.db
            .events(
                &wb.project_id,
                Some(&[EventKind::PermissionShapeRemembered])
            )
            .unwrap()
            .len(),
        1
    );

    let again = wb.request_push("origin").unwrap();
    assert!(again.pushed.is_some(), "remembered: no card");
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
    assert!(
        asked(wb, "git push origin main").is_none(),
        "Agent shares the rule"
    );
    let other = wb.request_push("backup").unwrap();
    assert!(other.pushed.is_none(), "another remote still asks");

    // A once-only confirm remembers nothing.
    let fx = fixture();
    let once = fx.wb.request_push("origin").unwrap();
    fx.wb.confirm_push(&once.question_id, false).unwrap();
    assert!(fx.wb.request_push("origin").unwrap().pushed.is_none());
}

#[test]
fn force_push_always_asks_is_never_remembered_and_delegation_leaves_it() {
    let fx = fixture();
    let wb = &fx.wb;
    crate::permissions::remember_push(&wb.db, &wb.project_id, "origin", "main").unwrap();
    for cmd in [
        "git push --force origin main",
        "git push -f origin main",
        "git push --force-with-lease origin main",
        "git push origin +main",
        "git push origin :main",
        "git push",
    ] {
        let card = asked(wb, cmd).unwrap_or_else(|| panic!("{cmd} must ask"));
        assert_eq!(card.payload["safety_net"], true, "{cmd}");
        assert_eq!(card.payload["push"]["force"], true, "{cmd}");
        assert!(
            permission_shape_suggestion(&wb.db, &wb.project_id, &card.id)
                .unwrap()
                .is_none(),
            "{cmd}: no remember option"
        );
        assert!(wb.allow_project_permission(&card.id).is_err(), "{cmd}");
    }
    // A broad command shape cannot cover pushes now that they left the safety net.
    wb.db.conn().execute(
        "INSERT INTO permission_rules(id,project_id,agent_id,tool,shape,effect,scope) VALUES('wide',?1,'a0','bash','git *','allow','project')",
        [&wb.project_id],
    ).unwrap();
    assert!(asked(wb, "git push origin feature").is_some());

    // Delegation answers the ordinary push card but leaves force pushes to the owner.
    let fx = fixture();
    let wb = &fx.wb;
    let ordinary = asked(wb, "git push origin main").unwrap();
    let force = asked(wb, "git push --force origin main").unwrap();
    crate::delegation::enable(&wb.db, &wb.project_id).unwrap();
    let owner: Vec<String> = owner_pending(&wb.db, &wb.project_id)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(
        owner,
        vec![force.id.clone()],
        "only the force push stays the owner's"
    );
    let planned: Vec<String> = wb
        .managed_plan()
        .unwrap()
        .into_iter()
        .map(|c| c.card_id)
        .collect();
    assert!(planned.contains(&ordinary.id), "{planned:?}");
    assert!(!planned.contains(&force.id), "{planned:?}");
}

#[test]
fn broad_access_does_not_release_an_ordinary_push_without_a_card() {
    let fx = fixture();
    let wb = &fx.wb;
    crate::approval_mode::set(
        &wb.db,
        &wb.project_id,
        crate::approval_mode::ApprovalMode::Broad,
    )
    .unwrap();
    assert!(asked(wb, "git push origin main").is_some());
    assert_eq!(remote_main(fx.origin.path()), None);
}

#[test]
fn push_targets_follow_the_baseline_upstream_or_let_the_owner_choose() {
    let fx = fixture();
    let wb = &fx.wb;
    let t = wb.push_targets().unwrap();
    assert_eq!(t.baseline, "main");
    assert_eq!(
        t.upstream, None,
        "no upstream: the owner chooses, no origin guess"
    );
    assert_eq!(t.remotes, vec!["backup".to_string(), "origin".to_string()]);
    crate::git::run(fx.work.path(), &["config", "branch.main.remote", "backup"]).unwrap();
    assert_eq!(
        wb.push_targets().unwrap().upstream.as_deref(),
        Some("backup")
    );
    // An upstream naming a remote that no longer exists is no upstream.
    crate::git::run(fx.work.path(), &["config", "branch.main.remote", "gone"]).unwrap();
    assert_eq!(wb.push_targets().unwrap().upstream, None);
}
