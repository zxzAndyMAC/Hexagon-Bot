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
        prop_assert!(!result.unwrap_or(false), "edited rule must retain executable");
        prop_assert!(crate::permissions::list_rules(&wb.db, &wb.project_id).unwrap().is_empty());
    }

}
