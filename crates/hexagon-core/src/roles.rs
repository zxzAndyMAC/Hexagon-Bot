//! 角色编辑与自定义角色（US6/US7）。
//!
//! 解析序：项目 `role_defs` 行 → 预置同名 RoleDef → 无名（自定义角色必有行）。
//! 编辑生效点：model_slot/归属 globs 即时（ctx_for/权限管线读库），
//! reviewer 即时（提案上级路由 superior_of 读库），duty/skills 下次激活简报生效。
//! 角色定义永不进提案面（ADR 0045）——这里是人手编辑面，不是 Agent 提案。

use crate::db::Db;
use crate::presets::{preset_roles, RoleDef};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum RoleError {
    #[error("角色名不能为空")]
    EmptyName,
    #[error("角色已存在: {0}")]
    Duplicate(String),
    #[error("角色不存在: {0}")]
    UnknownRole(String),
    #[error("上级「{0}」不在团队名单内")]
    UnknownReviewer(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Preset(#[from] crate::presets::PresetError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// 解析某角色的有效定义：项目覆盖行 → 预置底稿 → 无名（Err）。
pub fn role_def(db: &Db, project_id: &str, role: &str) -> Result<RoleDef, RoleError> {
    if let Some(def) = project_def(db, project_id, role)? {
        return Ok(def);
    }
    if let Some(def) = preset_roles()?.into_iter().find(|r| r.name == role) {
        return Ok(def);
    }
    Err(RoleError::UnknownRole(role.into()))
}

/// 项目 role_defs 行（有则覆盖预置）。实例 globs 以 agent_globs 表为准，不回读。
fn project_def(db: &Db, project_id: &str, role: &str) -> Result<Option<RoleDef>, RoleError> {
    let row = db.conn().query_row(
        "SELECT name, duty, reviewer, model_slot, skills FROM role_defs
         WHERE project_id=?1 AND name=?2",
        rusqlite::params![project_id, role],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    );
    match row {
        Ok((name, duty, reviewer, model_slot, skills)) => Ok(Some(RoleDef {
            name,
            duty,
            reviewer,
            model_slot,
            globs: vec![], // globs 是实例级：agent_globs 表
            skills: serde_json::from_str(&skills).unwrap_or_default(),
        })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 上级解析（提案路由用）：项目定义优先，缺回落内置映射。
/// 返回 Option<String>（运行时值，非 'static）。
pub fn superior_of(db: &Db, project_id: &str, role: &str) -> Option<String> {
    if let Ok(Some(def)) = project_def(db, project_id, role) {
        if def.reviewer.is_some() {
            return def.reviewer;
        }
    }
    if let Ok(def) = role_def(db, project_id, role) {
        if def.reviewer.is_some() {
            return def.reviewer;
        }
    }
    crate::proposals::builtin_superior(role).map(str::to_string)
}

/// 编辑 Agent：写项目覆盖行 + 同步实例字段（model_slot/agent_globs）。
/// None 字段不动；globs=Some 时整表替换 agent_globs（即时生效）。
pub fn update_agent_def(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    patch: &AgentPatch,
) -> Result<(), RoleError> {
    let role: String = db
        .conn()
        .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
            r.get(0)
        })
        .map_err(|_| RoleError::UnknownRole(agent_id.into()))?;
    // 上级必须在团队名单内（或为空）
    if let Some(rev) = &patch.reviewer {
        if !rev.is_empty() {
            team_roles(db, project_id)?
                .iter()
                .find(|r| *r == rev)
                .ok_or_else(|| RoleError::UnknownReviewer(rev.clone()))?;
        }
    }
    let base = project_def(db, project_id, &role)?
        .or_else(|| preset_roles().ok()?.into_iter().find(|r| r.name == role))
        .unwrap_or(RoleDef {
            name: role.clone(),
            duty: String::new(),
            reviewer: None,
            model_slot: "default".into(),
            globs: vec![],
            skills: vec![],
        });
    let duty = patch.duty.clone().unwrap_or(base.duty);
    let reviewer = patch
        .reviewer
        .clone()
        .map(|r| if r.is_empty() { None } else { Some(r) })
        .unwrap_or(base.reviewer);
    let model_slot = patch.model_slot.clone().unwrap_or(base.model_slot);
    let skills = patch.skills.clone().unwrap_or(base.skills);
    db.conn().execute(
        "INSERT INTO role_defs (project_id, name, duty, reviewer, model_slot, skills, custom)
         VALUES (?1,?2,?3,?4,?5,?6,0)
         ON CONFLICT(project_id, name) DO UPDATE SET
           duty=excluded.duty, reviewer=excluded.reviewer,
           model_slot=excluded.model_slot, skills=excluded.skills",
        rusqlite::params![
            project_id,
            role,
            duty,
            reviewer,
            model_slot,
            serde_json::to_string(&skills)?
        ],
    )?;
    // 实例字段同步：model_slot 进 agents，globs 整表替换
    db.conn().execute(
        "UPDATE agents SET model_slot=?1 WHERE id=?2",
        rusqlite::params![model_slot, agent_id],
    )?;
    if let Some(globs) = &patch.globs {
        db.conn()
            .execute("DELETE FROM agent_globs WHERE agent_id=?1", [agent_id])?;
        for g in globs {
            if !g.trim().is_empty() {
                db.conn().execute(
                    "INSERT OR IGNORE INTO agent_globs (agent_id, glob) VALUES (?1,?2)",
                    rusqlite::params![agent_id, g],
                )?;
            }
        }
    }
    Ok(())
}

/// 自建自定义角色：校验 → role_defs(custom=1) + agents 行（休眠）+ globs 种子。
/// 返回新 agent_id。
pub fn create_role(db: &Db, project_id: &str, def: &RoleDef) -> Result<String, RoleError> {
    let name = def.name.trim();
    if name.is_empty() {
        return Err(RoleError::EmptyName);
    }
    if team_roles(db, project_id)?.iter().any(|r| r == name) {
        return Err(RoleError::Duplicate(name.into()));
    }
    if let Some(rev) = &def.reviewer {
        team_roles(db, project_id)?
            .iter()
            .find(|r| *r == rev)
            .ok_or_else(|| RoleError::UnknownReviewer(rev.clone()))?;
    }
    db.conn().execute(
        "INSERT INTO role_defs (project_id, name, duty, reviewer, model_slot, skills, custom)
         VALUES (?1,?2,?3,?4,?5,?6,1)",
        rusqlite::params![
            project_id,
            name,
            def.duty,
            def.reviewer,
            def.model_slot,
            serde_json::to_string(&def.skills)?
        ],
    )?;
    let aid = format!("a{}", db.next_id("a")?);
    db.conn().execute(
        "INSERT INTO agents (id, project_id, role, model_slot) VALUES (?1,?2,?3,?4)",
        rusqlite::params![aid, project_id, name, def.model_slot],
    )?;
    for g in &def.globs {
        if !g.trim().is_empty() {
            db.conn().execute(
                "INSERT OR IGNORE INTO agent_globs (agent_id, glob) VALUES (?1,?2)",
                rusqlite::params![aid, g],
            )?;
        }
    }
    Ok(aid)
}

/// 同一角色再加一个 Agent。角色定义仍是一份，经验技能按角色名共用。
///
/// 花名册按角色名点名时唤醒最早的那一行。第二行用来写同一份经验，不是另一份目录。
/// 被否决：第二笔经验仍由同一个 agent id 再写一遍——那样「另一个 Agent」并不存在。
pub fn spawn_peer(db: &Db, project_id: &str, role: &str) -> Result<String, RoleError> {
    let role = role.trim();
    if role.is_empty() {
        return Err(RoleError::EmptyName);
    }
    let (src, model_slot): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT id, model_slot FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id LIMIT 1",
            rusqlite::params![project_id, role],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => RoleError::UnknownRole(role.into()),
            other => RoleError::Sqlite(other),
        })?;
    // for_test 直接写成 a0、a1，不走计数器。next_id 从 1 起会撞上 a1。
    let aid = loop {
        let candidate = format!("a{}", db.next_id("a")?);
        let taken: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM agents WHERE id=?1",
            [&candidate],
            |r| r.get(0),
        )?;
        if taken == 0 {
            break candidate;
        }
    };
    db.conn().execute(
        "INSERT INTO agents (id, project_id, role, model_slot, status) VALUES (?1,?2,?3,?4,'sleeping')",
        rusqlite::params![aid, project_id, role, model_slot],
    )?;
    db.conn().execute(
        "INSERT INTO agent_globs (agent_id, glob) SELECT ?1, glob FROM agent_globs WHERE agent_id=?2",
        rusqlite::params![aid, src],
    )?;
    Ok(aid)
}

/// 授权名单整表替换（kind 维）：装完授权默认空的原则不动——这里是人手显式授权。
pub fn set_grants(db: &Db, agent_id: &str, kind: &str, names: &[String]) -> Result<(), RoleError> {
    db.conn().execute(
        "DELETE FROM grants WHERE agent_id=?1 AND kind=?2",
        rusqlite::params![agent_id, kind],
    )?;
    for n in names {
        if n.trim().is_empty() {
            continue;
        }
        db.conn().execute(
            "INSERT OR IGNORE INTO grants (id, agent_id, kind, name) VALUES (?1,?2,?3,?4)",
            rusqlite::params![format!("g{}", db.next_id("g")?), agent_id, kind, n],
        )?;
    }
    Ok(())
}

/// 团队角色名单（agents 表）。
pub fn team_roles(db: &Db, project_id: &str) -> Result<Vec<String>, RoleError> {
    let mut st = db
        .conn()
        .prepare("SELECT role FROM agents WHERE project_id=?1 ORDER BY created_at")?;
    let rows = st
        .query_map([project_id.to_string()], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(rows)
}

/// 授权名单行（ADR 0054）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct GrantRow {
    #[ts(type = "'mcp' | 'skill'")] // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub kind: String,
    pub name: String,
}

/// 编辑面板数据：有效定义段（RoleDef 子集）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct AgentDetailDef {
    pub duty: String,
    pub reviewer: Option<String>,
    pub model_slot: String,
    pub skills: Vec<String>,
}

