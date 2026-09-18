//! 角色编辑与自定义角色（US6/US7）。
//!
//! 解析序：项目 `role_defs` 行 → 预置同名 RoleDef → 无名（自定义角色必有行）。
//! 编辑生效点：model_slot/归属 globs 即时（ctx_for/权限管线读库），
//! reviewer 即时（提案上级路由 superior_of 读库），duty/skills 下次激活简报生效。
//! 角色定义永不进提案面（ADR 0045）——这里是人手编辑面，不是 Agent 提案。

use crate::db::Db;
use crate::presets::{preset_roles, RoleDef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

/// 编辑面板数据：有效定义 + 实例字段 + 授权名单。
pub fn agent_detail(db: &Db, project_id: &str, agent_id: &str) -> Result<Value, RoleError> {
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
    let grants: Vec<Value> = st
        .query_map([agent_id], |r| {
            Ok(json!({"kind": r.get::<_,String>(0)?, "name": r.get::<_,String>(1)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "agent_id": agent_id, "role": role, "status": status,
        "model_slot": model_slot, "custom": custom,
        "def": {"duty": def.duty, "reviewer": def.reviewer,
                "model_slot": def.model_slot, "skills": def.skills},
        "globs": globs, "grants": grants,
    }))
}

/// 编辑补丁：None=不动。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentPatch {
    pub duty: Option<String>,
    /// Some("")=清掉上级直达负责人；Some(name)=设上级；None=不动
    pub reviewer: Option<String>,
    pub model_slot: Option<String>,
    pub skills: Option<Vec<String>>,
    pub globs: Option<Vec<String>>,
}
