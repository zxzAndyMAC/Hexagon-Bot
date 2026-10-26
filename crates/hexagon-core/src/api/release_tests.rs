//! 发布执行门面（owner-followups-1015 #15）：发布确认卡必问、永不记忆；缺凭据 / 目标被改 / 目标已删
//! fail-closed 且什么都不跑；执行结果落轨迹且凭据值不进轨迹；托管可确认但不能登记 / 修改目标。
//! 凭据库是 `Workbench::for_test` 的内存接缝，不碰系统钥匙串；发布命令只是临时目录里的无害本地
//! shell 片段（写一个标记文件 / 回显环境变量），从不做真实发布。
use super::*;
use crate::credentials::leak_scan;
use crate::errcode::ErrorCode;
use crate::release_targets::{credential_key, ReleaseTargetDraft};

const SECRET: &str = "fake-token-7f3a9c";

struct Fixture {
    dir: tempfile::TempDir,
    wb: Workbench,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    Fixture { dir, wb }
}

fn register(fx: &Fixture, name: &str, command: &str, creds: &[&str]) -> String {
    fx.wb
        .save_release_target(&ReleaseTargetDraft {
            id: None,
            name: name.into(),
            command: command.into(),
            cwd: String::new(),
            credentials: creds.iter().map(|c| c.to_string()).collect(),
        })
        .unwrap()
        .id
}

fn marker(fx: &Fixture, file: &str) -> bool {
    fx.dir.path().join(file).exists()
}

fn kinds(fx: &Fixture, kind: EventKind) -> Vec<serde_json::Value> {
    fx.wb
        .db
        .events(crate::PROJECT_ID, Some(&[kind]))
        .unwrap()
        .into_iter()
        .map(|e| e.payload)
        .collect()
}

fn queued_release(fx: &Fixture) -> Vec<String> {
    crate::cards::queued(&fx.wb.db, crate::PROJECT_ID)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == "release")
        .map(|c| c.id)
        .collect()
}

fn code(e: ApiError) -> String {
    e.code()
}

#[test]
fn release_card_always_asks_and_is_never_remembered() {
    let fx = fixture();
    let id = register(&fx, "pages", "printf done > released.txt", &[]);
    let rules_before: i64 = fx
        .wb
        .db
        .conn()
        .query_row("SELECT COUNT(*) FROM permission_rules", [], |r| r.get(0))
        .unwrap();

    let first = fx.wb.request_release(&id).unwrap();
    assert!(!marker(&fx, "released.txt"), "requesting runs nothing");
    let card = crate::cards::get(&fx.wb.db, &first).unwrap();
    assert_eq!(card.kind, "release");
    assert_eq!(card.payload["command"], "printf done > released.txt");
    // ⌘K pressed twice: same open card, not a second one.
    assert_eq!(fx.wb.request_release(&id).unwrap(), first);

    let out = fx.wb.confirm_release(&first).unwrap();
    assert_eq!(
        (out.target_id.as_str(), out.name.as_str()),
        (id.as_str(), "pages")
    );
    assert!(marker(&fx, "released.txt"));
    // The answered card cannot run the command again (double click / race).
    std::fs::remove_file(fx.dir.path().join("released.txt")).unwrap();
    assert_eq!(
        code(fx.wb.confirm_release(&first).unwrap_err()),
        "release_already_answered"
    );
    assert!(!marker(&fx, "released.txt"));

    // Never remembered: the next release of the same target is a new card, and
    // nothing runs until it is confirmed again.
    let second = fx.wb.request_release(&id).unwrap();
    assert_ne!(second, first);
    assert_eq!(queued_release(&fx), vec![second.clone()]);
    assert!(!marker(&fx, "released.txt"));
    let rules_after: i64 = fx
        .wb
        .db
        .conn()
        .query_row("SELECT COUNT(*) FROM permission_rules", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        rules_after, rules_before,
        "a release writes no remembered rule"
    );

    let requested = kinds(&fx, EventKind::ReleaseRequested);
    assert_eq!(requested.len(), 2);
    let confirmed = kinds(&fx, EventKind::ReleaseConfirmed);
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0]["by"], "owner");
    assert_eq!(confirmed[0]["exit_code"], 0);
    assert_eq!(confirmed[0]["question_id"], first.as_str());
}

