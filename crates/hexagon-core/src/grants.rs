//! 技能与 MCP 的授权确认（hands-free 票 04 / ADR 0063）。
//!
//! 授权是项目内 grants 表的一行，不是权限规则，也不是用户全局清单。
//! L0–L3 出确认卡，负责人点头才写入。L4 自动写入并留轨迹，不留待决卡。
//! 两条路都只 `INSERT` 当前项目的 grants 行，不碰 `~/.hexagon/`，
//! 也不改项目技能目录或项目 MCP 清单——那些是安装的写入面。
//!
//! 被否决：L4 调用 `skills::save_global_skill` / `mcp::save_global_mcp`。
//! 那会把这一档的自动通过写成用户全局。也否决：用 `set_grants` 整表替换，
//! 一次确认会抹掉该 Agent 已有的同 kind 授权。
//!
//! 读档失败按 0（等人）。false negative 多一张卡；false positive 是
//! 未审授权生效。

use crate::db::Db;
use crate::harnessgate::{self, HarnessAction};
use crate::trace::EventKind;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    BadInput(String),
    #[error("agent not in project: {0}")]
    UnknownAgent(String),
}

/// 授权确认回执。`via` = autonomy | owner | queued。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct GrantOutcome {
    pub granted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_id: Option<String>,
    pub via: String,
}

fn action_for(kind: &str) -> Result<HarnessAction, GrantError> {
    match kind {
        "skill" => Ok(HarnessAction::SkillGrant),
        "mcp" => Ok(HarnessAction::McpGrant),
        other => Err(GrantError::BadInput(format!(
            "grant kind must be skill or mcp: {other}"
        ))),
    }
}

/// 名字进 grants 表，不进路径。斜杠和 `..` 拒绝，避免和安装路径混用。
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.starts_with('.')
}

fn agent_in_project(db: &Db, project_id: &str, agent_id: &str) -> Result<(), GrantError> {
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM agents WHERE id=?1 AND project_id=?2",
        rusqlite::params![agent_id, project_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(GrantError::UnknownAgent(agent_id.into()));
    }
    Ok(())
}

fn insert_grant(db: &Db, agent_id: &str, kind: &str, name: &str) -> Result<(), GrantError> {
    let id = format!("g{}", db.next_id("g")?);
    db.conn().execute(
        "INSERT OR IGNORE INTO grants (id, agent_id, kind, name) VALUES (?1,?2,?3,?4)",
        rusqlite::params![id, agent_id, kind, name],
    )?;
    Ok(())
}

fn trace(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    kind: &str,
    name: &str,
    via: &str,
    allowed: bool,
) -> Result<(), GrantError> {
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "grant_confirmed",
            "via": via,
            "agent_id": agent_id,
            "grant_kind": kind,
            "name": name,
            "allowed": allowed,
            "scope": "project",
        }),
        Some(agent_id),
        None,
    )?;
    Ok(())
}

/// 请求把 `name` 授给这个 Agent。L4 直接写入项目 grants；否则只入队。
pub fn request(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    kind: &str,
    name: &str,
) -> Result<GrantOutcome, GrantError> {
    let action = action_for(kind)?;
    if !valid_name(name) {
        return Err(GrantError::BadInput(format!("bad grant name: {name}")));
    }
    agent_in_project(db, project_id, agent_id)?;
    // 读档失败按等人。不把一次查询故障升成未审授权。
    let rank = crate::autonomy::rank(db, project_id).unwrap_or(0);
    if harnessgate::auto_passes(rank, action) {
        insert_grant(db, agent_id, kind, name)?;
        trace(db, project_id, agent_id, kind, name, "autonomy", true)?;
        return Ok(GrantOutcome {
            granted: true,
            question_id: None,
            via: "autonomy".into(),
        });
    }
    let qid = crate::cards::enqueue(
        db,
        project_id,
        Some(agent_id),
        crate::cards::CardKind::Grant,
        json!({
            "agent_id": agent_id,
            "grant_kind": kind,
            "name": name,
            "scope": "project",
        }),
        None,
    )?;
    Ok(GrantOutcome {
        granted: false,
        question_id: Some(qid),
        via: "queued".into(),
    })
}

/// 负责人裁决授权卡。允许才写入项目 grants；驳回只留痕。
pub fn confirm(
    db: &Db,
    project_id: &str,
    qid: &str,
    allow: bool,
) -> Result<GrantOutcome, GrantError> {
    let card = crate::cards::get_queued(db, qid, crate::cards::CardKind::Grant)
        .map_err(|_| GrantError::BadInput(format!("unknown grant question: {qid}")))?;
    if card.project_id != project_id {
        return Err(GrantError::BadInput(format!(
            "unknown grant question: {qid}"
        )));
    }
    let agent_id = card.payload["agent_id"].as_str().unwrap_or("").to_string();
    let kind = card.payload["grant_kind"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let name = card.payload["name"].as_str().unwrap_or("").to_string();
    action_for(&kind)?;
    if !valid_name(&name) {
        return Err(GrantError::BadInput(format!("bad grant name: {name}")));
    }
    crate::cards::answer(db, qid, "owner")?;
    if allow {
        insert_grant(db, &agent_id, &kind, &name)?;
    }
    trace(db, project_id, &agent_id, &kind, &name, "owner", allow)?;
    Ok(GrantOutcome {
        granted: allow,
        question_id: Some(qid.to_string()),
        via: "owner".into(),
    })
}
