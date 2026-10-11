//! Managed-followups ticket 05 (spec T3): the report's external-operations
//! list. Every delegated app/MCP/browser action is listed with its target,
//! operation, decision and result and is marked unverified; a host trash is
//! marked recoverable with its Trash location. Owner decisions are not
//! listed, and no typed text, URL path or tool output leaks into an entry.
use super::*;
use crate::provider::{ModelProvider, ScriptedProvider};
use crate::tools::{CallOutcome, RiskClass, Tool, ToolContext, ToolError};
use crate::turn::text_response;
use serde_json::{json, Value};

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
        Ok(json!({"body":"tool output must not reach the report"}))
    }
}

fn asked(wb: &Workbench, tool: &str, input: Value) -> String {
    let ctx = wb.ctx_for("a0", None);
    match wb.registry.call(&wb.db, &ctx, tool, input).unwrap() {
        CallOutcome::Asked(qid) => qid,
        other => panic!("{tool}: expected a card, got {other:?}"),
    }
}

fn session() -> String {
    "a".repeat(36)
}

#[test]
fn delegated_app_mcp_and_browser_actions_are_listed_as_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    std::fs::create_dir_all(dir.path().join(".hexagon")).unwrap();
    std::fs::write(
        dir.path().join(".hexagon/mcp.json"),
        json!([{"name":"figma","command":"node"}]).to_string(),
    )
    .unwrap();
    wb.registry.register(FakeMcp);
    // An owner-decided card first: it is not a delegated action.
    let owner = asked(
        &wb,
        "browser_action",
        json!({"op":"navigate","session_id":session(),
        "snapshot_id":"bs1-x","url":"https://owner.example/x"}),
    );
    wb.answer_permission(&owner, false, None, "once").unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    let browse = asked(
        &wb,
        "browser_action",
        json!({"op":"navigate","session_id":session(),
        "snapshot_id":"bs1-x","url":"https://www.figma.com/file/abc?token=secret-token"}),
    );
    let typed = asked(
        &wb,
        "browser_action",
        json!({"op":"type","session_id":session(),
        "snapshot_id":"bs1-x","element_id":"be1","text":"typed-secret"}),
    );
    let mcp = asked(&wb, "mcp:figma:update_node", json!({"node":"1:2"}));
    let native = asked(
        &wb,
        "computer_action",
        json!({"op":"activate","app_id":"org.blenderfoundation.blender"}),
    );
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(
            ["deny", "deny", "allow", "deny"]
                .map(text_response)
                .to_vec(),
        )) as Arc<dyn ModelProvider>,
    );
    assert_eq!(wb.managed_tick().unwrap().len(), 4);
    let ops = managed_external_ops(&wb.db, crate::PROJECT_ID, 20).unwrap();
    assert!(
        ops.iter().all(|op| op.card_id != owner),
        "owner decisions are not listed"
    );
    let by_card = |card: &str| ops.iter().find(|op| op.card_id == card).unwrap().clone();
    let entry = by_card(&browse);
    assert_eq!(
        (
            entry.kind.as_str(),
            entry.target.as_str(),
            entry.action.as_str(),
            entry.decision.as_str()
        ),
        ("browser", "www.figma.com", "navigate", "deny")
    );
    assert_eq!(by_card(&typed).target, "current_page");
    let entry = by_card(&mcp);
    assert_eq!(
        (
            entry.kind.as_str(),
            entry.target.as_str(),
            entry.action.as_str(),
            entry.decision.as_str()
        ),
        ("mcp", "figma:update_node", "update_node", "allow")
    );
    let entry = by_card(&native);
    assert_eq!(
        (
            entry.kind.as_str(),
            entry.target.as_str(),
            entry.action.as_str()
        ),
        ("app", "org.blenderfoundation.blender", "activate")
    );
    for op in &ops {
        assert_eq!(op.verification, "host_unverified_deletion");
        assert!(op.trash_path.is_none());
        // Each entry points at its delegated decision row on the timeline.
        let history = managed_history(&wb.db, crate::PROJECT_ID, 20).unwrap();
        let row = history.iter().find(|r| r.event_id == op.event_id).unwrap();
        assert_eq!(row.card_id, op.card_id);
    }
    // The real result: the allowed MCP call never ran here (no task to
    // continue), so it is not reported as done.
    assert_ne!(by_card(&mcp).result, "done");
    let wire = serde_json::to_string(&ops).unwrap();
    for leak in ["secret-token", "typed-secret", "/file/abc", "tool output"] {
        assert!(!wire.contains(leak), "{leak} leaked into the report");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_delegated_trash_is_listed_as_recoverable_with_its_trash_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = outside
        .path()
        .join(format!("report-{}.txt", std::process::id()));
    std::fs::write(&file, "x").unwrap();
    let file = file.canonicalize().unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![crate::turn::tool_response(
            vec![("c1", "host_fs_trash", json!({"path": file}))],
        )])),
    );
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    assert!(matches!(
        wb.run_instance("a0", "remove the stale export").unwrap(),
        TurnOutcome::AwaitingPermission(_)
    ));
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![
            text_response("allow"),
            text_response("done"),
        ])),
    );
    wb.managed_tick().unwrap();
    let ops = managed_external_ops(&wb.db, crate::PROJECT_ID, 20).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, "local_trash");
    assert_eq!(ops[0].target, file.to_string_lossy());
    assert_eq!(ops[0].verification, "host_verified_recoverable");
    assert_eq!(ops[0].result, "done");
    let in_trash = PathBuf::from(ops[0].trash_path.clone().unwrap());
    assert!(in_trash.exists());
    std::fs::rename(&in_trash, &file).unwrap();
}

#[test]
fn a_refused_trash_says_nothing_was_moved() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker"], None).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("keep.txt");
    std::fs::write(&file, "x").unwrap();
    crate::delegation::enable(&wb.db, crate::PROJECT_ID).unwrap();
    asked(
        &wb,
        "host_fs_trash",
        json!({"path": file.canonicalize().unwrap()}),
    );
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![text_response("deny")])),
    );
    wb.managed_tick().unwrap();
    let ops = managed_external_ops(&wb.db, crate::PROJECT_ID, 20).unwrap();
    assert_eq!(
        (ops[0].decision.as_str(), ops[0].verification.as_str()),
        ("deny", "host_verified_not_moved")
    );
    assert!(ops[0].trash_path.is_none());
    assert!(file.exists());
}
