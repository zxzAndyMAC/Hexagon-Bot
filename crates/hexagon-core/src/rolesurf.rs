//! 角色定义的窄提案面（ADR 0069）。
//!
//! 可改职责、上级、模型槽，以及收窄路径归属或授权名单。放宽在执行判定前拒绝。
//! 证据是上级复审通过，不接受回放。权限规则和授权清单本身不是提案面。
//!
//! 代价：把放宽判成可送判定 = 角色借提案扩大路径或授权（false negative，未审副作用）。
//! 把收窄判成放宽 = 一次收紧被拒绝，负责人再改（false positive）。偏向拒绝。
//! 被否决：放宽也送给 Jev——判定不能推翻这道机械拒绝。

use crate::db::Db;
use crate::proposals::PropError;
use crate::tools::ToolContext;
use serde_json::{json, Value};
use std::path::Path;

pub fn payload(body: &str) -> Option<Value> {
    crate::proposals::fenced(body, "role").and_then(|raw| serde_json::from_str(&raw).ok())
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    })
}

pub(crate) fn globs_widen(current: &[String], next: &[String]) -> bool {
    if current.is_empty() {
        return false;
    }
    next.is_empty() || next.iter().any(|n| !current.contains(n))
}

fn list_widen(current: &[String], next: &[String]) -> bool {
    next.iter().any(|n| !current.contains(n))
}

