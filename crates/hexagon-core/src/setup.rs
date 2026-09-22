//! 项目向导（票 24，脏树/重开见 ADR 0060）：目录检查 → 角色/包选择 → 说明文件 → 密钥 → 建项目。
//!
//! - 目录：`inspect_dir` 报 git/脏/空/已有工作台/说明文件。脏树可开（不提交、不清理）；
//!   无 git 须确认初始化；已有 `.hexagon/state.db` 拒绝再建，走 `open_existing`。
//! - 说明文件主读 `AGENTS.md`，没有则 `CLAUDE.md`；草稿人确认才写，不覆盖已有。
//! - 密钥 fail-closed：所选角色的模型槽缺 key 不能开跑（`create_project` 内置复查）。
//! - 建项目把 RoleDef 落成实例：model_slot、agent_globs 归属、grants 技能授权。
//! - 票 14：最后一步按 `CREATE_STEP_ORDER` 逐步报告。每步做完才回调，失败不回调
//!   该步及之后的步，调用方因此不能把「打开项目」当成已成功。不用定时器假装进度。

use crate::api::Workbench;
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::git;
use crate::orchestra::PackDef;
use crate::presets::{preset_roles, PresetError, RoleDef};
use serde::{Deserialize, Serialize};
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
    #[error("目录已有工作台状态，请打开而不是新建: {0}")]
    AlreadyProject(PathBuf),
    #[error("目录没有工作台状态，不能按已有项目打开: {0}")]
    NotAProject(PathBuf),
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
    /// 已有工作台状态（`.hexagon/state.db`）。向导走打开，`create_project` 拒绝再建。
    pub has_workbench: bool,
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
    let has_workbench = exists && dir.join(".hexagon/state.db").is_file();
    let instructions = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .find(|f| dir.join(f).is_file())
        .map(|f| f.to_string());
    DirReport {
        exists,
        empty,
        is_git,
        dirty: is_git && git::is_dirty(dir),
        has_workbench,
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

/// 向导最后一步的可见进度（票 14）。顺序即 `CREATE_STEP_ORDER`：
/// 检查目录 → git → 角色落库 → 密钥复查 → 打开项目。
/// 只在该步成功结束后报告；失败的那一步以及后面的步都不报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum CreateStep {
    /// 目录已就绪（建得出来、是目录）。
    CheckDir,
    /// git 闸已过：已是干净仓库，或负责人确认后完成初始化。
    Git,
    /// 角色定义已写入项目库（model_slot / globs / grants）。
    PersistRoles,
    /// 所选角色的模型槽复查通过。
    RecheckKeys,
    /// 工作台实例已建好，可以交给壳层打开。
    OpenProject,
}

/// 创建进度的固定顺序。界面清单与测试都钉这一份，不另排。
pub const CREATE_STEP_ORDER: [CreateStep; 5] = [
    CreateStep::CheckDir,
    CreateStep::Git,
    CreateStep::PersistRoles,
    CreateStep::RecheckKeys,
    CreateStep::OpenProject,
];

/// 建项目（向导最后一步）。`roles` 是模板目录（内置∪自定义）角色名子集；
/// `role_overrides` 携带**完整定义**——自定义模板与向导改过的角色都经它传入，
/// 本函数不读全局模板文件（保持纯函数，测试不被 HOME 污染）。`pack` 为 None 时
/// `fastpath_role` 必须给（快速通道）；`init_git` = 负责人确认了「无 git 则初始化」。
/// 全程 fail-closed：已有工作台停（再建会改写花名册）、缺密钥停、未知角色停、野 override 停。
/// 脏树不停——ADR 0060：未提交改动留给负责人，开项目不提交、不清理。
/// 进度走 `create_project_reporting`；本函数不报告（脚本/测试旧入口）。
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
    create_project_reporting(
        dir,
        name,
        roles,
        role_overrides,
        pack,
        fastpath_role,
        init_git,
        store,
        doc,
        |_| {},
    )
}

