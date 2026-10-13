//! P1-1 physical worktree through the `worktree` confirmation card
//! (Wayfinder-07 Addendum A). Replaces the 2026-10-12 WIP test
//! `worktree_create_requires_owner_action_before_physical_write`, which
//! exercised a model-registry tool and a permission card attributed to the
//! first agent; that path was removed because it forged agent identity.
use super::*;
use crate::errcode::ErrorCode;
use crate::session_host::{
    self, Transition, WorktreeIntent, WorktreeIntentState, WorktreeOperation,
};
use std::path::{Path, PathBuf};

struct Fixture {
    _repo: tempfile::TempDir,
    _outside: tempfile::TempDir,
    root: PathBuf,
    target: PathBuf,
    wb: Workbench,
    session_id: String,
}

fn fixture() -> Fixture {
    let repo = tempfile::tempdir().unwrap();
    crate::git::init(repo.path(), "main").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(repo.path(), &["worker"], None).unwrap();
    let session = wb.ensure_session().unwrap();
    session_host::transition(&wb.db, &session.session_id, Transition::Start, 0).unwrap();
    Fixture {
        root: repo.path().to_path_buf(),
        target: outside.path().join("session-tree"),
        _repo: repo,
        _outside: outside,
        wb,
        session_id: session.session_id,
    }
}

fn code(error: &ApiError) -> String {
    error.code()
}

fn system_kinds(wb: &Workbench) -> Vec<serde_json::Value> {
    wb.db
        .conn()
        .prepare("SELECT payload FROM events WHERE project_id='p1' AND kind='system' ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|raw| serde_json::from_str(&raw.unwrap()).unwrap())
        .filter(|p: &serde_json::Value| {
            p["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("worktree_"))
        })
        .collect()
}

/// Every worktree System kind written by these paths is registered (D10).
fn assert_registered(wb: &Workbench) {
    for event in system_kinds(wb) {
        let kind = event["kind"].as_str().unwrap();
        assert!(
            crate::trace::SYSTEM_SUBKINDS.contains(&kind),
            "unregistered System subkind {kind}"
        );
    }
}

fn move_baseline(root: &Path) {
    crate::git::run(root, &["checkout", "main"]).ok();
    crate::git::run(root, &["commit", "--allow-empty", "-m", "baseline moved"]).unwrap();
}

fn create(f: &Fixture) -> String {
    let request =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    f.wb.confirm_session_worktree(&request.question_id).unwrap();
    request.question_id
}

#[test]
fn request_enqueues_one_owner_card_and_writes_nothing() {
    let f = fixture();
    let first =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    assert!(!first.reused);
    assert!(
        !f.target.exists(),
        "a request must not touch the filesystem"
    );
    let again =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    assert!(again.reused);
    assert_eq!(again.question_id, first.question_id);
    let cards = crate::cards::queued(&f.wb.db, "p1").unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].kind, "worktree");
    assert_eq!(
        cards[0].agent_id, None,
        "the owner's request is not an agent action"
    );
    assert_eq!(cards[0].payload["operation"], "create");
    assert_eq!(cards[0].payload["baseline_commit"], first.baseline_commit);
    let events = system_kinds(&f.wb);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "worktree_requested");
    assert_eq!(events[0]["question_id"], first.question_id.as_str());
}

#[test]
fn confirmed_create_and_cleanup_carry_the_card_identity() {
    let f = fixture();
    let create_id = create(&f);
    assert!(f.target.exists());
    let session = session_host::get(&f.wb.db, &f.session_id).unwrap();
    let bound = session.worktree.clone().unwrap();
    assert!(bound.created_by_session);
    assert_eq!(session.worktree_intent, None);
    assert!(crate::git::worktree_registered(&f.root, &f.target)
        .unwrap()
        .is_some());
    assert!(crate::cards::queued(&f.wb.db, "p1").unwrap().is_empty());
    f.wb.validate_session_worktree().unwrap();

    let cleanup =
        f.wb.request_session_worktree(WorktreeOperation::Cleanup, None)
            .unwrap();
    assert!(f.target.exists(), "cleanup waits for its own card");
    f.wb.confirm_session_worktree(&cleanup.question_id).unwrap();
    assert!(!f.target.exists());
    let session = session_host::get(&f.wb.db, &f.session_id).unwrap();
    assert_eq!(session.worktree, None);
    assert_eq!(session.session_id, f.session_id);

    let events = system_kinds(&f.wb);
    let created = events
        .iter()
        .find(|e| e["kind"] == "worktree_created")
        .unwrap();
    assert_eq!(created["question_id"], create_id.as_str());
    let deleted = events
        .iter()
        .find(|e| e["kind"] == "worktree_deleted")
        .unwrap();
    assert_eq!(deleted["question_id"], cleanup.question_id.as_str());
    assert_registered(&f.wb);
}