pub fn audit(db: &Db, ctx: &ToolContext, body: &str) -> Result<(), PropError> {
    if crate::proposals::fenced(body, "replay").is_some() {
        return Err(PropError::Rejected(
            "role definition does not take replay as evidence".into(),
        ));
    }
    let Some(p) = payload(body) else {
        return Err(PropError::Rejected("missing role block".into()));
    };
    let role = p["role"].as_str().unwrap_or("");
    if role.is_empty() {
        return Err(PropError::Rejected("missing role name".into()));
    }
    let agents = role_agent_ids(db, &ctx.project_id, role)?;
    if let Some(next) = strings(&p["globs"]) {
        for agent in &agents {
            let mut st = db
                .conn()
                .prepare("SELECT glob FROM agent_globs WHERE agent_id=?1 ORDER BY glob")?;
            let current: Vec<String> = st
                .query_map([agent], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            if globs_widen(&current, &next) {
                return Err(PropError::Rejected(
                    "role proposal must not widen path ownership".into(),
                ));
            }
        }
    }
    if let Some(next) = strings(&p["grants"]) {
        for agent in &agents {
            let mut st = db.conn().prepare(
                "SELECT kind || ':' || name FROM grants WHERE agent_id=?1 ORDER BY kind, name",
            )?;
            let current: Vec<String> = st
                .query_map([agent], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            if list_widen(&current, &next) {
                return Err(PropError::Rejected(
                    "role proposal must not widen grants".into(),
                ));
            }
        }
    }
    Ok(())
}

fn role_agent_ids(db: &Db, project_id: &str, role: &str) -> Result<Vec<String>, PropError> {
    let mut st = db
        .conn()
        .prepare("SELECT id FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id")?;
    let ids: Vec<String> = st
        .query_map(rusqlite::params![project_id, role], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if ids.is_empty() {
        return Err(PropError::Rejected(format!("unknown role: {role}")));
    }
    Ok(ids)
}

pub fn apply(db: &Db, ctx: &ToolContext, body: &str, backup: &Path) -> Result<bool, PropError> {
    let Some(p) = payload(body) else {
        return Ok(false);
    };
    audit(db, ctx, body)?;
    let role = p["role"].as_str().unwrap_or("").to_string();
    let agent: String = db.conn().query_row(
        "SELECT id FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id LIMIT 1",
        rusqlite::params![ctx.project_id, role],
        |r| r.get(0),
    )?;
    let duty: Option<String> = db
        .conn()
        .query_row(
            "SELECT duty FROM role_defs WHERE project_id=?1 AND name=?2",
            rusqlite::params![ctx.project_id, role],
            |r| r.get(0),
        )
        .ok();
    let reviewer: Option<String> = db
        .conn()
        .query_row(
            "SELECT reviewer FROM role_defs WHERE project_id=?1 AND name=?2",
            rusqlite::params![ctx.project_id, role],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    let model_slot: Option<String> = db
        .conn()
        .query_row("SELECT model_slot FROM agents WHERE id=?1", [&agent], |r| {
            r.get(0)
        })
        .ok()
        .flatten();
    let mut st = db
        .conn()
        .prepare("SELECT glob FROM agent_globs WHERE agent_id=?1")?;
    let globs: Vec<String> = st
        .query_map([&agent], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut st = db
        .conn()
        .prepare("SELECT kind, name FROM grants WHERE agent_id=?1")?;
    let grants: Vec<(String, String)> = st
        .query_map([&agent], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let before = json!({
        "role": role,
        "agent": agent,
        "had_row": duty.is_some(),
        "duty": duty,
        "reviewer": reviewer,
        "model_slot": model_slot,
        "globs": globs,
        "grants": grants,
    });
    std::fs::create_dir_all(backup)?;
    let text = serde_json::to_string(&before).map_err(|e| PropError::Rejected(e.to_string()))?;
    std::fs::write(backup.join("role.json"), text)?;
    if let Some(duty) = p["duty"].as_str() {
        db.conn().execute(
            "INSERT INTO role_defs (project_id, name, duty) VALUES (?1,?2,?3)
             ON CONFLICT(project_id, name) DO UPDATE SET duty=excluded.duty",
            rusqlite::params![ctx.project_id, role, duty],
        )?;
    }
    if p.get("reviewer").is_some() {
        let reviewer = p["reviewer"].as_str().filter(|s| !s.is_empty());
        db.conn().execute(
            "INSERT INTO role_defs (project_id, name, reviewer) VALUES (?1,?2,?3)
             ON CONFLICT(project_id, name) DO UPDATE SET reviewer=excluded.reviewer",
            rusqlite::params![ctx.project_id, role, reviewer],
        )?;
    }
    if let Some(slot) = p["model_slot"].as_str() {
        db.conn().execute(
            "UPDATE agents SET model_slot=?1 WHERE project_id=?2 AND role=?3",
            rusqlite::params![slot, ctx.project_id, role],
        )?;
        db.conn().execute(
            "INSERT INTO role_defs (project_id, name, model_slot) VALUES (?1,?2,?3)
             ON CONFLICT(project_id, name) DO UPDATE SET model_slot=excluded.model_slot",
            rusqlite::params![ctx.project_id, role, slot],
        )?;
    }
    if let Some(globs) = strings(&p["globs"]) {
        for id in role_agent_ids(db, &ctx.project_id, &role)? {
            db.conn()
                .execute("DELETE FROM agent_globs WHERE agent_id=?1", [&id])?;
            for g in &globs {
                db.conn().execute(
                    "INSERT INTO agent_globs (agent_id, glob) VALUES (?1,?2)",
                    rusqlite::params![id, g],
                )?;
            }
        }
    }
    if let Some(grants) = strings(&p["grants"]) {
        for id in role_agent_ids(db, &ctx.project_id, &role)? {
            let mut st = db
                .conn()
                .prepare("SELECT id, kind || ':' || name FROM grants WHERE agent_id=?1")?;
            let rows: Vec<(String, String)> = st
                .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            for (gid, key) in rows {
                if !grants.contains(&key) {
                    db.conn()
                        .execute("DELETE FROM grants WHERE id=?1", [&gid])?;
                }
            }
        }
    }
    let _ = ctx;
    Ok(true)
}

pub fn rollback(db: &Db, ctx: &ToolContext, backup: &Path) -> Result<bool, PropError> {
    let path = backup.join("role.json");
    if !path.exists() {
        return Ok(false);
    }
    let before: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)
        .map_err(|e| PropError::Rejected(e.to_string()))?;
    let role = before["role"].as_str().unwrap_or("");
    let agent = before["agent"].as_str().unwrap_or("");
    if before["had_row"] == true {
        db.conn().execute(
            "UPDATE role_defs SET duty=?1, reviewer=?2, model_slot=?3 WHERE project_id=?4 AND name=?5",
            rusqlite::params![
                before["duty"].as_str().unwrap_or(""),
                before["reviewer"].as_str(),
                before["model_slot"].as_str().unwrap_or("default"),
                ctx.project_id,
                role,
            ],
        )?;
    }
    if let Some(slot) = before["model_slot"].as_str() {
        db.conn().execute(
            "UPDATE agents SET model_slot=?1 WHERE id=?2",
            rusqlite::params![slot, agent],
        )?;
    }
    db.conn()
        .execute("DELETE FROM agent_globs WHERE agent_id=?1", [agent])?;
    if let Some(globs) = before["globs"].as_array() {
        for g in globs.iter().filter_map(|v| v.as_str()) {
            db.conn().execute(
                "INSERT INTO agent_globs (agent_id, glob) VALUES (?1,?2)",
                rusqlite::params![agent, g],
            )?;
        }
    }
    db.conn()
        .execute("DELETE FROM grants WHERE agent_id=?1", [agent])?;
    if let Some(grants) = before["grants"].as_array() {
        for g in grants {
            let id = format!("g{}", db.next_id("g")?);
            db.conn().execute(
                "INSERT INTO grants (id, agent_id, kind, name) VALUES (?1,?2,?3,?4)",
                rusqlite::params![
                    id,
                    agent,
                    g[0].as_str().unwrap_or(""),
                    g[1].as_str().unwrap_or("")
                ],
            )?;
        }
    }
    Ok(true)
}

pub fn reviewed(db: &Db, proposal_id: &str) -> Result<bool, PropError> {
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM events
         WHERE kind='proposal_reviewed' AND json_extract(payload,'$.proposal_id')=?1
           AND json_extract(payload,'$.pass')=1",
        [proposal_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::globs_widen;
    use proptest::prelude::*;

    proptest! {
        /// 从现任里抽出来的非空子集不是放宽。
        #[test]
        fn subset_of_owned_paths_is_not_wider(
            current in prop::collection::vec("[a-z]{1,3}", 1..6usize),
            picks in prop::collection::vec(0..6usize, 1..6usize),
        ) {
            let next: Vec<String> = picks
                .iter()
                .map(|i| current[i % current.len()].clone())
                .collect();
            prop_assert!(!globs_widen(&current, &next));
        }

        /// 现任非空时，多出来的路径（带点，不可能等于纯字母项）是放宽。
        #[test]
        fn a_path_outside_the_current_set_is_wider(
            current in prop::collection::vec("[a-z]{1,3}", 1..6usize),
        ) {
            let mut next = current.clone();
            next.push(format!("{}.x", current[0]));
            prop_assert!(globs_widen(&current, &next));
        }

        /// 现任非空时，收成空集等于取消归属限制，是放宽。
        #[test]
        fn clearing_owned_paths_is_wider(
            current in prop::collection::vec("[a-z]{1,3}", 1..6usize),
        ) {
            prop_assert!(globs_widen(&current, &[]));
        }

        /// 现任本来就没有归属行时，写上路径是收紧，不是放宽。
        #[test]
        fn empty_ownership_does_not_count_added_paths_as_wider(
            next in prop::collection::vec("[a-z]{1,3}", 0..6usize),
        ) {
            prop_assert!(!globs_widen(&[], &next));
        }
    }
}
