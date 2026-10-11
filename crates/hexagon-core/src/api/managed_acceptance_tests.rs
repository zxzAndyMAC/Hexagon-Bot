//! Test3/19: delegated business decisions. Acceptance only ever takes
//! complete current evidence; gaps are checked, failures go back for rework,
//! and a stage that cannot meet its requirements ends incomplete — no owner
//! card at any point, and every decision carries the grant actor.
use super::*;
use crate::provider::ScriptedProvider;
use crate::turn::text_response;
use serde_json::json;

fn checked_wb(dir: &Path, check: &str) -> Workbench {
    let pack: PackDef = serde_json::from_value(json!({"name":"managed","version":1,
        "stages":[{"name":"test","roles":["worker"],"due":[],"checks":[check],"stamp_point":true}]}))
    .unwrap();
    let wb = Workbench::for_test(dir, &["worker"], Some(pack)).unwrap();
    wb.open_stage(0).unwrap();
    wb
}

fn awaiting(wb: &Workbench) {
    assert_eq!(
        serde_json::to_value(wb.advance().unwrap()).unwrap()["action"],
        "awaiting_stamp"
    );
}

fn stamped_by(wb: &Workbench) -> Vec<String> {
    wb.db
        .events(crate::PROJECT_ID, Some(&[EventKind::Stamped]))
        .unwrap()
        .into_iter()
        .map(|e| e.payload["by"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn run_states(wb: &Workbench) -> Vec<String> {
    let mut st = wb
        .db
        .conn()
        .prepare("SELECT state FROM stage_runs ORDER BY rowid")
        .unwrap();
    st.query_map([], |r| r.get(0)).unwrap().flatten().collect()
}

#[test]
fn complete_current_evidence_is_accepted_under_the_grant_actor() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = checked_wb(dir.path(), "true");
    wb.run_checks().unwrap();
    awaiting(&wb);
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(
        owner_pending(&wb.db, crate::PROJECT_ID).unwrap().is_empty(),
        "no owner card"
    );
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "accept", "{done:?}");
    assert_eq!(done[0].via, "host");
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(stamped_by(&wb), vec![actor], "never `owner`");
    assert_eq!(run_states(&wb), vec!["done"]);
}

#[test]
fn stale_evidence_is_checked_again_then_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = checked_wb(dir.path(), "true");
    wb.run_checks().unwrap();
    awaiting(&wb);
    std::fs::write(dir.path().join("input.txt"), "changed after the check").unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "run_checks", "{done:?}");
    assert_eq!(
        managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap()[0].outcome,
        "checks_ran_and_accepted"
    );
    assert_eq!(run_states(&wb), vec!["done"]);
}

#[test]
fn evidence_that_changes_before_commit_is_not_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = checked_wb(dir.path(), "true");
    wb.run_checks().unwrap();
    awaiting(&wb);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    assert_eq!(plan[0].forced.as_ref().unwrap().0, "accept");
    // The delivery changes between the decision and its commit.
    std::fs::write(dir.path().join("late.txt"), "new bytes").unwrap();
    let decision = managed_decide_on(
        &wb.db,
        &HashMap::new(),
        dir.path(),
        crate::PROJECT_ID,
        &plan[0],
    );
    wb.managed_commit(&plan[0], decision).unwrap();
    assert_eq!(
        managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap()[0].outcome,
        "still_incomplete"
    );
    assert!(
        stamped_by(&wb).is_empty(),
        "an old version is never accepted"
    );
    assert_eq!(run_states(&wb), vec!["waiting_stamp"]);
}

/// A failing check keeps a stage from reaching its stamp at all, so the
/// delegated rework path is the real one: checks passed, the card was raised,
/// then the delivery changed and the checks now fail.
fn regress_after_stamp_card(wb: &Workbench, dir: &Path) {
    std::fs::write(dir.join("ok.txt"), "ok").unwrap();
    wb.run_checks().unwrap();
    awaiting(wb);
    std::fs::remove_file(dir.join("ok.txt")).unwrap();
}