#[test]
fn owner_rejection_writes_nothing_and_is_traced() {
    let f = fixture();
    let request =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    f.wb.reject_session_worktree(&request.question_id).unwrap();
    assert!(!f.target.exists());
    assert_eq!(
        session_host::get(&f.wb.db, &f.session_id).unwrap().worktree,
        None
    );
    let card = crate::cards::get(&f.wb.db, &request.question_id).unwrap();
    assert_eq!(card.payload["outcome"], "rejected");
    assert!(system_kinds(&f.wb)
        .iter()
        .any(|e| e["kind"] == "worktree_rejected" && e["reason_code"] == "owner_rejected"));
    // The answered card cannot be confirmed later.
    let late =
        f.wb.confirm_session_worktree(&request.question_id)
            .unwrap_err();
    assert_eq!(code(&late), "worktree_card_unknown");
    assert!(!f.target.exists());
    assert_registered(&f.wb);
}

#[test]
fn baseline_drift_between_request_and_confirm_settles_the_card_without_writing() {
    let f = fixture();
    let request =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    move_baseline(&f.root);
    let error =
        f.wb.confirm_session_worktree(&request.question_id)
            .unwrap_err();
    assert_eq!(code(&error), "worktree_binding_drift");
    assert!(!f.target.exists());
    let card = crate::cards::get(&f.wb.db, &request.question_id).unwrap();
    assert_eq!(card.payload["outcome"], "conflict");
    assert_eq!(card.payload["reason_code"], "worktree_binding_drift");
    assert!(system_kinds(&f.wb)
        .iter()
        .any(|e| e["kind"] == "worktree_binding_drift"));
    assert_registered(&f.wb);
}

#[test]
fn cleanup_refuses_dirty_content_and_keeps_the_binding() {
    let f = fixture();
    create(&f);
    std::fs::write(f.target.join("notes.txt"), "keep me").unwrap();
    let error =
        f.wb.request_session_worktree(WorktreeOperation::Cleanup, None)
            .unwrap_err();
    assert_eq!(code(&error), "worktree_dirty");
    assert!(f.target.join("notes.txt").exists());
    assert!(session_host::get(&f.wb.db, &f.session_id)
        .unwrap()
        .worktree
        .is_some());
}

#[test]
fn dirty_content_appearing_after_the_request_still_blocks_deletion() {
    let f = fixture();
    create(&f);
    let cleanup =
        f.wb.request_session_worktree(WorktreeOperation::Cleanup, None)
            .unwrap();
    std::fs::write(f.target.join("late.txt"), "written after the request").unwrap();
    let error =
        f.wb.confirm_session_worktree(&cleanup.question_id)
            .unwrap_err();
    assert_eq!(code(&error), "worktree_dirty");
    assert!(f.target.join("late.txt").exists());
}

#[test]
fn baseline_drift_blocks_cleanup_and_recovery() {
    let f = fixture();
    create(&f);
    move_baseline(&f.root);
    let error =
        f.wb.request_session_worktree(WorktreeOperation::Cleanup, None)
            .unwrap_err();
    assert_eq!(code(&error), "worktree_binding_drift");
    let resume = f.wb.validate_session_worktree().unwrap_err();
    assert_eq!(code(&resume), "worktree_binding_drift");
    assert!(f.target.exists(), "drift never deletes");
    assert_registered(&f.wb);
}

