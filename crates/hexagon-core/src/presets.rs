//! 预置内容包：随包数据文件（`presets/`）+ 载入校验。
//!
//! - 十一预置角色 `presets/roles.json`：职责、上级链、默认模型槽、默认路径归属、默认技能。
//! - 四预置流程包 `presets/packs/*.json`：PackDef 数据；项目内改的是钉住的副本
//!   （`PackDef::pin` 写 `.hexagon/pack.active.json`），源文件只读。
//! - 预置技能 `presets/skills/<name>/SKILL.md`：Agent Skills 格式，角色按名引用。
//!
//! 校验两层：JSON 形状由 serde 管；语义由 `validate_*` 管——角色/技能引用必须
//! 落在预置名单内、阶段名唯一、至少一个盖章点。坏包报出具体位置。

use crate::orchestra::PackDef;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("preset {file}: bad json: {src}")]
    Json {
        file: &'static str,
        src: serde_json::Error,
    },
    #[error("角色定义重复: {0}")]
    DuplicateRole(String),
    #[error("角色「{role}」的上级「{reviewer}」不在预置角色名单内")]
    UnknownReviewer { role: String, reviewer: String },
    #[error("角色「{role}」引用了不存在的预置技能「{skill}」")]
    UnknownSkill { role: String, skill: String },
    #[error("流程包「{pack}」没有阶段")]
    EmptyStages { pack: String },
    #[error("流程包「{pack}」阶段名重复: {stage}")]
    DuplicateStage { pack: String, stage: String },
    #[error("流程包「{pack}」{field} 引用了不存在的角色「{role}」")]
    UnknownRole {
        pack: String,
        field: &'static str,
        role: String,
    },
    #[error("流程包「{pack}」至少需要一个盖章点")]
    NoStampPoint { pack: String },
}

/// 预置角色定义。`reviewer` = 上级链（复审路由的默认上级），空 = 直达负责人。
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RoleDef {
    pub name: String,
    /// 一句话职责（进激活简报与团队说明）
    pub duty: String,
    #[serde(default)]
    pub reviewer: Option<String>,
    pub model_slot: String,
    /// 默认路径所有权 glob（项目实例可覆盖）
    #[serde(default)]
    pub globs: Vec<String>,
    /// 默认技能名（presets/skills/<name>/，不自动授权）
    #[serde(default)]
    pub skills: Vec<String>,
}

const ROLES_JSON: &str = include_str!("../presets/roles.json");

const PACK_FILES: &[(&str, &str)] = &[
    (
        "spec-driven",
        include_str!("../presets/packs/spec-driven.json"),
    ),
    (
        "stage-gate",
        include_str!("../presets/packs/stage-gate.json"),
    ),
    (
        "short-iter",
        include_str!("../presets/packs/short-iter.json"),
    ),
    ("kanban", include_str!("../presets/packs/kanban.json")),
];

/// (name, SKILL.md 全文)。pub(crate)：skills.rs 的 SkillLoader 把内置技能
/// 当最底层种子（global-config 票 03——此前只在二进制里睡着，load_skill 404）。
pub(crate) const SKILL_FILES: &[(&str, &str)] = &[
    (
        "spec-writing",
        include_str!("../presets/skills/spec-writing/SKILL.md"),
    ),
    (
        "ia-outline",
        include_str!("../presets/skills/ia-outline/SKILL.md"),
    ),
    (
        "ui-annotation",
        include_str!("../presets/skills/ui-annotation/SKILL.md"),
    ),
    (
        "api-contract",
        include_str!("../presets/skills/api-contract/SKILL.md"),
    ),
    (
        "test-plan",
        include_str!("../presets/skills/test-plan/SKILL.md"),
    ),
    (
        "code-review",
        include_str!("../presets/skills/code-review/SKILL.md"),
    ),
    (
        "adr-writing",
        include_str!("../presets/skills/adr-writing/SKILL.md"),
    ),
    (
        "runbook",
        include_str!("../presets/skills/runbook/SKILL.md"),
    ),
    (
        "handoff-note",
        include_str!("../presets/skills/handoff-note/SKILL.md"),
    ),
    (
        "policy-tuning",
        include_str!("../presets/skills/policy-tuning/SKILL.md"),
    ),
];