/// 同 `create_project`，但每一步**做完**就调用 `on_step`。
/// 壳层把回调推进 IPC channel，界面才不会等到整段结束才有反应（票 14）。
/// 被否决的替代：界面用定时器按清单打勾——那和真实步骤脱节，失败也会假装往后走。
#[allow(clippy::too_many_arguments)]
pub fn create_project_reporting<F>(
    dir: impl AsRef<Path>,
    name: &str,
    roles: &[String],
    role_overrides: &[RoleDef],
    pack: Option<&PackDef>,
    fastpath_role: Option<&str>,
    init_git: bool,
    store: &dyn CredentialStore,
    doc: &crate::provider_config::ProviderDoc,
    mut on_step: F,
) -> Result<Workbench, SetupError>
where
    F: FnMut(CreateStep),
{
    let dir = dir.as_ref();
    // ADR 0060：已有工作台状态是打开，不是再建。先于 mkdir / git init，
    // 避免二次创建改 mode、槽位、授权。被否决的替代：静默当成 open 并套用
    // 本次向导选项（会改写已有项目）。false negative（漏判已有库）会重跑落库；
    // 本闸偏向拒绝再建。
    if dir.join(".hexagon/state.db").is_file() {
        return Err(SetupError::AlreadyProject(dir.to_path_buf()));
    }
    std::fs::create_dir_all(dir)?;
    on_step(CreateStep::CheckDir);

    // git 闸：非仓库须确认后才 init。已是仓库则无论干净或有未提交改动都直接用：
    // 不 init、不 commit、不 clean（ADR 0060 弃「脏树先停」；自动提交/stash 会动工作区）。
    // 失败不报告 Git，调用方停在这一步。
    if !git::is_repo(dir) {
        if init_git {
            git::init(dir, "main")?;
        } else {
            return Err(SetupError::NoGit(dir.to_path_buf()));
        }
    }
    on_step(CreateStep::Git);

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

    // 可见顺序是「角色落库 → 密钥复查」（票 14）。缺 key 仍不能留下项目：
    // 新建目录落库后再复查，失败则删掉本次的 .hexagon（返回时没有 state.db）。
    // 目录里本来就有 state.db 时不能这么删——那会误伤已有工作台，所以复查
    // 提前到落库之前，缺 key 直接停、不改库。被否决的替代：缺 key 也留着半成品库。
    let db_path = dir.join(".hexagon/state.db");
    let db_existed = db_path.exists();
    if db_existed {
        let missing = missing_model_keys(store, &picked, doc)?;
        if !missing.is_empty() {
            return Err(SetupError::MissingKeys(missing));
        }
    }
    let hex_existed = dir.join(".hexagon").exists();

    // 开项目 + 实例化角色（model_slot / globs / 技能授权）
    let pairs: Vec<(String, String)> = picked
        .iter()
        .enumerate()
        .map(|(i, r)| (format!("a{i}"), r.name.clone()))
        .collect();
    let persisted = (|| -> Result<Workbench, SetupError> {
        let wb = Workbench::open(dir, name, &pairs, pack.cloned())?;
        let db = Db::open(&db_path)?;
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
    })();
    let wb = match persisted {
        Ok(wb) => wb,
        Err(e) => {
            rollback_fresh_hexagon(dir, hex_existed);
            return Err(e);
        }
    };
    on_step(CreateStep::PersistRoles);

    let missing = missing_model_keys(store, &picked, doc)?;
    if !missing.is_empty() {
        drop(wb);
        rollback_fresh_hexagon(dir, hex_existed);
        return Err(SetupError::MissingKeys(missing));
    }
    on_step(CreateStep::RecheckKeys);
    // 工作台对象已经在手里。壳层随后才把它装进窗口；这一步失败则命令报错，
    // 界面不得因为本回调而进入工作台。
    on_step(CreateStep::OpenProject);
    Ok(wb)
}

/// 打开已有项目（ADR 0060）。只接受已有 `.hexagon/state.db` 的目录。
/// 不初始化 git、不提交、不清理工作区，不重跑角色落库（空角色表 + INSERT OR IGNORE）。
/// 被否决的替代：没有状态也 `Workbench::open`（那会把空目录建成新库）。
pub fn open_existing(dir: impl AsRef<Path>) -> Result<Workbench, SetupError> {
    let dir = dir.as_ref();
    if !dir.join(".hexagon/state.db").is_file() {
        return Err(SetupError::NotAProject(dir.to_path_buf()));
    }
    let pack = PackDef::pinned(dir).ok();
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    Ok(Workbench::open(dir, name, &[], pack)?)
}