#[test]
fn unsafe_paths_and_double_binding_are_refused_before_a_card() {
    let f = fixture();
    let inside = f.root.join("nested-tree");
    for (path, expected) in [
        (
            inside.to_str().unwrap().to_string(),
            "worktree_path_invalid",
        ),
        ("relative/tree".to_string(), "worktree_path_invalid"),
        (
            f.root.to_str().unwrap().to_string(),
            "worktree_path_invalid",
        ),
        (
            f._outside.path().to_str().unwrap().to_string(),
            "worktree_path_invalid",
        ),
    ] {
        let error =
            f.wb.request_session_worktree(WorktreeOperation::Create, Some(&path))
                .unwrap_err();
        assert_eq!(code(&error), expected, "{path}");
    }
    assert!(crate::cards::queued(&f.wb.db, "p1").unwrap().is_empty());
    create(&f);
    let other = f._outside.path().join("second-tree");
    let error =
        f.wb.request_session_worktree(WorktreeOperation::Create, other.to_str())
            .unwrap_err();
    assert_eq!(code(&error), "worktree_binding_conflict");
    assert_registered(&f.wb);
}

#[test]
fn interrupted_create_is_unknown_until_a_read_only_verify_settles_it() {
    let f = fixture();
    let commit = crate::git::baseline_commit(&f.root, "main").unwrap();
    // Simulate a crash after git add and before metadata.
    crate::git::worktree_add(&f.root, &f.target, &commit).unwrap();
    let target = crate::git::canonical_worktree_path(&f.target).unwrap();
    session_host::set_worktree_intent(
        &f.wb.db,
        &f.session_id,
        Some(&WorktreeIntent {
            operation: WorktreeOperation::Create,
            question_id: "q-crash".into(),
            path: target.to_string_lossy().into_owned(),
            baseline_commit: commit,
            state: WorktreeIntentState::Executing,
            reason_code: None,
        }),
    )
    .unwrap();
    assert_eq!(
        code(&f.wb.validate_session_worktree().unwrap_err()),
        "worktree_outcome_unknown"
    );
    assert_eq!(
        code(
            &f.wb
                .request_session_worktree(WorktreeOperation::Cleanup, None)
                .unwrap_err()
        ),
        "worktree_outcome_unknown"
    );
    let settled = f.wb.verify_session_worktree().unwrap();
    assert!(settled.worktree.as_ref().unwrap().created_by_session);
    assert_eq!(settled.worktree_intent, None);
    assert!(system_kinds(&f.wb)
        .iter()
        .any(|e| e["kind"] == "worktree_verified"
            && e["reason_code"] == "worktree_create_confirmed"
            && e["question_id"] == "q-crash"));
    f.wb.validate_session_worktree().unwrap();
    assert_registered(&f.wb);
}

#[test]
fn interrupted_create_that_never_reached_git_is_dropped_by_verify() {
    let f = fixture();
    session_host::set_worktree_intent(
        &f.wb.db,
        &f.session_id,
        Some(&WorktreeIntent {
            operation: WorktreeOperation::Create,
            question_id: "q-lost".into(),
            path: f.target.to_string_lossy().into_owned(),
            baseline_commit: crate::git::baseline_commit(&f.root, "main").unwrap(),
            state: WorktreeIntentState::Executing,
            reason_code: None,
        }),
    )
    .unwrap();
    let settled = f.wb.verify_session_worktree().unwrap();
    assert_eq!(settled.worktree, None);
    assert_eq!(settled.worktree_intent, None);
    assert!(!f.target.exists());
}

#[derive(Debug, Clone)]
enum Step {
    RequestCreate,
    RequestCleanup,
    RejectFirst,
    Verify,
    MoveBaseline,
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(10))]

    /// D12: no sequence of requests, rejections, verifies or baseline moves
    /// can create or delete a worktree without a confirmed card.
    #[test]
    fn no_physical_write_without_a_confirmed_card(steps in proptest::collection::vec(
        proptest::prop_oneof![
            proptest::strategy::Just(Step::RequestCreate),
            proptest::strategy::Just(Step::RequestCleanup),
            proptest::strategy::Just(Step::RejectFirst),
            proptest::strategy::Just(Step::Verify),
            proptest::strategy::Just(Step::MoveBaseline),
        ], 1..8)) {
        let f = fixture();
        for step in steps {
            match step {
                Step::RequestCreate => { let _ = f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str()); }
                Step::RequestCleanup => { let _ = f.wb.request_session_worktree(WorktreeOperation::Cleanup, None); }
                Step::RejectFirst => {
                    if let Some(card) = crate::cards::queued(&f.wb.db, "p1").unwrap().into_iter().find(|c| c.kind == "worktree") {
                        f.wb.reject_session_worktree(&card.id).unwrap();
                    }
                }
                Step::Verify => { let _ = f.wb.verify_session_worktree(); }
                Step::MoveBaseline => move_baseline(&f.root),
            }
            proptest::prop_assert!(!f.target.exists());
            proptest::prop_assert!(crate::git::worktree_registered(&f.root, &f.target).unwrap().is_none());
            proptest::prop_assert_eq!(session_host::get(&f.wb.db, &f.session_id).unwrap().worktree, None);
        }
    }
}