#[test]
fn missing_credential_fails_closed_and_runs_nothing() {
    let fx = fixture();
    let id = register(
        &fx,
        "npm",
        r#"printf '%s' "$NPM_TOKEN" | wc -c > len.txt && echo "using token $NPM_TOKEN""#,
        &["NPM_TOKEN"],
    );
    let qid = fx.wb.request_release(&id).unwrap();
    let err = fx.wb.confirm_release(&qid).unwrap_err();
    assert!(err.to_string().contains("NPM_TOKEN"), "{err}");
    assert_eq!(code(err), "release_credential_missing");
    assert!(!marker(&fx, "len.txt"), "nothing ran");
    assert_eq!(
        queued_release(&fx),
        vec![qid.clone()],
        "the card waits for the fix"
    );
    let failed = kinds(&fx, EventKind::ReleaseFailed);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["executed"], false);
    assert_eq!(failed[0]["error"], "release_credential_missing");

    // Owner stores the credential in Settings · Release, confirms the same card.
    fx.wb
        .set_release_credential(&id, "NPM_TOKEN", SECRET)
        .unwrap();
    let out = fx.wb.confirm_release(&qid).unwrap();
    let len = std::fs::read_to_string(fx.dir.path().join("len.txt")).unwrap();
    assert_eq!(
        len.trim(),
        SECRET.len().to_string(),
        "the value reached the command as an env var"
    );
    assert!(
        out.output.contains("using token [redacted]"),
        "{}",
        out.output
    );
    assert!(!out.output.contains(SECRET));
    assert!(
        leak_scan(&fx.wb.db, crate::PROJECT_ID, SECRET)
            .unwrap()
            .is_empty(),
        "the secret never reaches cards, trace or the registry"
    );
}

#[test]
fn a_target_changed_or_removed_after_the_card_refuses_to_run() {
    let fx = fixture();
    let id = register(&fx, "pages", "printf a > a.txt", &[]);
    let qid = fx.wb.request_release(&id).unwrap();
    fx.wb
        .save_release_target(&ReleaseTargetDraft {
            id: Some(id.clone()),
            name: "pages".into(),
            command: "printf b > b.txt".into(),
            cwd: String::new(),
            credentials: vec![],
        })
        .unwrap();
    assert_eq!(
        code(fx.wb.confirm_release(&qid).unwrap_err()),
        "release_target_changed"
    );
    assert!(!marker(&fx, "a.txt") && !marker(&fx, "b.txt"));

    let fresh = fx.wb.request_release(&id);
    assert_eq!(fresh.unwrap(), qid, "the stale card is still the open one");
    fx.wb.remove_release_target(&id).unwrap();
    assert_eq!(
        code(fx.wb.confirm_release(&qid).unwrap_err()),
        "release_target_gone"
    );
    assert_eq!(
        code(fx.wb.request_release(&id).unwrap_err()),
        "release_target_unknown"
    );
    // The owner can still decline the dead card.
    fx.wb.reject_release(&qid).unwrap();
    assert!(queued_release(&fx).is_empty());
}

#[test]
fn a_failed_release_is_recorded_and_never_silently_rerun() {
    let fx = fixture();
    let id = register(
        &fx,
        "deploy",
        "echo half-way; printf x >> runs.txt; exit 3",
        &[],
    );
    let qid = fx.wb.request_release(&id).unwrap();
    assert_eq!(
        code(fx.wb.confirm_release(&qid).unwrap_err()),
        "release_command_failed"
    );
    let failed = kinds(&fx, EventKind::ReleaseFailed);
    assert_eq!(failed[0]["executed"], true);
    assert_eq!(failed[0]["exit_code"], 3);
    assert!(failed[0]["output"].as_str().unwrap().contains("half-way"));
    assert!(queued_release(&fx).is_empty(), "the card was used up");
    assert!(fx.wb.confirm_release(&qid).is_err());
    let runs = std::fs::read_to_string(fx.dir.path().join("runs.txt")).unwrap();
    assert_eq!(runs, "x", "ran exactly once");
}

#[test]
fn rejecting_runs_nothing() {
    let fx = fixture();
    let id = register(&fx, "pages", "printf a > a.txt", &[]);
    let qid = fx.wb.request_release(&id).unwrap();
    fx.wb.reject_release(&qid).unwrap();
    assert!(!marker(&fx, "a.txt"));
    assert_eq!(kinds(&fx, EventKind::ReleaseRejected)[0]["by"], "owner");
    assert_eq!(
        code(fx.wb.confirm_release(&qid).unwrap_err()),
        "release_already_answered"
    );
}

#[test]
fn a_hung_release_is_killed_at_the_timeout() {
    let fx = fixture();
    let id = register(&fx, "slow", "sleep 5; printf late > late.txt", &[]);
    let qid = fx.wb.request_release(&id).unwrap();
    let started = std::time::Instant::now();
    let err = crate::release::confirm_by(
        &fx.wb.db,
        crate::PROJECT_ID,
        &qid,
        fx.wb.creds.as_ref(),
        &crate::release::Decider::Owner,
        std::time::Duration::from_millis(300),
    )
    .unwrap_err();
    assert_eq!(err.code(), "release_timed_out");
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    std::thread::sleep(std::time::Duration::from_millis(5200).saturating_sub(started.elapsed()));
    assert!(
        !marker(&fx, "late.txt"),
        "the whole process group was killed"
    );
}

