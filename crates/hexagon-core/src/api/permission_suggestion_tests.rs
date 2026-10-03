use super::*;
use proptest::prelude::*;
use serde_json::{json, Value};

fn suggestion(
    wb: &Workbench,
    tool: &str,
    input: Value,
) -> Option<crate::permission_suggestion::PermissionShapeSuggestion> {
    let id = crate::cards::enqueue(
        &wb.db,
        &wb.project_id,
        Some("a0"),
        crate::cards::CardKind::Permission,
        json!({"tool":tool,"input":input,"raw_input":input,"safety_net":false}),
        None,
    )
    .unwrap();
    permission_shape_suggestion(&wb.db, &wb.project_id, &id).unwrap()
}

#[test]
fn permission_suggestion_prefills_bounded_web_family_without_granting_it() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
    let proposal = suggestion(
        &wb,
        "web_fetch",
        json!({"url":"https://example.com/docs/start"}),
    )
    .unwrap();
    assert_eq!(proposal.shape, "https://example.com/docs/*");
    assert!(proposal.generalized);
    assert_eq!(proposal.agent_id, "a0");
    assert!(crate::permissions::list_rules(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
    let ctx = wb.ctx_for("a0", None);
    let input = json!({"url":"https://example.com/docs/start"});
    crate::permissions::persist_rule(
        &wb.db,
        &ctx,
        "web_fetch",
        &input,
        Some(&proposal.shape),
        "project",
        None,
    )
    .unwrap();
    assert!(
        crate::permissions::matching_rule(&wb.db, &ctx, "web_fetch", &input, "allow")
            .unwrap()
            .is_some()
    );
    let other = wb.ctx_for("a1", None);
    assert!(
        crate::permissions::matching_rule(&wb.db, &other, "web_fetch", &input, "allow")
            .unwrap()
            .is_none()
    );
}

#[test]
fn permission_suggestion_exact_shell_does_not_generalize_test_into_any_command() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    let proposal = suggestion(&wb, "bash", json!({"cmd":"npm test -- --run"})).unwrap();
    assert_eq!(proposal.shape, "exact:npm test -- --run");
    assert!(!proposal.generalized);
    assert!(suggestion(&wb, "bash", json!({"cmd":"* test"})).is_none());

    assert!(crate::permissions::shape_matches(
        &proposal.shape,
        "bash",
        &json!({"cmd":"npm test -- --run"})
    ));
    assert!(!crate::permissions::shape_matches(
        &proposal.shape,
        "bash",
        &json!({"cmd":"npm publish"})
    ));
    assert!(suggestion(&wb, "bash", json!({"cmd":"git push origin main"})).is_none());
    assert!(suggestion(&wb, "mcp:mail:send", json!({"body":"hello"})).is_none());
    assert!(crate::permissions::persist_rule(
        &wb.db,
        &wb.ctx_for("a0", None),
        "bash",
        &json!({"cmd":"npm test"}),
        Some("*"),
        "project",
        None
    )
    .is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn permission_suggestion_generalized_url_keeps_origin_and_parent_path(
        name in "[a-z]{1,10}",
        tail in "[a-z]{1,10}",
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let input = json!({"url":format!("https://example.com/docs/{name}")});
        let proposal = suggestion(&wb, "web_fetch", input.clone()).unwrap();
        prop_assert!(crate::permissions::shape_matches(&proposal.shape, "web_fetch", &input));
        for url in [format!("https://evil.example.com/docs/{tail}"), format!("http://example.com/docs/{tail}"),
            format!("https://example.com/other/{tail}"), format!("https://example.com/docs/../other/{tail}")] {
            prop_assert!(!crate::permissions::shape_matches(&proposal.shape, "web_fetch", &json!({"url":url})), "must stay within origin and path");
        }
    }

    #[test]
    fn permission_suggestion_exact_shell_never_accepts_appended_action(
        name in "[a-z]{1,10}", sep in prop::sample::select(vec![";", " && ", " | ", "\n"]),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let command = format!("npm test -- {name}");
        let proposal = suggestion(&wb, "bash", json!({"cmd":command})).unwrap();
        prop_assert!(!crate::permissions::shape_matches(&proposal.shape, "bash", &json!({"cmd":format!("{command}{sep}echo surprise")})), "must not append an action");
    }
    #[test]
    fn permission_suggestion_edited_rule_cannot_replace_the_executable(program in "[a-z]{1,12}") {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let result = crate::permissions::persist_rule(&wb.db, &wb.ctx_for("a0", None), "bash",
            &json!({"cmd":format!("{program} test")}), Some("* test"), "project", None);
        prop_assert!(result.unwrap_or(None).is_none(), "edited rule must retain executable");
        prop_assert!(crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap().is_empty());
    }

}

// ADR 0079: exercise the real queued action/owner decision/read-model seam.
fn queued_command(wb: &Workbench, agent: &str, command: &str) -> String {
    match wb
        .registry
        .call(
            &wb.db,
            &wb.ctx_for(agent, None),
            "bash",
            json!({"cmd":command}),
        )
        .unwrap()
    {
        crate::tools::CallOutcome::Asked(id) => id,
        other => panic!("expected permission card: {other:?}"),
    }
}

#[test]
fn project_permission_shares_with_peers_rechecks_queue_and_can_be_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
    let first = queued_command(&wb, "a0", "printf project");
    let peer = queued_command(&wb, "a1", "printf project");
    wb.allow_project_permission(&first).unwrap();
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
    let events = wb
        .db
        .events(&wb.project_id, Some(&[EventKind::PermissionAllowed]))
        .unwrap();
    assert!(events
        .iter()
        .any(|e| e.payload["question_id"] == peer && e.payload["via"] == "project_permission"));
    let rules = crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert!(rules[0].project_shared);
    assert_eq!(rules[0].agent_id, None);
    assert_eq!(rules[0].scope, "project");
    assert!(
        wb.allow_project_permission(&first).is_err(),
        "duplicate clicks cannot execute twice"
    );
    crate::permissions::revoke_rule(&wb.db, &wb.project_id, &rules[0].id).unwrap();
    queued_command(&wb, "a1", "printf project");
}

