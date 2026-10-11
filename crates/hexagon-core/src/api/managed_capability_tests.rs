//! Managed-followups ticket 01 (ADR0087 完全放权): under active delegation,
//! external apps, browser page actions and MCP take the delegated permission
//! decision instead of a forced refusal; installed MCP and skills are usable
//! by any role without a grant card or grants row; tool-layer hard refusals
//! are untouched; pausing or turning delegation off restores the old checks.
use super::*;
use crate::provider::{ModelProvider, ScriptedProvider};
use crate::tools::{CallOutcome, RiskClass, Tool, ToolContext, ToolError};
use crate::turn::text_response;
use proptest::prelude::*;
use serde_json::{json, Value};

fn grant_rows(wb: &Workbench) -> i64 {
    wb.db
        .conn()
        .query_row("SELECT COUNT(*) FROM grants", [], |r| r.get(0))
        .unwrap()
}

fn install_mcp(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir.join(".hexagon")).unwrap();
    std::fs::write(
        dir.join(".hexagon/mcp.json"),
        json!([{"name": name, "command": "node", "args": ["server.js"]}]).to_string(),
    )
    .unwrap();
}

/// Stand-in for a registered third-party MCP tool (never executed here).
struct FakeMcp;
impl Tool for FakeMcp {
    fn name(&self) -> &str {
        "mcp:figma:update_node"
    }
    fn description(&self) -> &str {
        "fake"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::External
    }
    fn exec(&self, _: &Db, _: &Value, _: &ToolContext) -> Result<Value, ToolError> {
        Ok(json!({}))
    }
}

fn grant_gate(wb: &Workbench) -> crate::permissions::Decision {
    crate::permissions::evaluate(
        &wb.db,
        &wb.ctx_for("a0", None),
        &FakeMcp,
        "mcp:figma:update_node",
        &json!({}),
    )
    .unwrap()
}

fn denied_by_grant(decision: &crate::permissions::Decision) -> bool {
    matches!(
        decision,
        crate::permissions::Decision::Deny { layer: "grant", .. }
    )
}

#[test]
fn an_installed_mcp_needs_no_grant_only_while_delegation_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    install_mcp(dir.path(), "figma");
    assert!(denied_by_grant(&grant_gate(&wb)), "no grant, no delegation");
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let decision = grant_gate(&wb);
    assert!(!denied_by_grant(&decision), "{decision:?}");
    assert_eq!(grant_rows(&wb), 0, "no persistent grant");
    crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, true).unwrap();
    assert!(
        denied_by_grant(&grant_gate(&wb)),
        "paused restores the check"
    );
    crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, false).unwrap();
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(denied_by_grant(&grant_gate(&wb)), "off restores the check");
}

#[test]
fn an_mcp_that_is_not_installed_still_needs_its_grant_under_delegation() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    install_mcp(dir.path(), "other");
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(denied_by_grant(&grant_gate(&wb)));
}

#[test]
fn installed_skills_and_mcp_need_no_grant_card_while_delegating() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    std::fs::create_dir_all(dir.path().join(".hexagon/skills/brand-kit")).unwrap();
    install_mcp(dir.path(), "figma");
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    for (kind, name) in [("skill", "brand-kit"), ("mcp", "figma")] {
        let outcome = wb.request_grant("a0", kind, name).unwrap();
        assert!(outcome.granted, "{kind}");
        assert_eq!(outcome.via, "managed_installed");
        assert!(outcome.question_id.is_none());
    }
    assert_eq!(grant_rows(&wb), 0);
    assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID)
        .unwrap()
        .is_empty());
    crate::delegation::disable(&wb.db, crate::PROJECT_ID).unwrap();
    let outcome = wb.request_grant("a0", "skill", "brand-kit").unwrap();
    assert_eq!(outcome.via, "queued", "back to the owner's grant card");
}

#[test]
fn a_grant_card_queued_before_delegation_ends_as_direct_use_without_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    install_mcp(dir.path(), "figma");
    let queued = wb.request_grant("a0", "mcp", "figma").unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let done = wb.managed_tick().unwrap();
    assert_eq!(
        (done[0].action.as_str(), done[0].reason.as_str()),
        ("direct_use", "installed_capability_direct_use")
    );
    let card = crate::cards::get(&wb.db, &queued.question_id.unwrap()).unwrap();
    assert_eq!(card.state, crate::cards::CardState::Answered);
    assert_eq!(grant_rows(&wb), 0);
    let history = managed_history(&wb.db, crate::PROJECT_ID, 1).unwrap();
    assert_eq!(history[0].result, "done");
}

