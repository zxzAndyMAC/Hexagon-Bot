//! 项目向导（票 24）：目录检查 → 角色/包选择 → 说明文件 → 密钥 → 建项目。
//!
//! - 三态目录：`inspect_dir` 报 git/脏/空/说明文件；脏树先停，无 git 须确认初始化。
//! - 说明文件主读 `AGENTS.md`，没有则 `CLAUDE.md`；草稿人确认才写，不覆盖已有。
//! - 密钥 fail-closed：所选角色的模型槽缺 key 不能开跑（`create_project` 内置复查）。
//! - 建项目把 RoleDef 落成实例：model_slot、agent_globs 归属、grants 技能授权。

use crate::api::Workbench;
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::git;
use crate::orchestra::PackDef;
use crate::presets::{preset_roles, PresetError, RoleDef};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error(transparent)]
    Api(#[from] crate::api::ApiError),
    #[error(transparent)]
    Preset(#[from] PresetError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Cred(#[from] crate::credentials::CredError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("git: {0}")]
    Git(#[from] git::GitError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("目录不是 git 仓库，需确认初始化: {0}")]
    NoGit(PathBuf),
    #[error("仓库有未提交改动，先清理再开项目: {0}")]
    DirtyTree(PathBuf),
    #[error("未知预置角色: {0}")]
    UnknownRole(String),
    #[error("角色定制传了未勾选的角色: {0}")]
    StrayOverride(String),
    #[error("角色定制不能自审: {0}")]
    SelfReviewer(String),
    #[error("角色定制「{role}」的上级「{reviewer}」不在模板目录内")]
    UnknownOverrideReviewer { role: String, reviewer: String },
    #[error("缺模型密钥，补齐或拿掉对应角色才能开跑: {}", .0.join(", "))]
    MissingKeys(Vec<String>),
    #[error("说明文件已存在，不覆盖: {0}")]
    AgentsMdExists(PathBuf),
    #[error("快速通道须指定一个已勾选角色")]
    NoFastRole,
}

/// 目录体检报告：向导每一步的判定依据。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct DirReport {
    pub exists: bool,
    /// 目录存在且没有条目（.hexagon 等隐藏项也算条目）
    pub empty: bool,
    pub is_git: bool,
    pub dirty: bool,
    /// 主说明文件：AGENTS.md 优先，其次 CLAUDE.md；都没有 = None
    pub instructions: Option<String>,
}

pub fn inspect_dir(dir: impl AsRef<Path>) -> DirReport {
    let dir = dir.as_ref();
    let exists = dir.is_dir();
    let empty = exists
        && std::fs::read_dir(dir)
            .map(|mut it| it.next().is_none())
            .unwrap_or(true);
    let is_git = exists && git::is_repo(dir);
    let instructions = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .find(|f| dir.join(f).is_file())
        .map(|f| f.to_string());
    DirReport {
        exists,
        empty,
        is_git,
        dirty: is_git && git::is_dirty(dir),
        instructions,
    }
}

/// AGENTS.md 草稿（模板）。模型起草要等运行期供应商接线——向导阶段还没有
/// 可用 provider；先给结构骨架，人确认才写盘，绝不覆盖已有文件。
pub fn agents_md_draft(project_name: &str) -> String {
    format!(
        "# {project_name}\n\n## Commands\n\n- Build:\n- Test:\n- Check:\n\n## Layout\n\n-\n\n## Conventions\n\n-\n"
    )
}

/// 写 AGENTS.md：文件已存在即报错（内容相同也报——不静默跳过，让向导显式处理）。
pub fn write_agents_md(dir: impl AsRef<Path>, content: &str) -> Result<(), SetupError> {
    let p = dir.as_ref().join("AGENTS.md");
    if p.exists() {
        return Err(SetupError::AgentsMdExists(p));
    }
    std::fs::write(&p, content)?;
    Ok(())
}

/// 所选角色的模型槽里哪些不就绪（返回槽位名，去重排序）。
/// 就绪 = 槽位有绑定 + 供应商存在且启用 + key 已存（default 槽兜底）——
/// 语义已从「缺 key」升级为「缺可用供应商」，见 provider_config.rs。
pub fn missing_model_keys(
    store: &dyn CredentialStore,
    roles: &[RoleDef],
    doc: &crate::provider_config::ProviderDoc,
) -> Result<Vec<String>, SetupError> {
    let mut missing = Vec::new();
    for slot in {
        let mut s: Vec<&str> = roles.iter().map(|r| r.model_slot.as_str()).collect();
        s.sort();
        s.dedup();
        s
    } {
        if !crate::provider_config::slot_ready(doc, store, slot) {
            missing.push(slot.to_string());
        }
    }
    Ok(missing)
}

/// 建项目（向导最后一步）。`roles` 是模板目录（内置∪自定义）角色名子集；
/// `role_overrides` 携带**完整定义**——自定义模板与向导改过的角色都经它传入，
/// 本函数不读全局模板文件（保持纯函数，测试不被 HOME 污染）。`pack` 为 None 时
/// `fastpath_role` 必须给（快速通道）；`init_git` = 负责人确认了「无 git 则初始化」。
/// 全程 fail-closed：脏树停、缺密钥停、未知角色停、野 override 停。
#[allow(clippy::too_many_arguments)]
pub fn create_project(
    dir: impl AsRef<Path>,
    name: &str,
    roles: &[String],
    role_overrides: &[RoleDef],
    pack: Option<&PackDef>,
    fastpath_role: Option<&str>,
    init_git: bool,
    store: &dyn CredentialStore,
    doc: &crate::provider_config::ProviderDoc,
) -> Result<Workbench, SetupError> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir)?;

    // git 闸：非仓库须确认初始化；是仓库则脏树先停
    if git::is_repo(dir) {
        if git::is_dirty(dir) {
            return Err(SetupError::DirtyTree(dir.to_path_buf()));
        }
    } else if init_git {
        git::init(dir, "main")?;
    } else {
        return Err(SetupError::NoGit(dir.to_path_buf()));
    }

    // 解析角色：override 优先（自定义模板/向导定制），回落内置（未知角色拒绝）。
    // override 校验只做定义级（名非空/不自审/上级在目录内）——技能名不验，
    // 理由同 templates.rs 头注；上级目录级校验同内置预置，不要求已勾选。
    let presets = preset_roles()?;
    let known: std::collections::HashSet<&str> = presets
        .iter()
        .chain(role_overrides.iter())
        .map(|d| d.name.as_str())
        .collect();
    for o in role_overrides {
        if !roles.contains(&o.name) {
            return Err(SetupError::StrayOverride(o.name.clone()));
        }
        if o.name.trim().is_empty() {
            return Err(SetupError::UnknownRole(o.name.clone()));
        }
        if o.reviewer.as_deref() == Some(o.name.as_str()) {
            return Err(SetupError::SelfReviewer(o.name.clone()));
        }
        if let Some(rev) = &o.reviewer {
            if !known.contains(rev.as_str()) {
                return Err(SetupError::UnknownOverrideReviewer {
                    role: o.name.clone(),
                    reviewer: rev.clone(),
                });
            }
        }
    }
    let mut picked: Vec<RoleDef> = Vec::new();
    for r in roles {
        if let Some(o) = role_overrides.iter().find(|o| &o.name == r) {
            picked.push(o.clone());
        } else {
            picked.push(
                presets
                    .iter()
                    .find(|p| &p.name == r)
                    .ok_or_else(|| SetupError::UnknownRole(r.clone()))?
                    .clone(),
            );
        }
    }
    if pack.is_none() {
        match fastpath_role {
            Some(fr) if picked.iter().any(|p| p.name == fr) => {}
            _ => return Err(SetupError::NoFastRole),
        }
    }
    let missing = missing_model_keys(store, &picked, doc)?;
    if !missing.is_empty() {
        return Err(SetupError::MissingKeys(missing));
    }

    // 开项目 + 实例化角色（model_slot / globs / 技能授权）
    let pairs: Vec<(String, String)> = picked
        .iter()
        .enumerate()
        .map(|(i, r)| (format!("a{i}"), r.name.clone()))
        .collect();
    let wb = Workbench::open(dir, name, &pairs, pack.cloned())?;
    let db = Db::open(dir.join(".hexagon/state.db"))?;

    let mode = if pack.is_some() { "pack" } else { "fastpath" };
    let fast_agent = fastpath_role.map(|fr| {
        pairs
            .iter()
            .find(|(_, r)| r == fr)
            .map(|(a, _)| a.clone())
            .unwrap_or_default()
    });
    db.conn().execute(
        "UPDATE projects SET mode=?1, pack_name=?2, pack_copy_version=?3, fastpath_agent_id=?4 WHERE id='p1'",
        rusqlite::params![
            mode,
            pack.map(|p| p.name.as_str()),
            pack.map(|p| p.version as i64),
            fast_agent,
        ],
    )?;

    for ((aid, _), def) in pairs.iter().zip(picked.iter()) {
        db.conn().execute(
            "UPDATE agents SET model_slot=?1 WHERE id=?2",
            rusqlite::params![def.model_slot, aid],
        )?;
        for g in &def.globs {
            db.conn().execute(
                "INSERT OR IGNORE INTO agent_globs (agent_id, glob) VALUES (?1, ?2)",
                rusqlite::params![aid, g],
            )?;
        }
        for s in &def.skills {
            db.conn().execute(
                "INSERT OR IGNORE INTO grants (id, agent_id, kind, name)
                 VALUES (?1, ?2, 'skill', ?3)",
                rusqlite::params![format!("g-{aid}-{s}"), aid, s],
            )?;
        }
    }
    Ok(wb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use serde_json::json;

    fn store_with_key() -> MemoryStore {
        let s = MemoryStore::default();
        s.set("provider/test-prov", "sk-test").unwrap();
        s
    }

    /// 测试用供应商档：test-prov + chat 槽绑定（新就绪语义=绑定+启用+key）。
    fn doc_with_provider() -> crate::provider_config::ProviderDoc {
        let mut doc = crate::provider_config::ProviderDoc::default();
        doc.providers.push(crate::provider_config::ProviderDef {
            id: "test-prov".into(),
            name: "Test".into(),
            kind: crate::provider::ProviderKind::OpenAi,
            base_url: "http://localhost".into(),
            models: vec![],
            enabled: true,
        });
        doc.slots.insert(
            "chat".into(),
            crate::provider_config::SlotBinding {
                provider_id: "test-prov".into(),
                model: "m".into(),
            },
        );
        doc
    }

    fn pack() -> PackDef {
        serde_json::from_value(json!({
            "name":"规格驱动","version":1,
            "stages":[{"name":"规格","roles":["产品策划"],"due":["规格"],"stamp_point":true}]
        }))
        .unwrap()
    }

    #[test]
    fn inspect_empty_dir() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("new-proj");
        let r = inspect_dir(&sub);
        assert!(!r.exists);
        std::fs::create_dir(&sub).unwrap();
        let r = inspect_dir(&sub);
        assert!(r.exists && r.empty && !r.is_git && r.instructions.is_none());
    }

    #[test]
    fn create_empty_dir_with_git_init() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let wb = create_project(
            &sub,
            "测试项目",
            &["产品策划".into(), "后端".into()],
            &[],
            Some(&pack()),
            None,
            true, // 确认初始化
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert!(sub.join(".hexagon/pack.active.json").exists());
        let team = crate::orchestra::team(&wb.db, &wb.project_id, &wb.repo_root).unwrap();
        assert_eq!(team.len(), 2);
        // model_slot 和 globs 落库
        let db = Db::open(sub.join(".hexagon/state.db")).unwrap();
        let slot: String = db
            .conn()
            .query_row("SELECT model_slot FROM agents WHERE id='a0'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(slot, "chat");
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM agent_globs WHERE agent_id='a0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n > 0);
        let skills: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM grants WHERE agent_id='a0' AND kind='skill'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(skills > 0);
    }

    #[test]
    fn existing_clean_repo_ok_dirty_stops() {
        let d = tempfile::tempdir().unwrap();
        git::init(d.path(), "main").unwrap();
        // init 后提交基线使树干净
        std::fs::write(d.path().join("README.md"), "x").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        create_project(
            d.path(),
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        // 弄脏 → 拒绝
        std::fs::write(d.path().join("README.md"), "dirty").unwrap();
        assert!(matches!(
            create_project(
                d.path(),
                "仓",
                &["产品策划".into()],
                &[],
                Some(&pack()),
                None,
                false,
                &store_with_key(),
                &doc_with_provider()
            ),
            Err(SetupError::DirtyTree(_))
        ));
    }

    #[test]
    fn no_git_requires_confirm() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x.txt"), "x").unwrap(); // 非空非仓库
        assert!(matches!(
            create_project(
                d.path(),
                "p",
                &["产品策划".into()],
                &[],
                Some(&pack()),
                None,
                false,
                &store_with_key(),
                &doc_with_provider()
            ),
            Err(SetupError::NoGit(_))
        ));
    }

    #[test]
    fn missing_key_blocks_start() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let err = match create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &MemoryStore::default(), // 没有任何 key
            &crate::provider_config::ProviderDoc::default(),
        ) {
            Ok(_) => panic!("missing key should block"),
            Err(e) => e,
        };
        match err {
            SetupError::MissingKeys(keys) => assert_eq!(keys, vec!["chat"]),
            e => panic!("expected MissingKeys, got {e}"),
        }
        // 项目没建起来
        assert!(!sub.join(".hexagon/state.db").exists());
    }

    #[test]
    fn fastpath_requires_checked_role() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        assert!(matches!(
            create_project(
                &sub,
                "p",
                &["产品策划".into()],
                &[],
                None,
                Some("后端"), // 未勾选
                true,
                &store_with_key(),
                &doc_with_provider()
            ),
            Err(SetupError::NoFastRole)
        ));
        // 勾选后可走快速通道
        create_project(
            d.path().join("q"),
            "p",
            &["后端".into()],
            &[],
            None,
            Some("后端"),
            true,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        let db = Db::open(d.path().join("q/.hexagon/state.db")).unwrap();
        let mode: String = db
            .conn()
            .query_row("SELECT mode FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "fastpath");
    }

    /// override：自定义模板名（不在内置目录）能选中并物化；定制字段落库；
    /// 野 override（未勾选角色）与自审/幽灵上级拒绝。
    #[test]
    fn role_overrides_materialize_and_validate() {
        let d = tempfile::tempdir().unwrap();
        let custom = RoleDef {
            name: "自建角色".into(),
            duty: "定制职责".into(),
            reviewer: None,
            model_slot: "chat".into(),
            globs: vec!["src/x/**".into()],
            skills: vec![],
        };
        // 自定义名不在内置目录 → 无 override 时 UnknownRole；有 override 时建得起来
        let sub = d.path().join("p");
        assert!(matches!(
            create_project(
                &sub,
                "p",
                &["自建角色".into()],
                &[],
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider()
            ),
            Err(SetupError::UnknownRole(_))
        ));
        let wb = create_project(
            &sub,
            "p",
            &["自建角色".into()],
            std::slice::from_ref(&custom),
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        let db = Db::open(sub.join(".hexagon/state.db")).unwrap();
        let role: String = db
            .conn()
            .query_row("SELECT role FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(role, "自建角色");
        let glob: String = db
            .conn()
            .query_row(
                "SELECT glob FROM agent_globs WHERE agent_id='a0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(glob, "src/x/**");
        drop(wb);
        // 野 override / 自审 / 幽灵上级
        for (overrides, want) in [
            (vec![custom.clone()], "StrayOverride"),
            (
                vec![RoleDef {
                    reviewer: Some("自建角色".into()),
                    ..custom.clone()
                }],
                "SelfReviewer",
            ),
            (
                vec![RoleDef {
                    reviewer: Some("幽灵".into()),
                    ..custom.clone()
                }],
                "UnknownOverrideReviewer",
            ),
        ] {
            let sub = d.path().join(format!("{:?}", want));
            let roles = if want == "StrayOverride" {
                vec!["产品策划".into()]
            } else {
                vec!["自建角色".into()]
            };
            let e = match create_project(
                &sub,
                "p",
                &roles,
                &overrides,
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider(),
            ) {
                Ok(_) => panic!("{want} should fail"),
                Err(e) => e,
            };
            assert_eq!(
                crate::errcode::variant_code(&e),
                match want {
                    "StrayOverride" => "stray_override",
                    "SelfReviewer" => "self_reviewer",
                    _ => "unknown_override_reviewer",
                }
            );
        }
    }

    #[test]
    fn agents_md_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        write_agents_md(d.path(), &agents_md_draft("x")).unwrap();
        assert!(matches!(
            write_agents_md(d.path(), "new"),
            Err(SetupError::AgentsMdExists(_))
        ));
        assert_eq!(
            inspect_dir(d.path()).instructions.as_deref(),
            Some("AGENTS.md")
        );
    }
}