#[test]
fn project_permission_file_scope_is_exact_and_scrubbed_content_does_not_hide_it() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
    let id = crate::cards::enqueue(
        &wb.db,
        &wb.project_id,
        Some("a0"),
        crate::cards::CardKind::Permission,
        json!({"tool":"fs_write", "input":{"path":"src/a.ts","content":"[scrubbed]"},
            "raw_input":{"path":"src/a.ts","content":"hello"}}),
        None,
    )
    .unwrap();
    let proposal = permission_shape_suggestion(&wb.db, &wb.project_id, &id)
        .unwrap()
        .unwrap();
    assert_eq!(proposal.shape, "exact:src/a.ts");
    assert!(!proposal.generalized);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn project_permission_never_widens_file_target(name in "[a-z]{1,12}") {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let path = format!("src/{name}.ts");
        let proposal = suggestion(&wb, "fs_write", json!({"path":path,"content":"a"})).unwrap();
        prop_assert!(crate::permissions::shape_matches(&proposal.shape,"fs_write",&json!({"path":path,"content":"changed"})), "same file may change content");
        prop_assert!(!crate::permissions::shape_matches(&proposal.shape,"fs_write",&json!({"path":format!("src/{name}other.ts"),"content":"a"})), "another file needs permission");
    }
}

#[test]
fn project_permission_keeps_queued_network_upgrade_and_current_denial() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA", "前端"], None).unwrap();
    let first = queued_command(&wb, "a0", "printf scope");
    let denied = queued_command(&wb, "a1", "printf scope");
    let network = match wb
        .registry
        .call(
            &wb.db,
            &wb.ctx_for("a2", None),
            "bash",
            json!({"cmd":"printf scope", "net":true}),
        )
        .unwrap()
    {
        crate::tools::CallOutcome::Asked(id) => id,
        other => panic!("expected network card: {other:?}"),
    };
    // Permission changes while a card is queued must be observed by the sweep.
    wb.db.conn().execute("INSERT INTO permission_rules(id,project_id,agent_id,tool,shape,effect,scope) VALUES('late-deny',?1,'a1','bash','*','deny','project')",[&wb.project_id]).unwrap();
    wb.allow_project_permission(&first).unwrap();
    let remaining = crate::cards::queued(&wb.db, &wb.project_id).unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().any(|q| q.id == denied));
    assert!(remaining.iter().any(|q| q.id == network));
}

