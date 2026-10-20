//! 彩蛋「画师」（owner-followups-1015 票 19）：调度方召唤画师；出图槽未绑 / 不支持时
//! 画师只回一句提示。断言面 = 门面读面（团队、花名册、预置角色、待决卡）与轨迹事实。
use super::*;
use crate::provider::ScriptedProvider;
use crate::provider_config::{ProviderDef, ProviderDoc, SlotBinding};
use crate::turn::{text_response, tool_response};
use serde_json::Value;
use std::sync::Arc;

const MISSING: &str = "artist_image_model_missing";

fn doc_with(slots: &[(&str, &str)]) -> ProviderDoc {
    let mut doc = ProviderDoc::default();
    doc.providers.push(ProviderDef {
        id: "p".into(),
        name: "P".into(),
        kind: crate::provider::ProviderKind::OpenAi,
        base_url: "http://localhost".into(),
        models: vec![],
        enabled: true,
    });
    for (slot, model) in slots {
        doc.slots.insert(
            (*slot).into(),
            SlotBinding {
                provider_id: "p".into(),
                model: (*model).into(),
            },
        );
    }
    doc
}

/// 项目经理 a0 + 后端 a1；项目经理这一回合调一次 summon_artist。
fn summon(wb: &mut Workbench, agent: &str, input: Value) {
    orchestra::write_agent_status(&wb.db, "p1", agent, false).unwrap();
    wb.register_provider(
        "default",
        Arc::new(ScriptedProvider::new(vec![
            tool_response(vec![("s1", "summon_artist", input)]),
            text_response("ok"),
            // 项目经理说完没点名 → 封闭选择；先不派活。
            text_response(crate::pm_route::HOLD_TOKEN),
        ])),
    );
    wb.run_instance(agent, "给我的团队每个角色设置一个像素风格的头像")
        .unwrap();
}

fn missing_events(wb: &Workbench) -> Vec<Value> {
    wb.db
        .timeline("p1", None, 500, None)
        .unwrap()
        .into_iter()
        .filter(|it| it.event.kind == EventKind::System && it.event.payload["kind"] == MISSING)
        .map(|it| it.event.payload)
        .collect()
}

fn tool_result_error(wb: &Workbench) -> bool {
    wb.db
        .timeline("p1", None, 500, None)
        .unwrap()
        .into_iter()
        .find(|it| it.event.kind == EventKind::ToolResult)
        .map(|it| it.event.payload["is_error"] == true || it.event.payload["ok"] == false)
        .unwrap()
}

#[test]
fn unbound_image_slot_gets_one_artist_notice_and_artist_stays_off_team_surfaces() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["项目经理", "后端"], None).unwrap();
    // default 绑的是出图模型也不算：出图槽只认精确绑定（不回退 default）。
    let _doc = crate::provider_config::fixture_document(doc_with(&[("default", "gpt-image-1")]));
    summon(&mut wb, "a0", json!({"style": "pixel art"}));

    let notices = missing_events(&wb);
    assert_eq!(notices.len(), 1, "画师只回一条提示");
    assert_eq!(notices[0]["state"], "unconfigured");
    assert_eq!(notices[0]["speaker"], crate::artist::ARTIST_ROLE);
    assert_eq!(notices[0]["slot"], "image");
    assert!(
        crate::trace::SYSTEM_SUBKINDS.contains(&MISSING),
        "子类登记进轨迹词表"
    );
    assert!(
        crate::presets::is_hidden_role(crate::artist::ARTIST_ROLE),
        "隐藏标记"
    );

    let mut roles: Vec<String> = orchestra::team(&wb.db, &wb.project_id, &wb.repo_root)
        .unwrap()
        .into_iter()
        .map(|r| r.role)
        .collect();
    roles.sort();
    assert_eq!(roles, vec!["后端", "项目经理"], "画师不进团队读面");
    assert!(!crate::roles::team_roles(&wb.db, "p1")
        .unwrap()
        .iter()
        .any(|r| r == crate::artist::ARTIST_ROLE));
    assert!(!crate::presets::preset_roles()
        .unwrap()
        .iter()
        .any(|r| r.name == crate::artist::ARTIST_ROLE));
    let cards = crate::cards::queued(&wb.db, "p1").unwrap();
    assert!(
        cards.is_empty(),
        "提示不是待决卡: {:?}",
        cards
            .iter()
            .map(|c| (&c.kind, &c.payload))
            .collect::<Vec<_>>()
    );
}

#[test]
fn image_slot_bound_to_a_chat_model_reports_unsupported_with_that_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["项目经理", "后端"], None).unwrap();
    let _doc = crate::provider_config::fixture_document(doc_with(&[
        ("default", "gpt-image-1"),
        ("image", "gpt-5"),
    ]));
    summon(
        &mut wb,
        "a0",
        json!({"style": "watercolor", "roles": ["后端"]}),
    );
    let notices = missing_events(&wb);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["state"], "unsupported");
    assert_eq!(notices[0]["model"], "gpt-5");
}

#[test]
fn only_the_project_manager_can_summon_the_artist() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["项目经理", "后端"], None).unwrap();
    let _doc = crate::provider_config::fixture_document(doc_with(&[]));
    summon(&mut wb, "a1", json!({"style": "pixel art"}));
    assert!(missing_events(&wb).is_empty());
    assert!(tool_result_error(&wb));
}

#[test]
fn target_roles_outside_the_team_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbench::for_test(dir.path(), &["项目经理", "后端"], None).unwrap();
    let _doc = crate::provider_config::fixture_document(doc_with(&[]));
    summon(
        &mut wb,
        "a0",
        json!({"style": "pixel art", "roles": [crate::artist::ARTIST_ROLE]}),
    );
    assert!(missing_events(&wb).is_empty(), "画师不能把自己当目标");
    assert!(tool_result_error(&wb));
}