/// Owner ruling 2026-10-12 (Addendum A, managed (c)): the delegate confirms
/// only on host-verified facts; drift forces a refusal and nothing is written.
#[test]
fn delegated_worktree_card_follows_host_verified_facts() {
    let mut f = fixture();
    let request =
        f.wb.request_session_worktree(WorktreeOperation::Create, f.target.to_str())
            .unwrap();
    let status = crate::delegation::enable(&f.wb.db, crate::PROJECT_ID).unwrap();
    let done = f.wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].via.as_str()),
        ("confirm", "host"),
        "{done:?}"
    );
    assert!(f.target.exists());
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    let card = crate::cards::get(&f.wb.db, &request.question_id).unwrap();
    assert_eq!(card.answered_by.as_deref(), Some(actor.as_str()));

    let cleanup =
        f.wb.request_session_worktree(WorktreeOperation::Cleanup, None)
            .unwrap();
    move_baseline(&f.root);
    let done = f.wb.managed_tick().unwrap();
    assert_eq!(done[0].action.as_str(), "incomplete", "{done:?}");
    assert!(
        f.target.exists(),
        "a drifted cleanup is never run by the delegate"
    );
    let report = managed_external_ops(&f.wb.db, crate::PROJECT_ID, 10).unwrap();
    let worktree: Vec<_> = report
        .iter()
        .filter(|op| op.kind == "local_worktree")
        .collect();
    assert_eq!(worktree.len(), 2, "{report:?}");
    assert_eq!(
        (
            worktree[1].action.as_str(),
            worktree[1].result.as_str(),
            worktree[1].verification.as_str()
        ),
        ("create", "done", "host_verified_worktree_created")
    );
    assert_eq!(worktree[0].action.as_str(), "cleanup");
    assert_eq!(worktree[0].result.as_str(), "incomplete");
    assert!(
        crate::cards::get(&f.wb.db, &cleanup.question_id)
            .unwrap()
            .state
            != crate::cards::CardState::Queued
    );
    assert_registered(&f.wb);
}

#[test]
fn worktree_forced_confirms_only_verified_facts() {
    use super::managed::worktree_forced;
    assert_eq!(worktree_forced(&Ok(())).0, "confirm");
    for error in [
        WorktreeError::WorktreeBindingDrift("x".into()),
        WorktreeError::WorktreeDirty("x".into()),
        WorktreeError::WorktreeNotOwned,
        WorktreeError::WorktreeOutcomeUnknown,
        WorktreeError::WorktreeMissing,
        WorktreeError::WorktreeSessionInactive("x".into()),
    ] {
        assert_eq!(worktree_forced(&Err(error)).0, "incomplete");
    }
}

/// Golden path 2026-10-13: the strip's default path
/// `<parent>/<project>-worktrees/<session>` was refused because its parent did
/// not exist yet. Missing parents are created by git on confirm; nothing is
/// written at request time, and `..` in the missing part is still refused.
#[test]
fn a_default_path_under_a_missing_parent_is_accepted_and_created_on_confirm() {
    let f = fixture();
    let sneaky = format!("{}/missing/../escape", f._outside.path().display());
    assert_eq!(
        code(
            &f.wb
                .request_session_worktree(WorktreeOperation::Create, Some(&sneaky))
                .unwrap_err()
        ),
        "worktree_path_invalid"
    );
    let nested = f._outside.path().join("qa-worktrees").join("abc12345");
    let request =
        f.wb.request_session_worktree(WorktreeOperation::Create, nested.to_str())
            .unwrap();
    assert!(!nested.parent().unwrap().exists(), "requests write nothing");
    f.wb.confirm_session_worktree(&request.question_id).unwrap();
    assert!(nested.exists());
    assert!(crate::git::worktree_registered(&f.root, &nested)
        .unwrap()
        .is_some());
}