#[test]
fn delegation_confirms_a_ready_release_but_refuses_missing_credentials() {
    let mut fx = fixture();
    let ready = register(&fx, "pages", "printf a > a.txt", &[]);
    let needs = register(&fx, "npm", "printf b > b.txt", &["NPM_TOKEN"]);
    fx.wb.request_release(&ready).unwrap();
    let blocked = fx.wb.request_release(&needs).unwrap();
    let status = crate::delegation::enable(&fx.wb.db, crate::PROJECT_ID).unwrap();
    let before = fx.wb.release_targets().unwrap();

    let done = fx.wb.managed_tick().unwrap();
    let mut decided: Vec<(String, String)> = done
        .iter()
        .map(|d| (d.action.clone(), d.reason.clone()))
        .collect();
    decided.sort();
    assert_eq!(
        decided,
        [
            ("incomplete".to_string(), "credential_missing".to_string()),
            (
                "release".to_string(),
                "registered_target_credentials_present".to_string()
            ),
        ]
    );
    assert!(marker(&fx, "a.txt"), "the ready release ran");
    assert!(
        !marker(&fx, "b.txt"),
        "the one missing a credential did not"
    );
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(
        kinds(&fx, EventKind::ReleaseConfirmed)[0]["by"],
        actor.as_str()
    );
    assert_eq!(
        fx.wb.release_targets().unwrap(),
        before,
        "delegation never edits the registry"
    );

    // The report row points to Settings · Release, located at the target.
    let history = managed_history(&fx.wb.db, crate::PROJECT_ID, 10).unwrap();
    let row = history.iter().find(|r| r.card_id == blocked).unwrap();
    assert_eq!(row.result, "incomplete");
    assert_eq!(
        (row.next.kind, row.next.target.as_str()),
        (ManagedNextKind::ReleaseTargets, needs.as_str())
    );
    let ran = history.iter().find(|r| r.card_id != blocked).unwrap();
    assert_eq!(ran.result, "done");
    assert_eq!(ran.evidence, ManagedEvidence::HostVerified);
}

#[test]
fn only_the_owner_changes_release_targets() {
    let fx = fixture();
    let id = register(&fx, "npm", "npm publish", &["NPM_TOKEN"]);
    let _acting = fx.wb.acting_as("managed:g:1");
    let draft = ReleaseTargetDraft {
        id: None,
        name: "other".into(),
        command: "true".into(),
        cwd: String::new(),
        credentials: vec![],
    };
    for result in [
        fx.wb.save_release_target(&draft).map(|_| ()),
        fx.wb
            .set_release_credential(&id, "NPM_TOKEN", SECRET)
            .map(|_| ()),
        fx.wb
            .remove_release_credential(&id, "NPM_TOKEN")
            .map(|_| ()),
        fx.wb.remove_release_target(&id),
    ] {
        assert_eq!(code(result.unwrap_err()), "release_owner_only");
    }
    assert!(fx
        .wb
        .creds
        .get(&credential_key(&id, "NPM_TOKEN"))
        .unwrap()
        .is_none());
    assert_eq!(fx.wb.release_targets().unwrap().len(), 1);
}

#[test]
fn release_agent_tools_only_draft_or_request() {
    // Behaviour changed in owner-followups-1015 #16: chat-triggered release added exactly two
    // agent tools (draft a registration card / request a release card). No tool confirms a
    // release or edits the registry; see release_chat_tests.rs for their card-only behaviour.
    let reg = crate::tools::Registry::builtin();
    let mut names: Vec<String> = reg
        .defs()
        .into_iter()
        .map(|d| d.name)
        .filter(|n| n.contains("release"))
        .collect();
    names.sort();
    assert_eq!(names, ["propose_release_target", "request_release"]);
}

#[test]
fn a_card_read_failure_is_reported_not_called_unknown() {
    // ticket 33（2026-10-25 批次审查 S2，原票 #15 / #16）：裁决入口曾 `cards::get(..).ok()`，读库失败被报成
    // 「未知卡」。现在读失败如实上抛（Cards），只有真没有这张卡（或卡种 / 项目不符）才是 unknown。
    let fx = fixture();
    let id = register(&fx, "site", "printf ok > ran.txt", &[]);
    let qid = fx.wb.request_release(&id).unwrap();
    let db = &fx.wb.db;
    let store = fx.wb.creds.as_ref();
    let pid = crate::PROJECT_ID;
    // 卡种不符：发布卡交给拟稿裁决 → 拟稿未知；不存在的卡 → 发布未知
    assert_eq!(
        crate::release_chat::approve(db, pid, &qid, store, &crate::release::Decider::Owner)
            .unwrap_err()
            .code(),
        "release_proposal_unknown"
    );
    assert_eq!(
        crate::release::reject(db, pid, "q-nope")
            .unwrap_err()
            .code(),
        "release_question_unknown"
    );
    crate::cards::break_reads_for_test(db);
    let errs = [
        crate::release::confirm(db, pid, &qid, store).unwrap_err(),
        crate::release::reject(db, pid, &qid).unwrap_err(),
        crate::release_chat::approve(db, pid, &qid, store, &crate::release::Decider::Owner)
            .unwrap_err(),
        crate::release_chat::reject(db, pid, &qid, &crate::release::Decider::Owner).unwrap_err(),
    ];
    for e in errs {
        assert!(
            matches!(e, crate::release::ReleaseRunError::Cards(_)),
            "read failure must surface, got {e:?}"
        );
    }
    assert!(!marker(&fx, "ran.txt"), "nothing ran");
}