/// Agent 详情（ADR 0054）：有效定义 + 实例字段 + globs + 授权名单。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct AgentDetail {
    pub agent_id: String,
    pub role: String,
    #[ts(type = "'active' | 'sleeping'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub status: String,
    pub model_slot: Option<String>,
    pub custom: bool,
    pub def: AgentDetailDef,
    pub globs: Vec<String>,
    pub grants: Vec<GrantRow>,
}

/// 编辑面板数据：有效定义 + 实例字段 + 授权名单。
pub fn agent_detail(db: &Db, project_id: &str, agent_id: &str) -> Result<AgentDetail, RoleError> {
    let (role, model_slot, status): (String, Option<String>, String) = db
        .conn()
        .query_row(
            "SELECT role, model_slot, status FROM agents WHERE id=?1",
            [agent_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| RoleError::UnknownRole(agent_id.into()))?;
    let def = role_def(db, project_id, &role)?;
    let custom: bool = db
        .conn()
        .query_row(
            "SELECT custom FROM role_defs WHERE project_id=?1 AND name=?2",
            rusqlite::params![project_id, role],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c == 1)
        .unwrap_or(false);
    let globs = crate::permissions::agent_globs(db, agent_id).unwrap_or_default();
    let mut st = db
        .conn()
        .prepare("SELECT kind, name FROM grants WHERE agent_id=?1")?;
    let grants: Vec<GrantRow> = st
        .query_map([agent_id], |r| {
            Ok(GrantRow {
                kind: r.get(0)?,
                name: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AgentDetail {
        agent_id: agent_id.into(),
        role,
        status,
        model_slot,
        custom,
        def: AgentDetailDef {
            duty: def.duty,
            reviewer: def.reviewer,
            model_slot: def.model_slot,
            skills: def.skills,
        },
        globs,
        grants,
    })
}

/// 读头像 → data URL（前端直接 <img src>）；未设置回 None。
/// 纯文件读（ADR 0052）：只需 repo_root，不摸 Db、不占 wb 锁。
pub fn agent_avatar(
    repo_root: &std::path::Path,
    agent_id: &str,
) -> Result<Option<String>, RoleError> {
    use base64::Engine;
    let dir = repo_root.join(".hexagon/avatars");
    for (ext, mime) in [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("webp", "image/webp"),
        ("gif", "image/gif"),
    ] {
        let p = dir.join(format!("{agent_id}.{ext}"));
        if p.exists() {
            crate::db::validate_generic_file_access(&p)?;
            let b = std::fs::read(&p)?;
            return Ok(Some(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(b)
            )));
        }
    }
    Ok(None)
}

/// 头像内容哈希（arch-review 票 07）：team 行随行下发，UI 只在哈希
/// 变化时拉 data URL——取代轮询期每 2s × N agent 的盲拉。
/// DefaultHasher::new() 定值（非 RandomState），同工具链内可复现；
/// 跨工具链换算法最多多一次拉取，无正确性问题。
pub fn avatar_hash(
    repo_root: &std::path::Path,
    agent_id: &str,
) -> Result<Option<String>, RoleError> {
    use std::hash::{Hash, Hasher};
    let dir = repo_root.join(".hexagon/avatars");
    for ext in ["png", "jpg", "webp", "gif"] {
        let p = dir.join(format!("{agent_id}.{ext}"));
        if p.exists() {
            crate::db::validate_generic_file_access(&p)?;
            let b = std::fs::read(&p)?;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            b.hash(&mut h);
            return Ok(Some(format!("{:016x}", h.finish())));
        }
    }
    Ok(None)
}

/// 设置 agent 头像：UI 传 data URL（data:image/png;base64,…），
/// 落盘 `<root>/.hexagon/avatars/<agent>.<ext>`，换扩展名时清旧文件。
pub fn set_agent_avatar(
    repo_root: &std::path::Path,
    agent_id: &str,
    data_url: &str,
) -> Result<(), RoleError> {
    use base64::Engine;
    let invalid = || {
        RoleError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected data:<mime>;base64,<payload>",
        ))
    };
    let (mime, b64) = data_url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
        .ok_or_else(invalid)?;
    let ext = match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| RoleError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(RoleError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "avatar >2MB",
        )));
    }
    let dir = repo_root.join(".hexagon/avatars");
    std::fs::create_dir_all(&dir)?;
    for e in ["png", "jpg", "webp", "gif"] {
        let p = dir.join(format!("{agent_id}.{e}"));
        if e != ext && p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    std::fs::write(dir.join(format!("{agent_id}.{ext}")), bytes)?;
    Ok(())
}