/// 预置技能名 → SKILL.md 正文（激活时先给名称摘要，用到再读正文）。
pub fn skill_doc(name: &str) -> Option<&'static str> {
    SKILL_FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
}

pub fn preset_skill_names() -> Vec<&'static str> {
    SKILL_FILES.iter().map(|(n, _)| *n).collect()
}

/// 十一预置角色（载入即校验）。
pub fn preset_roles() -> Result<Vec<RoleDef>, PresetError> {
    let roles: Vec<RoleDef> =
        serde_json::from_str(ROLES_JSON).map_err(|src| PresetError::Json {
            file: "roles.json",
            src,
        })?;
    validate_roles(&roles)?;
    Ok(roles)
}

/// 四预置流程包（载入即校验，角色名单以预置角色为准）。
pub fn preset_packs() -> Result<Vec<PackDef>, PresetError> {
    let known: Vec<String> = preset_roles()?.iter().map(|r| r.name.clone()).collect();
    PACK_FILES
        .iter()
        .map(|(slug, src)| {
            let pack: PackDef = serde_json::from_str(src).map_err(|src| PresetError::Json {
                file: match *slug {
                    "spec-driven" => "packs/spec-driven.json",
                    "stage-gate" => "packs/stage-gate.json",
                    "short-iter" => "packs/short-iter.json",
                    _ => "packs/kanban.json",
                },
                src,
            })?;
            validate_pack(&pack, &known)?;
            Ok(pack)
        })
        .collect()
}

/// 角色名单语义校验：名唯一、上级引用有效、技能引用有效。
pub fn validate_roles(roles: &[RoleDef]) -> Result<(), PresetError> {
    let names: Vec<&str> = roles.iter().map(|r| r.name.as_str()).collect();
    let skills = preset_skill_names();
    for r in roles {
        if names.iter().filter(|n| **n == r.name).count() > 1 {
            return Err(PresetError::DuplicateRole(r.name.clone()));
        }
        if let Some(rev) = &r.reviewer {
            if !names.contains(&rev.as_str()) {
                return Err(PresetError::UnknownReviewer {
                    role: r.name.clone(),
                    reviewer: rev.clone(),
                });
            }
        }
        for s in &r.skills {
            if !skills.contains(&s.as_str()) {
                return Err(PresetError::UnknownSkill {
                    role: r.name.clone(),
                    skill: s.clone(),
                });
            }
        }
    }
    Ok(())
}

