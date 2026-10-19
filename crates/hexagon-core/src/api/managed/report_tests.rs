//! Gap A2–A5 regressions through the in-process facade.
use super::*;
use proptest::prelude::*;
use serde_json::json;

fn wb() -> (tempfile::TempDir, Workbench) {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    (dir, wb)
}
fn spend(wb: &Workbench, mc: i64, at: &str, known: bool) {
    wb.db
        .conn()
        .execute(
            "INSERT INTO usage(project_id,cost_millicents,created_at,cost_known) VALUES (?1,?2,?3,?4)",
            rusqlite::params![crate::PROJECT_ID, mc, at, known as i64],
        )
        .unwrap();
}

#[test]
fn summary_reports_the_latest_session_budget_and_session_spend() {
    let (_dir, wb) = wb();
    let empty = managed_summary(&wb.db, crate::PROJECT_ID).unwrap();
    assert_eq!(
        (empty.started_at.clone(), empty.session_spent_mc),
        (None, 0)
    );
    crate::usage::set_limit(&wb.db, crate::PROJECT_ID, Some(10_000)).unwrap();
    spend(&wb, 5_000, "2000-01-01 00:00:00", true);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, true).unwrap();
    crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, false).unwrap();
    spend(&wb, 700, "2999-01-01 00:00:00", true);
    let on = managed_summary(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(on.started_at.is_some());
    assert_eq!(on.ended_at, None, "pause/resume keeps the session open");
    assert_eq!(on.limit_cents, Some(10_000));
    assert_eq!(on.spent_mc, 5_700);
    assert_eq!(
        on.session_spent_mc, 700,
        "spend before the session is excluded"
    );
    assert!(!on.cost_unknown);
    spend(&wb, 0, "2999-01-01 00:00:00", false);
    assert!(
        managed_summary(&wb.db, crate::PROJECT_ID)
            .unwrap()
            .cost_unknown
    );
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    let off = managed_summary(&wb.db, crate::PROJECT_ID).unwrap();
    assert_eq!(
        off.started_at, on.started_at,
        "the report keeps the closed session"
    );
    assert!(off.ended_at.is_some());
    assert_eq!(off.session_spent_mc, 0, "spend after the end is excluded");
}

#[test]
fn history_records_carry_scope_and_evidence_from_host_codes() {
    let (_dir, wb) = wb();
    for (action, reason, via) in [
        ("incomplete", "target_missing", "host"),
        ("accept", "evidence_complete", "host"),
        ("deny", "target_missing", "model"),
    ] {
        wb.db
            .append_event(
                crate::PROJECT_ID,
                EventKind::System,
                json!({"kind":"managed_decision","card_id":"c","card_kind":"push",
                    "action":action,"reason":reason,"via":via,"actor":"managed:d:1","outcome":"ok"}),
                None,
                None,
            )
            .unwrap();
    }
    let records = managed_history(&wb.db, crate::PROJECT_ID, 10).unwrap();
    let got: Vec<_> = records.iter().map(|r| (r.scope, r.evidence)).collect();
    assert_eq!(
        got,
        vec![
            (ManagedScope::InScope, ManagedEvidence::ModelChoice),
            (ManagedScope::InScope, ManagedEvidence::RealCheck),
            (ManagedScope::OutOfScope, ManagedEvidence::HostRule),
        ]
    );
}

/// review R2：报告行的下一步读卡的当前状态——仍在等的卡定位卡，已结的授权卡按 grant_kind 指到设置分区。
#[test]
fn history_records_carry_an_executable_next_step() {
    let (_dir, wb) = wb();
    let enqueue = |payload| {
        crate::cards::enqueue(
            &wb.db,
            crate::PROJECT_ID,
            None,
            crate::cards::CardKind::Grant,
            payload,
            None,
        )
        .unwrap()
    };
    let queued = enqueue(json!({"grant_kind":"mcp"}));
    let ended = enqueue(json!({"grant_kind":"skill"}));
    crate::cards::answer(&wb.db, &ended, "managed:d:1").unwrap();
    let decide = |card: &str, reason: &str, outcome: &str| {
        wb.db
            .append_event(
                crate::PROJECT_ID,
                EventKind::System,
                json!({"kind":"managed_decision","card_id":card,"card_kind":"grant",
                    "action":"incomplete","reason":reason,"via":"host","actor":"managed:d:1","outcome":outcome}),
                None,
                None,
            )
            .unwrap()
    };
    decide(
        &queued,
        "managed_route_failed:authentication",
        "failed: provider",
    );
    decide(&ended, "capability_not_installed", "incomplete");
    let done = decide("gone", "x", "granted");
    let records = managed_history(&wb.db, crate::PROJECT_ID, 10).unwrap();
    let got: Vec<_> = records
        .iter()
        .map(|r| (r.next.kind, r.next.target.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (ManagedNextKind::Timeline, done.to_string()),
            (ManagedNextKind::Skills, "skills".to_string()),
            (ManagedNextKind::OpenCard, queued.clone()),
        ]
    );
}

#[test]
fn export_writes_markdown_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.md");
    export_managed_report(&path, "# 托管报告\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 托管报告\n");
    assert!(export_managed_report(&dir.path().join("report.txt"), "x").is_err());
    assert!(export_managed_report(&dir.path().join("missing/report.md"), "x").is_err());
    assert!(export_managed_report(&path, &"x".repeat(EXPORT_MAX_BYTES + 1)).is_err());
}

proptest! {
    /// D12: the classifier never grants evidence or an out-of-scope verdict to a model's own choice.
    #[test]
    fn only_host_codes_earn_evidence_or_out_of_scope(
        action in prop::sample::select(vec!["accept","adopt","push","confirm","direct_use","deny","incomplete","run_checks","allow"]),
        reason in prop::sample::select(vec!["evidence_complete","independent_quality_qualified","configured_target_accepted_delivery",
            "host_verified_worktree_binding","installed_capability_direct_use","target_missing","credential_missing","checks_missing_or_stale","x"]),
        via in prop::sample::select(vec!["host","model","fallback"]),
    ) {
        let (scope, evidence) = classify(action, reason, via);
        if via != "host" {
            prop_assert_eq!(scope, ManagedScope::InScope);
            prop_assert_eq!(evidence, ManagedEvidence::ModelChoice);
        }
        if scope == ManagedScope::OutOfScope {
            prop_assert!(OUT_OF_SCOPE_REASONS.contains(&reason));
        }
    }
}