/// 编辑补丁：None=不动。
#[derive(Debug, Default, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct AgentPatch {
    #[ts(optional)]
    pub duty: Option<String>,
    /// Some("")=清掉上级直达负责人；Some(name)=设上级；None=不动
    #[ts(optional)]
    pub reviewer: Option<String>,
    #[ts(optional)]
    pub model_slot: Option<String>,
    #[ts(optional)]
    pub skills: Option<Vec<String>>,
    #[ts(optional)]
    pub globs: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::RoleDef;

    fn def(name: &str, reviewer: Option<&str>) -> RoleDef {
        RoleDef {
            name: name.into(),
            duty: format!("{name}的活"),
            reviewer: reviewer.map(|r| r.into()),
            model_slot: "default".into(),
            globs: vec!["src/**".into()],
            skills: vec![],
        }
    }

    /// create_role 三道闸：空名/重名/上级不在团队——全拒；
    /// 通过则落 role_defs（custom=1）+ agents + agent_globs。
    #[test]
    fn create_role_validates_then_creates() {
        let dir = tempfile::tempdir().unwrap();
        // 单角色：for_test 的 a0 不占 id_counters——create_role 的
        // next_id("a") 从 1 起（a1），预置两个 agent 会撞上 a1。
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let db = &wb.db;
        let pid = &wb.project_id;

        assert!(matches!(
            create_role(db, pid, &def("  ", None)),
            Err(RoleError::EmptyName)
        ));
        assert!(matches!(
            create_role(db, pid, &def("后端", None)),
            Err(RoleError::Duplicate(_))
        ));
        assert!(matches!(
            create_role(db, pid, &def("测试", Some("不存在的人"))),
            Err(RoleError::UnknownReviewer(_))
        ));

        let aid = create_role(db, pid, &def("测试", Some("后端"))).unwrap();
        assert!(team_roles(db, pid).unwrap().contains(&"测试".to_string()));
        // globs 落实例表
        let g = crate::permissions::agent_globs(db, &aid).unwrap();
        assert_eq!(g, vec!["src/**".to_string()]);
    }

    /// 有效定义：项目 role_defs 覆盖行赢预置底稿；无名角色报错。
    #[test]
    fn role_def_project_row_beats_preset() {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let db = &wb.db;

        // 预置底稿：「后端」存在且 duty 来自预置
        let preset = role_def(db, &wb.project_id, "后端").unwrap();
        assert!(!preset.duty.is_empty());

        // 项目覆盖行：同名不同 duty → 以项目为准
        db.conn()
            .execute(
                "INSERT INTO role_defs (project_id, name, duty, reviewer, model_slot, skills, custom)
                 VALUES (?1,'后端','项目定制职责',NULL,'fast','[]',1)",
                [&wb.project_id],
            )
            .unwrap();
        let d = role_def(db, &wb.project_id, "后端").unwrap();
        assert_eq!(d.duty, "项目定制职责");

        assert!(matches!(
            role_def(db, &wb.project_id, "没这个角色"),
            Err(RoleError::UnknownRole(_))
        ));
    }

    /// set_grants 是整表替换非追加；agent_detail 反映 grants/custom。
    #[test]
    fn grants_replace_and_detail_reflects() {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["后端"], None).unwrap();
        let db = &wb.db;
        let aid = "a0";

        set_grants(db, aid, "mcp", &["svc-a".into(), "svc-b".into()]).unwrap();
        set_grants(db, aid, "mcp", &["svc-c".into()]).unwrap();
        let d = serde_json::to_value(agent_detail(db, &wb.project_id, aid).unwrap()).unwrap();
        let names: Vec<&str> = d["grants"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|g| g["name"].as_str())
            .collect();
        assert_eq!(names, vec!["svc-c"], "set_grants 应整表替换而非追加");
        assert_eq!(d["role"], "后端");
        assert_eq!(d["custom"], false); // 预置角色非项目自定义

        // 自定义角色 → custom=true
        let aid2 = create_role(db, &wb.project_id, &def("自定义", None)).unwrap();
        let d = serde_json::to_value(agent_detail(db, &wb.project_id, &aid2).unwrap()).unwrap();
        assert_eq!(d["custom"], true);
    }
}