/// 流程包语义校验：阶段非空且名唯一、复审者/回填边/会诊名单的角色引用有效、
/// 至少一个盖章点（没有盖章点的包等于放掉了负责人闸口）。
/// `known_roles` = 允许引用的角色全集（预置角色名单）。
pub fn validate_pack(pack: &PackDef, known_roles: &[String]) -> Result<(), PresetError> {
    if pack.stages.is_empty() {
        return Err(PresetError::EmptyStages {
            pack: pack.name.clone(),
        });
    }
    for (i, s) in pack.stages.iter().enumerate() {
        if pack.stages[..i].iter().any(|x| x.name == s.name) {
            return Err(PresetError::DuplicateStage {
                pack: pack.name.clone(),
                stage: s.name.clone(),
            });
        }
        let check = |field: &'static str, role: &str| -> Result<(), PresetError> {
            if known_roles.iter().any(|k| k == role) {
                Ok(())
            } else {
                Err(PresetError::UnknownRole {
                    pack: pack.name.clone(),
                    field,
                    role: role.to_string(),
                })
            }
        };
        for r in &s.roles {
            check("roles", r)?;
        }
        for rv in &s.reviews {
            check("reviews.reviewer", &rv.reviewer)?;
        }
        for (a, b) in &s.backfill_edges {
            check("backfill_edges", a)?;
            check("backfill_edges", b)?;
        }
        for r in &s.consult_wake {
            check("consult_wake", r)?;
        }
    }
    if !pack.stages.iter().any(|s| s.stamp_point) {
        return Err(PresetError::NoStampPoint {
            pack: pack.name.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::orchestra;
    use crate::provider::ScriptedProvider;
    use crate::tools::{Registry, ToolContext};
    use crate::trace::EventKind;
    use crate::turn::{run_turn, text_response, tool_response, TurnOutcome};
    use serde_json::json;

    #[test]
    fn preset_roles_load_and_validate() {
        let roles = preset_roles().unwrap();
        assert_eq!(roles.len(), 11);
        assert!(roles.iter().all(|r| !r.duty.is_empty()));
        assert!(roles.iter().all(|r| !r.model_slot.is_empty()));
        // CONTEXT.md 钉的四条上级链
        let rev = |name: &str| {
            roles
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .reviewer
                .clone()
        };
        assert_eq!(rev("UI").as_deref(), Some("UX"));
        assert_eq!(rev("前端").as_deref(), Some("前端技术负责人"));
        assert_eq!(rev("后端").as_deref(), Some("后端技术负责人"));
        assert_eq!(rev("运维").as_deref(), Some("后端技术负责人"));
        // 每个角色至少 1 个默认技能，且正文可取
        for r in &roles {
            assert!(!r.skills.is_empty(), "{} has no skills", r.name);
            for s in &r.skills {
                assert!(skill_doc(s).unwrap().contains("name:"), "bad skill {s}");
            }
        }
    }

    #[test]
    fn preset_packs_load_and_validate() {
        let packs = preset_packs().unwrap();
        assert_eq!(packs.len(), 4);
        let names: Vec<_> = packs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["规格驱动", "阶段门", "短迭代", "看板流"]);
        for p in &packs {
            assert!(
                p.stages.iter().any(|s| s.stamp_point),
                "{} no stamp",
                p.name
            );
        }
    }

    #[test]
    fn validate_pack_catches_bad_refs() {
        let known = vec!["产品策划".to_string()];
        let bad_reviewer: PackDef = serde_json::from_value(json!({
        "name":"x","version":1,"stages":[
            {"name":"a","roles":["产品策划"],"due":[],
             "reviews":[{"artifact_kind":"k","reviewer":"幽灵"}],"stamp_point":true}
        ]}))
        .unwrap();
        assert!(matches!(
            validate_pack(&bad_reviewer, &known),
            Err(PresetError::UnknownRole {
                field: "reviews.reviewer",
                ..
            })
        ));
        let bad_edge: PackDef = serde_json::from_value(json!({
        "name":"x","version":1,"stages":[
            {"name":"a","roles":["产品策划"],"due":[],"stamp_point":true,
             "backfill_edges":[["产品策划","幽灵"]]}
        ]}))
        .unwrap();
        assert!(matches!(
            validate_pack(&bad_edge, &known),
            Err(PresetError::UnknownRole {
                field: "backfill_edges",
                ..
            })
        ));
        let no_stamp: PackDef = serde_json::from_value(json!({
            "name":"x","version":1,"stages":[{"name":"a","roles":["产品策划"],"due":[]}]
        }))
        .unwrap();
        assert!(matches!(
            validate_pack(&no_stamp, &known),
            Err(PresetError::NoStampPoint { .. })
        ));
        let dup: PackDef = serde_json::from_value(json!({
        "name":"x","version":1,"stages":[
            {"name":"a","roles":["产品策划"],"due":[],"stamp_point":true},
            {"name":"a","roles":["产品策划"],"due":[]}
        ]}))
        .unwrap();
        assert!(matches!(
            validate_pack(&dup, &known),
            Err(PresetError::DuplicateStage { .. })
        ));
    }

    // ---------- 四套预置包端到端走通（假供应商） ----------

    fn setup_all_roles() -> (Db, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                // 票 02：省略 autonomy 默认 L4，非最终盖章点会自动通过，打乱「每阶段等人盖章」走查。
                // 走查钉 L0。L3 自动通过由 Workbench 门面测试钉。
                "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES ('p1','/tmp/x','x','pack','L0')",
                [],
            )
            .unwrap();
        for (i, r) in preset_roles().unwrap().iter().enumerate() {
            db.conn()
                .execute(
                    "INSERT INTO agents (id, project_id, role) VALUES (?1,'p1',?2)",
                    rusqlite::params![format!("a{i}"), r.name],
                )
                .unwrap();
        }
        (db, tempfile::tempdir().unwrap())
    }

    fn agent_id(db: &Db, role: &str) -> String {
        db.conn()
            .query_row(
                "SELECT id FROM agents WHERE project_id='p1' AND role=?1",
                [role],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn active_run(db: &Db) -> (String, String) {
        db.conn()
            .query_row(
                "SELECT id, stage_name FROM stage_runs WHERE project_id='p1'
                 AND state IN ('active','waiting_stamp') ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// 每个预置包：全角色团队在 ScriptedProvider 下走完所有阶段到 pack_finished。
    /// 脚本按阶段生成——每个激活角色交付其轮值的一份 due 产物（kind 均分），
    /// 声明复审直接补 review_passed 事件（复审本身在票 10 已测）。
    #[test]
    fn all_preset_packs_run_to_finish() {
        let reg = Registry::builtin();
        for pack in preset_packs().unwrap() {
            let (db, dir) = setup_all_roles();
            pack.pin(dir.path()).unwrap();

            // 预生成脚本：阶段×角色 → artifact_write(due 轮值) + text
            let mut resps = Vec::new();
            let mut script_for = Vec::new(); // 每阶段 Vec<(role, Option<kind>)>
            for st in &pack.stages {
                let mut per_role = Vec::new();
                for (i, role) in st.roles.iter().enumerate() {
                    let kind = if st.due.is_empty() {
                        None
                    } else {
                        Some(st.due[i % st.due.len()].clone())
                    };
                    per_role.push((role.clone(), kind.clone()));
                    if let Some(k) = kind {
                        // skeleton 档 kind 有必备节（artifacts::required_sections）
                        let sections = match k.as_str() {
                            "规格" => "## 目标\nx\n## 范围\nx\n## 验收\nx\n",
                            "接口说明" => "## 资源\nx\n## 端点\nx\n## 错误码\nx\n",
                            "技术裁定记录" => "## 背景\nx\n## 决定\nx\n## 后果\nx\n",
                            _ => "## 概述\nx\n",
                        };
                        let content = format!("---\nkind: {k}\nauthor: {role}\n---\n{sections}");
                        resps.push(tool_response(vec![(
                            "c",
                            "artifact_write",
                            json!({"path": format!("docs/{k}.md"), "content": content}),
                        )]));
                    }
                    resps.push(text_response(&format!("{role} 交付完毕")));
                }
                script_for.push(per_role);
            }
            let provider = ScriptedProvider::new(resps);

            orchestra::open_stage(&db, "p1", &pack, 0).unwrap();
            let mut finished = false;
            for (seq, st) in pack.stages.iter().enumerate() {
                let (rid, sname) = active_run(&db);
                assert_eq!(sname, st.name, "{} stage {seq}", pack.name);
                // 激活角色各跑一回合（吃预生成脚本）
                for (role, _) in &script_for[seq] {
                    let aid = agent_id(&db, role);
                    let ctx = ToolContext {
                        project_id: "p1".into(),
                        agent_id: aid,
                        repo_root: dir.path().to_path_buf(),
                        stage_run_id: Some(rid.clone()),
                        owned_globs: vec![],
                        tiers: crate::artifacts::TierMap::new(),
                        sessions: Default::default(),
                        caps: Default::default(),
                    };
                    assert_eq!(
                        run_turn(&db, &provider, &reg, &ctx, vec![], "干活").unwrap(),
                        TurnOutcome::Finished,
                        "{} {} {}",
                        pack.name,
                        st.name,
                        role
                    );
                }
                // 声明复审补齐通过事件
                for rv in &st.reviews {
                    db.append_event(
                        "p1",
                        EventKind::ReviewPassed,
                        json!({"artifact_kind": rv.artifact_kind, "reviewer": rv.reviewer}),
                        Some(&agent_id(&db, &rv.reviewer)),
                        Some(&rid),
                    )
                    .unwrap();
                }
                // 推进：可能是下一阶段直接开，也可能停在盖章点
                let a =
                    serde_json::to_value(orchestra::advance(&db, "p1", &pack).unwrap()).unwrap();
                match a["action"].as_str().unwrap() {
                    "awaiting_stamp" => {
                        let s = serde_json::to_value(orchestra::stamp(&db, "p1", &pack).unwrap())
                            .unwrap();
                        finished = s["action"] == "pack_finished";
                    }
                    "pack_finished" => finished = true,
                    "stage_opened" => {}
                    other => panic!("{} stage {}: advance={other}", pack.name, st.name),
                }
            }
            assert!(finished, "{} did not reach pack_finished", pack.name);
        }
    }
}