#[test]
fn legacy_rules_keep_their_owner_and_scope_after_creating_shared_permission() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
    wb.db.conn().execute("INSERT INTO permission_rules(id,project_id,agent_id,tool,shape,effect,scope) VALUES('old',?1,'a0','bash','printf legacy','allow','project')",[&wb.project_id]).unwrap();
    queued_command(&wb, "a1", "printf legacy");
    let first = queued_command(&wb, "a0", "printf new");
    wb.allow_project_permission(&first).unwrap();
    let rules = crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap();
    let old = rules.iter().find(|r| r.id == "old").unwrap();
    assert!(!old.project_shared);
    assert_eq!(old.agent_id.as_deref(), Some("a0"));
    assert_eq!(old.scope, "project");
    assert_eq!(
        crate::cards::queued(&wb.db, &wb.project_id).unwrap().len(),
        1
    );
}

#[test]
fn single_owner_approval_never_remembers_declared_check_at_api_seam() {
    let dir = tempfile::tempdir().unwrap();
    let pack = serde_json::from_value(json!({"name":"t","version":1,"stages":[{"name":"实现","roles":["后端"],"due":[],"checks":["printf check"]}]})).unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端"], Some(pack)).unwrap();
    let first = queued_command(&wb, "a0", "printf check");
    wb.answer_permission(&first, true, None, "activation")
        .unwrap();
    assert!(crate::permissions::list_rules(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
    queued_command(&wb, "a0", "printf check");
}

struct FailFirstCommand(std::sync::atomic::AtomicBool);
impl crate::tools::Tool for FailFirstCommand {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "test command transport"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Exec
    }
    fn exec(
        &self,
        _: &crate::db::Db,
        _: &Value,
        _: &crate::tools::ToolContext,
    ) -> Result<Value, crate::tools::ToolError> {
        if self.0.swap(false, std::sync::atomic::Ordering::SeqCst) {
            Err(crate::tools::ToolError::NotExecuted(
                "transport unavailable before dispatch".into(),
            ))
        } else {
            Ok(json!({"exit_code":0}))
        }
    }
}

#[test]
fn project_permission_sweeps_peers_even_when_first_execution_fails() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
    wb.registry
        .register(FailFirstCommand(std::sync::atomic::AtomicBool::new(true)));
    let first = queued_command(&wb, "a0", "printf failure");
    let peer = queued_command(&wb, "a1", "printf failure");
    assert!(wb.allow_project_permission(&first).is_err());
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
    assert!(wb
        .db
        .events(&wb.project_id, Some(&[EventKind::PermissionAllowed]))
        .unwrap()
        .iter()
        .any(|e| e.payload["question_id"] == peer && e.payload["via"] == "project_permission"));
}

#[test]
fn project_permission_reopen_keeps_shared_scope() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::open_scoped(
        dir.path(),
        "test",
        &[("a0".into(), "后端".into()), ("a1".into(), "QA".into())],
        None,
        false,
    )
    .unwrap();
    let first = queued_command(&wb, "a0", "printf persistent");
    wb.allow_project_permission(&first).unwrap();
    drop(wb);
    let wb = Workbench::open_scoped(dir.path(), "test", &[], None, false).unwrap();
    let result = wb
        .registry
        .call(
            &wb.db,
            &wb.ctx_for("a1", None),
            "bash",
            json!({"cmd":"printf persistent"}),
        )
        .unwrap();
    assert!(matches!(result, crate::tools::CallOutcome::Done(_)));
    assert!(crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap()[0].project_shared);
}