#[test]
fn a_delegated_browser_action_is_the_model_choice_not_a_forced_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let ctx = wb.ctx_for("a0", None);
    let input = json!({"op":"navigate","session_id":"a".repeat(36),"snapshot_id":"bs1-x",
        "url":"https://www.figma.com/file/x"});
    let CallOutcome::Asked(qid) = wb
        .registry
        .call(&wb.db, &ctx, "browser_action", input)
        .unwrap()
    else {
        panic!("a browser mutation asks")
    };
    let chat = Arc::new(ScriptedProvider::new(vec![text_response("deny")]));
    wb.register_provider("default", chat.clone() as Arc<dyn ModelProvider>);
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let plan = wb.managed_plan().unwrap();
    assert!(plan[0].forced.is_none(), "{:?}", plan[0].forced);
    assert!(plan[0].state.contains(routes::DELEGATED_CONSTRAINTS));
    let done = wb.managed_tick().unwrap();
    assert_eq!(done[0].via, "model");
    assert_eq!(done[0].reason, "model_choice");
    assert_eq!(chat.recorded().len(), 1, "the model was asked");
    let card = crate::cards::get(&wb.db, &qid).unwrap();
    assert!(card.answered_by.unwrap().starts_with("managed:"));
}

fn hard_refused(op: &str) -> Value {
    match op {
        "file_url" => {
            json!({"op":"navigate","session_id":"a".repeat(36),"snapshot_id":"bs1-x","url":"file:///etc/hosts"})
        }
        "script_url" => {
            json!({"op":"navigate","session_id":"a".repeat(36),"snapshot_id":"bs1-x","url":"javascript:alert(1)"})
        }
        other => {
            json!({"op":other,"session_id":"a".repeat(36),"snapshot_id":"bs1-x","element_id":"be1"})
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    // Ticket 01 rewrite of "external is always refused" (ADR0087): the hard
    // refusal set — non-HTTP/script destinations, uploads, scripts and
    // subagent desktop/browser control — is refused under delegation too,
    // never turned into a card the delegated decision could allow.
    #[test]
    fn the_hard_refusal_set_stays_refused_under_delegation(
        op in prop::sample::select(vec!["file_url", "script_url", "upload", "eval", "set_file"]),
        child in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
        crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
        let mut ctx = wb.ctx_for("a0", None);
        if child {
            ctx.subagent = Some(crate::subagent::Scope {
                approval_mode: crate::approval_mode::ApprovalMode::Broad,
                permission_rules: Default::default(),
                halt: Default::default(),
                answer: Default::default(),
                mcp: Default::default(),
                reads: Default::default(),
            });
        }
        for tool in ["browser_action", "computer_action"] {
            let outcome = wb.registry.call(&wb.db, &ctx, tool, hard_refused(op));
            prop_assert!(
                !matches!(outcome, Ok(CallOutcome::Asked(_)) | Ok(CallOutcome::Done(_))),
                "{tool} {op} child={child}: {outcome:?}"
            );
        }
        prop_assert!(crate::cards::queued(&wb.db, crate::PROJECT_ID).unwrap().is_empty());
    }

    // Off or paused, the delegated processor holds nothing: an external
    // action's card stays with the owner (ADR0087: 放权只在托管期间有效).
    #[test]
    fn without_active_delegation_nothing_external_is_decided(paused in any::<bool>()) {
        let dir = tempfile::tempdir().unwrap();
        let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
        let ctx = wb.ctx_for("a0", None);
        let input = json!({"op":"navigate","session_id":"a".repeat(36),"snapshot_id":"bs1-x","url":"https://example.com"});
        let outcome = wb.registry.call(&wb.db, &ctx, "browser_action", input).unwrap();
        prop_assert!(matches!(outcome, CallOutcome::Asked(_)));
        if paused {
            crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
            crate::delegation::set_paused(&wb.db, crate::PROJECT_ID, true).unwrap();
        }
        prop_assert!(wb.managed_tick().unwrap().is_empty());
        prop_assert_eq!(crate::cards::queued(&wb.db, crate::PROJECT_ID).unwrap().len(), 1);
    }
}