/// 落库后失败：本次新建的 `.hexagon` 整目录删掉，避免缺密钥却留下项目库。
/// 目录事先就有 `.hexagon` 时不动（里面可能是已有工作台）。
fn rollback_fresh_hexagon(dir: &Path, hex_existed: bool) {
    if hex_existed {
        return;
    }
    let _ = std::fs::remove_dir_all(dir.join(".hexagon"));
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
        assert!(r.exists && r.empty && !r.is_git && !r.has_workbench && r.instructions.is_none());
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

    /// ADR 0060：干净的非空 git 仓库仍可开成项目。
    #[test]
    fn clean_nonempty_repo_becomes_project() {
        let d = tempfile::tempdir().unwrap();
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "kept").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        let head = git::head(d.path()).unwrap();
        assert!(!git::is_dirty(d.path()));
        assert!(!inspect_dir(d.path()).empty);
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
        assert!(d.path().join(".hexagon/state.db").is_file());
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "kept"
        );
        // 开项目本身也不提交（.hexagon 留在工作区，不进这次 HEAD）
        assert_eq!(git::head(d.path()).unwrap(), head);
    }

    /// ADR 0060 弃「脏树先停」：未提交改动在创建后仍在，且没有被提交、没有被清掉。
    /// 旧断言是 DirtyTree——那条闸把已有代码的仓库挡在外面。
    #[test]
    fn dirty_repo_becomes_project_without_commit_or_clean() {
        let d = tempfile::tempdir().unwrap();
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "base").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        let head = git::head(d.path()).unwrap();
        std::fs::write(d.path().join("README.md"), "dirty-work").unwrap();
        std::fs::write(d.path().join("notes.txt"), "scratch").unwrap();
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
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "dirty-work"
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("notes.txt")).unwrap(),
            "scratch"
        );
        assert_eq!(git::head(d.path()).unwrap(), head);
        let status = git::run(d.path(), &["status", "--porcelain"]).unwrap();
        assert!(
            status.lines().any(|l| l.contains("README.md")),
            "modified file must stay uncommitted, status={status}"
        );
        assert!(
            status.lines().any(|l| l.contains("notes.txt")),
            "untracked file must stay, status={status}"
        );
    }

    #[test]
    fn no_git_requires_confirm() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x.txt"), "keep").unwrap(); // 非空非仓库
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
        assert!(!git::is_repo(d.path()));
        assert!(!d.path().join(".hexagon/state.db").exists());
        assert_eq!(
            std::fs::read_to_string(d.path().join("x.txt")).unwrap(),
            "keep"
        );
    }

    /// 确认后才 `git init`。初始化不把文件夹里已有文件提交进去。
    #[test]
    fn non_git_folder_inits_only_after_confirm() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("plain");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.txt"), "keep").unwrap();
        create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert_eq!(std::fs::read_to_string(sub.join("x.txt")).unwrap(), "keep");
        assert!(
            !git::run(&sub, &["ls-files"]).unwrap().contains("x.txt"),
            "git init must not commit the folder's existing files"
        );
    }

    /// 空目录同样未确认不能 init；确认后才成为仓库。
    #[test]
    fn empty_dir_requires_confirm_before_git_init() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("empty");
        std::fs::create_dir(&sub).unwrap();
        assert!(inspect_dir(&sub).empty);
        assert!(matches!(
            create_project(
                &sub,
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
        assert!(!git::is_repo(&sub));
        create_project(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        assert!(git::is_repo(&sub));
        assert!(sub.join(".hexagon/state.db").is_file());
    }

    /// 已有工作台状态不能再建；打开不改花名册，也不提交/清理工作区。
    #[test]
    fn existing_workbench_refuses_recreate_and_opens() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("proj");
        let wb = create_project(
            &sub,
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
        )
        .unwrap();
        drop(wb);
        std::fs::write(sub.join("README.md"), "still-dirty").unwrap();
        let head = git::head(&sub).unwrap();
        assert!(inspect_dir(&sub).has_workbench);
        assert!(matches!(
            create_project(
                &sub,
                "另一个",
                &["后端".into()],
                &[],
                Some(&pack()),
                None,
                true,
                &store_with_key(),
                &doc_with_provider()
            ),
            Err(SetupError::AlreadyProject(_))
        ));
        assert_eq!(git::head(&sub).unwrap(), head);
        assert_eq!(
            std::fs::read_to_string(sub.join("README.md")).unwrap(),
            "still-dirty"
        );
        let opened = open_existing(&sub).unwrap();
        let n: i64 = opened
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let role: String = opened
            .db
            .conn()
            .query_row("SELECT role FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(role, "产品策划");
        let name: String = opened
            .db
            .conn()
            .query_row("SELECT name FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "仓");
        assert!(opened.pack.is_some());
        assert_eq!(git::head(&sub).unwrap(), head);
        assert!(matches!(
            open_existing(d.path().join("missing")),
            Err(SetupError::NotAProject(_))
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

    /// 票 14：五步按做完的顺序报告，成功才包含「打开项目」。
    #[test]
    fn create_reports_each_step_when_it_finishes() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("p");
        let mut seen = Vec::new();
        let wb = create_project_reporting(
            &sub,
            "测试项目",
            &["产品策划".into(), "后端".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &store_with_key(),
            &doc_with_provider(),
            |step| seen.push(step),
        )
        .unwrap();
        assert_eq!(seen, CREATE_STEP_ORDER.to_vec());
        assert!(sub.join(".hexagon/state.db").exists());
        drop(wb);
    }

    /// 票 14：失败停在该步，后面的步（尤其是打开项目）不报告，项目也没打开。
    #[test]
    fn create_failure_stops_before_later_steps() {
        let d = tempfile::tempdir().unwrap();

        // 脏树不再停（ADR 0060）：五步都报告，未提交改动还在，且没有被这次创建提交。
        git::init(d.path(), "main").unwrap();
        std::fs::write(d.path().join("README.md"), "x").unwrap();
        git::commit_all(d.path(), "init").unwrap();
        std::fs::write(d.path().join("README.md"), "dirty").unwrap();
        let mut seen = Vec::new();
        create_project_reporting(
            d.path(),
            "仓",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            |step| seen.push(step),
        )
        .unwrap();
        assert_eq!(seen, CREATE_STEP_ORDER.to_vec());
        assert_eq!(std::fs::read_to_string(d.path().join("README.md")).unwrap(), "dirty");
        assert!(git::is_dirty(d.path()));

        // 无 git 且未确认：同样停在 git，不打开。
        // 单独的临时目录——上一段的仓库是父目录，子目录会被 rev-parse 认成仓库。
        let bare_root = tempfile::tempdir().unwrap();
        let bare = bare_root.path().join("bare");
        std::fs::create_dir(&bare).unwrap();
        seen.clear();
        let err = match create_project_reporting(
            &bare,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            false,
            &store_with_key(),
            &doc_with_provider(),
            |step| seen.push(step),
        ) {
            Ok(_) => panic!("no git should stop"),
            Err(e) => e,
        };
        assert!(matches!(err, SetupError::NoGit(_)));
        assert_eq!(seen, vec![CreateStep::CheckDir]);

        // 缺密钥：目录和 git 已过，角色落了库但复查失败。打开不报告，库被收回。
        // 不放进上面的脏仓库里，否则 git 闸会先停。
        let fresh = tempfile::tempdir().unwrap();
        let sub = fresh.path().join("nokey");
        seen.clear();
        let err = match create_project_reporting(
            &sub,
            "p",
            &["产品策划".into()],
            &[],
            Some(&pack()),
            None,
            true,
            &MemoryStore::default(),
            &crate::provider_config::ProviderDoc::default(),
            |step| seen.push(step),
        ) {
            Ok(_) => panic!("missing keys should stop"),
            Err(e) => e,
        };
        assert!(matches!(err, SetupError::MissingKeys(_)));
        assert_eq!(
            seen,
            vec![
                CreateStep::CheckDir,
                CreateStep::Git,
                CreateStep::PersistRoles,
            ]
        );
        assert!(!seen.contains(&CreateStep::RecheckKeys));
        assert!(!seen.contains(&CreateStep::OpenProject));
        assert!(
            !sub.join(".hexagon/state.db").exists(),
            "missing keys must not leave a project behind"
        );
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