#[test]
fn a_failed_check_is_reworked_and_passes_after_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = checked_wb(dir.path(), "test -f ok.txt");
    regress_after_stamp_card(&wb, dir.path());
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let first = wb.managed_tick().unwrap();
    assert_eq!(first[0].action, "run_checks");
    assert_eq!(
        managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap()[0].outcome,
        "checks_ran_still_incomplete"
    );
    let second = wb.managed_tick().unwrap();
    assert_eq!(
        (second[0].action.as_str(), second[0].reason.as_str()),
        ("rework", "checks_failed")
    );
    assert_eq!(
        run_states(&wb),
        vec!["rejected", "active"],
        "the named stage reopened"
    );
    let rejected = wb
        .db
        .events(crate::PROJECT_ID, Some(&[EventKind::StampRejected]))
        .unwrap()[0]
        .payload
        .clone();
    assert!(
        rejected["by"].as_str().unwrap().starts_with("managed:"),
        "{rejected}"
    );
    assert!(
        rejected["note"].as_str().unwrap().contains("checks_failed"),
        "{rejected}"
    );
    // The role fixes it; the same stage budget continues (no reset per card).
    std::fs::write(dir.path().join("ok.txt"), "ok").unwrap();
    wb.run_checks().unwrap();
    awaiting(&wb);
    let third = wb.managed_tick().unwrap();
    assert_eq!(third[0].action, "accept", "{third:?}");
    assert_eq!(run_states(&wb), vec!["rejected", "done"]);
    assert_eq!(
        crate::delegation::rounds(&wb.db, crate::PROJECT_ID, "stage:test"),
        3
    );
}

#[test]
fn a_stage_that_cannot_pass_ends_incomplete_after_three_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = checked_wb(dir.path(), "test -f ok.txt");
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    regress_after_stamp_card(&wb, dir.path());
    assert_eq!(wb.managed_tick().unwrap()[0].action, "run_checks");
    assert_eq!(wb.managed_tick().unwrap()[0].action, "rework");
    regress_after_stamp_card(&wb, dir.path());
    assert_eq!(wb.managed_tick().unwrap()[0].action, "run_checks");
    let last = wb.managed_tick().unwrap();
    assert_eq!(
        (last[0].action.as_str(), last[0].reason.as_str()),
        ("incomplete", "budget_exhausted")
    );
    assert!(stamped_by(&wb).is_empty(), "never accepted below the bar");
    assert!(
        crate::cards::queued(&wb.db, crate::PROJECT_ID)
            .unwrap()
            .is_empty(),
        "no owner card left"
    );
    assert!(wb.managed_tick().unwrap().is_empty());
}

#[test]
fn the_model_chooses_a_versioned_design_option_and_the_task_continues() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["UI/UX", "前端"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    let requested = crate::design::require(&wb.db, &wb.project_id, "a1").unwrap();
    let mockup = |fill: &str| crate::design::DesignMockupInput {
        page: "Dashboard".into(),
        mime: "image/svg+xml".into(),
        content: format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 600"><rect width="800" height="600" fill="{fill}"/></svg>"#
        ),
    };
    let option = |id: &str, fill: &str| crate::design::DesignOptionInput {
        id: id.into(),
        title: format!("Direction {id}"),
        description: "Readable dashboard".into(),
        layout: "Sidebar".into(),
        typography: "System sans".into(),
        palette: fill.into(),
        mockups: vec![mockup(fill)],
    };
    wb.registry
        .call(
            &wb.db,
            &ctx,
            "propose_design",
            json!({"expected_revision":requested.revision,"options":[option("a","red"),option("b","blue")]}),
        )
        .unwrap();
    let chat = Arc::new(ScriptedProvider::new(vec![
        text_response("b"),
        text_response("continuing with b"),
        text_response("continuing with b"),
    ]));
    wb.register_provider("default", chat.clone());
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "b", "{done:?}");
    assert_eq!(done[0].via, "model");
    let direction = design_direction(&wb.db, &wb.project_id).unwrap();
    assert_eq!(direction.selected_option.as_deref(), Some("b"));
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    let card = crate::cards::get(&wb.db, direction.question_id.as_deref().unwrap()).unwrap();
    assert_eq!(
        card.answered_by.as_deref(),
        Some(actor.as_str()),
        "not owner_confirmed"
    );
    assert!(chat.recorded().len() >= 2, "the proposer's task continued");
}
