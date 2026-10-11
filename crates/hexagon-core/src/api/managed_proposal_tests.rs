//! Test3/20: delegated proposal decisions. Model advice never completes a
//! policy decision; adoption re-validates the change and records the grant.
use super::super::tests::{git_wb, proposal_body, proposal_status, submit_proposal, PROPOSAL_DIFF};
use super::*;
use crate::provider::ScriptedProvider;
use crate::turn::text_response;
use serde_json::json;

fn awaiting(wb: &Workbench) -> String {
    let pid = submit_proposal(wb, "art1", "agents_md", "AGENTS.md", PROPOSAL_DIFF).unwrap();
    wb.review_proposal(&pid, true, "ok").unwrap();
    assert_eq!(proposal_status(wb, &pid), "awaiting_stamp");
    pid
}

fn replies(wb: &mut Workbench, texts: &[&str]) -> Arc<ScriptedProvider> {
    let chat = Arc::new(ScriptedProvider::new(
        texts.iter().map(|t| text_response(t)).collect(),
    ));
    wb.register_provider("default", chat.clone());
    chat
}

fn by_of(wb: &Workbench, kind: EventKind) -> Vec<String> {
    wb.db
        .events(crate::PROJECT_ID, Some(&[kind]))
        .unwrap()
        .into_iter()
        .map(|e| e.payload["by"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_model_adopt_applies_the_change_under_the_grant_actor() {
    let (dir, mut wb) = git_wb(&["前端", "前端技术负责人"]);
    let pid = awaiting(&wb);
    replies(&mut wb, &["adopt"]);
    let status = crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].action, "adopt", "{done:?}");
    assert_eq!(proposal_status(&wb, &pid), "active");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "line1\nline2\n"
    );
    let actor = format!(
        "managed:{}:{}",
        status.delegation_id.unwrap(),
        status.generation
    );
    assert_eq!(by_of(&wb, EventKind::ProposalStamped), vec![actor.clone()]);
    assert_eq!(by_of(&wb, EventKind::ProposalActivated), vec![actor]);
}

#[test]
fn a_model_reject_or_failure_leaves_the_file_untouched() {
    for reply in [Some("reject"), Some("looks fine to me"), None] {
        let (dir, mut wb) = git_wb(&["前端", "前端技术负责人"]);
        let pid = awaiting(&wb);
        if let Some(text) = reply {
            replies(&mut wb, &[text]);
        }
        crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
        let done = wb.managed_tick().unwrap();
        assert_eq!(done[0].action, "reject", "{reply:?}: {done:?}");
        assert_eq!(proposal_status(&wb, &pid), "rejected");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            "line1\n"
        );
        assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn a_conflicting_change_is_never_applied_and_ends_rejected_within_budget() {
    let (dir, mut wb) = git_wb(&["前端", "前端技术负责人"]);
    let pid = awaiting(&wb);
    // The target moved on: the diff no longer applies.
    std::fs::write(dir.path().join("AGENTS.md"), "rewritten by someone else\n").unwrap();
    replies(&mut wb, &["adopt", "adopt", "adopt"]);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    for _ in 0..crate::delegation::MAX_ROUNDS {
        assert_eq!(wb.managed_tick().unwrap()[0].action, "adopt");
        assert!(managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap()[0]
            .outcome
            .starts_with("failed"));
        assert_eq!(proposal_status(&wb, &pid), "awaiting_stamp");
    }
    let last = wb.managed_tick().unwrap();
    assert_eq!(
        (last[0].action.as_str(), last[0].reason.as_str()),
        ("reject", "budget_exhausted")
    );
    assert_eq!(proposal_status(&wb, &pid), "rejected");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "rewritten by someone else\n"
    );
}

#[test]
fn an_unverified_policy_candidate_is_never_adopted() {
    let (dir, mut wb) = git_wb(&["流程优化", "前端技术负责人"]);
    wb.db
        .conn()
        .execute(
            "INSERT INTO role_defs (project_id, name, reviewer) VALUES ('p1','流程优化','前端技术负责人')",
            [],
        )
        .unwrap();
    let mut content = proposal_body(
        "pack_copy",
        ".hexagon/pack.active.json",
        "+ knobs.flag_patience: 2 → 5",
    );
    content.push_str(
        "\n```replay\n{\"schema\":1,\"scenario_fingerprint\":\"scene-7\",\"baseline_pack\":\"now\",\"candidate_pack\":\"next\",\"baseline\":{\"stages_done\":0},\"candidate\":{\"stages_done\":1}}\n```\n",
    );
    content.push_str(
        "\n```judge\n{\"verdict\":\"needs-human\",\"rationale\":\"look\",\"backend\":\"mechanical\"}\n```\n",
    );
    let baseline: PackDef =
        serde_json::from_value(json!({"name":"policy","version":1,"stages":[]})).unwrap();
    baseline.pin(dir.path()).unwrap();
    let mut candidate = baseline.clone();
    candidate.knobs.flag_patience = Some(5);
    content.push_str(&format!(
        "\n```policy\n{}\n```\n",
        json!({"baseline":baseline,"candidate":candidate})
    ));
    std::fs::create_dir_all(dir.path().join(".hexagon/props")).unwrap();
    std::fs::write(dir.path().join(".hexagon/props/pack.md"), &content).unwrap();
    wb.db
        .conn()
        .execute(
            "INSERT INTO artifacts (id, project_id, path, kind, tier, author_agent_id, version)
             VALUES ('art-pack','p1','props/pack.md','改进提案','parse','a0',1)",
            [],
        )
        .unwrap();
    let ctx = wb.ctx_for("a0", None);
    let pid = crate::proposals::submit(&wb.db, &ctx, "art-pack", &content).unwrap();
    assert_eq!(proposal_status(&wb, &pid), "awaiting_stamp");
    let policy = dir.path().join(".hexagon/pack.active.json");
    let before = std::fs::read(&policy).unwrap();
    let chat = replies(&mut wb, &["adopt"]);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("incomplete", "independent_quality_missing")
    );
    assert!(
        chat.recorded().is_empty(),
        "the model's advice is never asked for policy"
    );
    assert_eq!(std::fs::read(&policy).unwrap(), before);
    assert_eq!(
        proposal_status(&wb, &pid),
        "awaiting_stamp",
        "not adopted, not rewritten"
    );
    assert!(
        crate::cards::queued(&wb.db, crate::PROJECT_ID)
            .unwrap()
            .is_empty(),
        "no owner card left"
    );
}