struct QueuedWrite;
impl crate::tools::Tool for QueuedWrite {
    fn name(&self) -> &str {
        "fs_write"
    }
    fn description(&self) -> &str {
        "write transport requiring permission"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn write_targets(&self, input: &Value) -> Result<Vec<String>, crate::tools::ToolError> {
        Ok(vec![input["path"].as_str().unwrap().into()])
    }
    fn exec(
        &self,
        _: &crate::db::Db,
        input: &Value,
        ctx: &crate::tools::ToolContext,
    ) -> Result<Value, crate::tools::ToolError> {
        std::fs::write(
            ctx.repo_root.join(input["path"].as_str().unwrap()),
            input["content"].as_str().unwrap(),
        )?;
        Ok(json!({"ok":true}))
    }
}

#[test]
fn project_permission_retains_stale_write_and_unrelated_grants_cannot_release_it() {
    let dir = tempfile::tempdir().unwrap();
    let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
    wb.registry.register(QueuedWrite);
    let queue = |agent, text| match wb
        .registry
        .call(
            &wb.db,
            &wb.ctx_for(agent, None),
            "fs_write",
            json!({"path":"a.ts","content":text}),
        )
        .unwrap()
    {
        crate::tools::CallOutcome::Asked(id) => id,
        other => panic!("expected card: {other:?}"),
    };
    let first = queue("a0", "first");
    let peer = queue("a1", "second");
    wb.allow_project_permission(&first).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.ts")).unwrap(),
        "first"
    );
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .any(|q| q.id == peer));
    // Restoring the old snapshot does not make an unrelated new grant consent.
    std::fs::remove_file(dir.path().join("a.ts")).unwrap();
    let unrelated = queued_command(&wb, "a0", "printf unrelated");
    wb.allow_project_permission(&unrelated).unwrap();
    assert!(!dir.path().join("a.ts").exists());
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .iter()
        .any(|q| q.id == peer));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn project_permission_execution_scope_never_expands(net in any::<bool>(), background in any::<bool>(), session in any::<bool>()) {
        let dir = tempfile::tempdir().unwrap();
        let wb = Workbench::for_test(dir.path(), &["后端", "QA"], None).unwrap();
        let input = json!({"cmd":"printf scope"});
        let id = crate::permissions::persist_rule(&wb.db, &wb.ctx_for("a0", None), "bash", &input, Some("exact:printf scope"), "project_shared", None).unwrap().unwrap();
        let mut next = json!({"cmd":"printf scope", "net":net, "background":background});
        if session { next["session"] = json!("other"); }
        let matched = crate::permissions::matches_project_permission(&wb.db, &wb.ctx_for("a1", None), "bash", &next, &id).unwrap();
        prop_assert_eq!(matched, !net && !background && !session);
    }
}

#[test]
fn project_permission_continues_every_resolved_agent_without_replaying_effects() {
    use crate::provider::ScriptedProvider;
    use crate::turn::{text_response, tool_response};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["worker", "tester"], None).unwrap();
    wb.registry
        .register(FailFirstCommand(std::sync::atomic::AtomicBool::new(true)));
    let provider = Arc::new(ScriptedProvider::new(vec![
        text_response("plan first"),
        tool_response(vec![("first", "bash", json!({"cmd":"printf resume"}))]),
        text_response(r#"{"verdict":"unsure","reason":"owner decision"}"#),
        text_response("plan second"),
        tool_response(vec![("second", "bash", json!({"cmd":"printf resume"}))]),
        text_response(r#"{"verdict":"unsure","reason":"owner decision"}"#),
        text_response("continued first"),
        text_response("continued second"),
    ]));
    wb.register_provider("default", provider.clone());
    let TurnOutcome::AwaitingPermission(first) =
        wb.dispatch("worker", "first original task", &[]).unwrap()
    else {
        panic!("first must wait")
    };
    let second = wb.dispatch("tester", "second original task", &[]).unwrap();
    assert!(
        matches!(second, TurnOutcome::AwaitingPermission(_)),
        "{second:?}"
    );
    assert!(
        wb.allow_project_permission_and_continue(&first).is_err(),
        "report original execution failure after continuing peers"
    );
    let recorded = provider.recorded();
    assert_eq!(recorded.len(), 8);
    for (request, task) in recorded[6..]
        .iter()
        .zip(["first original task", "second original task"])
    {
        let history = serde_json::to_string(&request.messages).unwrap();
        assert!(history.contains(task));
        assert!(history.contains("tool_result"));
    }
    assert_eq!(
        wb.db
            .events(&wb.project_id, Some(&[EventKind::ToolCalled]))
            .unwrap()
            .len(),
        2
    );
    assert!(crate::cards::queued(&wb.db, &wb.project_id)
        .unwrap()
        .is_empty());
}
