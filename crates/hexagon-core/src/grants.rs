//! 技能与 MCP 的授权确认（hands-free 票 04 / ADR 0063）。
//!
//! 授权是项目内 grants 表的一行，不是权限规则，也不是用户全局清单。
//! 2026-10-01 ticket 05 / Q10：新增能力授权在所有模式下先出负责人确认卡。
//! 旧固定 L4 自动写入绕过了受限模式。广泛访问也不代表已审阅未知 MCP。
//! false negative 多一次人工；false positive 是未审能力生效，故偏向确认。
//! 已安装技能的任务自动选择由 LoadSkill 管理，不经过这里。

use crate::db::Db;
use crate::harnessgate::HarnessAction;
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

/// Governance 14: called only inside the reviewed experience receipt transaction,
/// never the autonomous grant-request path; ownership stays with the work author.
pub(crate) fn grant_reviewed_experience(
    db: &Db,
    project: &str,
    author: &str,
    skill: &str,
) -> Result<(), GrantError> {
    agent_in_project(db, project, author)?;
    insert_grant(db, author, "skill", skill)?;
    trace(
        db,
        project,
        author,
        "skill",
        skill,
        "reviewed_experience",
        true,
    )
}

/// 请求新增能力授权；负责人确认前不写 grants。
pub fn request(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    kind: &str,
    name: &str,
) -> Result<GrantOutcome, GrantError> {
    action_for(kind)?;
    if !valid_name(name) {
        return Err(GrantError::BadInput(format!("bad grant name: {name}")));
    }
    agent_in_project(db, project_id, agent_id)?;
    // Ticket 05: all modes preserve owner review of newly introduced capabilities.
    let started = std::time::Instant::now();
    crate::diag::note(
        crate::diag::CLASS_JUDGE,
        false,
        Some(project_id),
        Some(agent_id),
        None,
        None,
        "grant",
        "queued_unknown_capability",
        started,
    );
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
