//! 功能缺口 A2–A5（2026-10-15，负责人通则 2026-10-14：原型涉及的读模型直接补）：托管报告与胶囊弹层的
//! 宿主事实——托管时段（A2）、项目预算与本次托管花费（A3）、报告导出落盘（A4）、决定的范围 / 证据分类（A5）。
//! 票 08 偏差 1 / 2 当时因为宿主不给这些事实而没画；这里只出事实，不替 UI 造数。
use super::*;
use std::io::Write as _;

/// 决定行的范围分类（managed#running「范围外」chip）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ManagedScope {
    InScope,
    OutOfScope,
}

/// 已完成项的证据分类（managed#report「真实检查 / 项目内 / 有质量证据」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum ManagedEvidence {
    /// 宿主看过当前检查 / 验收证据（盖章证据齐全、运行检查）。
    RealCheck,
    /// 独立质量评估合格（提案采纳）。
    QualityEvidence,
    /// 宿主刚复核过绑定事实（发布目标与交付、工作树绑定）。
    HostVerified,
    /// 只作用于本项目（项目内安装 / 已安装能力直接使用）。
    InProject,
    /// 模型在封闭选项内的选择，宿主没有额外证据。
    ModelChoice,
    /// 宿主固定规则（续接、恢复等），没有额外证据。
    HostRule,
}

/// 宿主拒绝「超出托管授权」的原因码：缺发布目标 / 目标在卡发出后被改过（票 15 发布卡）/ 凭据
/// （托管不会新增或改动目标、不碰账号）、
/// 需要登录、宿主无法证明不删外部内容、绝不降低验收标准。其余拒绝是范围内的判断。
const OUT_OF_SCOPE_REASONS: &[&str] = &[
    "target_missing",
    "target_changed",
    "credential_missing",
    "credentials_or_login_required",
    "host_shell_cannot_exclude_external_deletion",
    "acceptance_never_lowered",
];

/// 封闭表：范围与证据只按宿主写下的 (action, reason, via) 判，不读模型给的理由文本。
/// 代价模型：把范围内误判成「范围外」花负责人一次核对；把模型选择误标成「真实检查」会让未核实的
/// 结果冒充有证据——所以只有 via = host 且原因码在表里的才给证据档，其余落 ModelChoice / HostRule。
pub fn classify(action: &str, reason: &str, via: &str) -> (ManagedScope, ManagedEvidence) {
    let scope = if via == "host" && OUT_OF_SCOPE_REASONS.contains(&reason) {
        ManagedScope::OutOfScope
    } else {
        ManagedScope::InScope
    };
    if via != "host" {
        return (scope, ManagedEvidence::ModelChoice);
    }
    let evidence = match (action, reason) {
        ("accept", "evidence_complete") | ("run_checks", "checks_missing_or_stale") => {
            ManagedEvidence::RealCheck
        }
        ("adopt", "independent_quality_qualified") => ManagedEvidence::QualityEvidence,
        ("push", "configured_target_accepted_delivery")
        | ("release", "registered_target_credentials_present")
        | ("confirm", "host_verified_worktree_binding") => ManagedEvidence::HostVerified,
        ("direct_use", "installed_capability_direct_use") => ManagedEvidence::InProject,
        _ => ManagedEvidence::HostRule,
    };
    (scope, evidence)
}

/// 胶囊弹层与报告头的托管事实。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ManagedSummary {
    /// 最近一次托管（同一 delegation_id，暂停 / 继续不断开）开启的时刻；从未开启为 null。
    pub started_at: Option<String>,
    /// 这次托管关闭的时刻；仍在托管（含暂停）为 null。
    pub ended_at: Option<String>,
    /// 项目用量上限（分）；null = 不限。
    #[ts(type = "number | null")]
    pub limit_cents: Option<i64>,
    /// 项目累计花费（毫分）。
    #[ts(type = "number")]
    pub spent_mc: i64,
    /// 本次托管时段内的花费（毫分）；从未开启为 0。
    #[ts(type = "number")]
    pub session_spent_mc: i64,
    /// 时段内有成本未知的用量行：花费是下界，UI 要写明。
    pub cost_unknown: bool,
}

/// 最近一次托管时段：`delegation_changed` 里 enabled=true 且换了 delegation_id 开始，enabled=false 结束。
fn session(db: &Db, project_id: &str) -> Result<(Option<String>, Option<String>), ApiError> {
    let mut statement = db.conn().prepare(
        "SELECT json_extract(payload,'$.enabled'), json_extract(payload,'$.delegation_id'), created_at
         FROM events WHERE project_id=?1 AND kind='system'
           AND json_extract(payload,'$.kind')='delegation_changed' ORDER BY id",
    )?;
    let rows = statement.query_map([project_id], |r| {
        Ok((
            r.get::<_, Option<i64>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let (mut id, mut start, mut end): (Option<String>, Option<String>, Option<String>) =
        (None, None, None);
    for row in rows {
        let (enabled, grant, at) = row?;
        match (enabled == Some(1), grant) {
            (true, Some(grant)) if id.as_ref() != Some(&grant) || end.is_some() => {
                id = Some(grant);
                start = Some(at);
                end = None;
            }
            (false, _) if start.is_some() && end.is_none() => end = Some(at),
            _ => {}
        }
    }
    Ok((start, end))
}

pub fn managed_summary(db: &Db, project_id: &str) -> Result<ManagedSummary, ApiError> {
    let (started_at, ended_at) = session(db, project_id)?;
    let limit_cents: Option<i64> = db.conn().query_row(
        "SELECT usage_limit_cents FROM projects WHERE id=?1",
        [project_id],
        |r| r.get(0),
    )?;
    let spent_mc = crate::usage::spent_mc(db, project_id)?;
    let (session_spent_mc, unknown) = match &started_at {
        None => (0, 0),
        Some(start) => db.conn().query_row(
            "SELECT COALESCE(SUM(cost_millicents),0), COALESCE(SUM(cost_known=0),0) FROM usage
             WHERE project_id=?1 AND created_at>=?2 AND (?3 IS NULL OR created_at<=?3)",
            rusqlite::params![project_id, start, ended_at],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?,
    };
    Ok(ManagedSummary {
        started_at,
        ended_at,
        limit_cents,
        spent_mc,
        session_spent_mc,
        cost_unknown: unknown > 0,
    })
}

/// 报告导出的上限：报告是二十条决定加外部操作，远小于此；超出说明调用方传错了内容。
const EXPORT_MAX_BYTES: usize = 1 << 20;

/// 写出 UI 生成的托管报告 Markdown（界面文案走 i18n，所以正文由 UI 按当前语言生成）。路径来自负责人在
/// 保存对话框里选的位置（与 export_events 同口径）；只接受 `.md`，先写临时文件再改名，失败不留半截文件。
pub fn export_managed_report(path: &Path, markdown: &str) -> Result<(), ApiError> {
    let invalid = |why: &str| ApiError::BadInput(format!("managed_report_export: {why}"));
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return Err(invalid("not_markdown"));
    }
    if markdown.len() > EXPORT_MAX_BYTES {
        return Err(invalid("too_large"));
    }
    let parent = path
        .parent()
        .filter(|p| p.is_dir())
        .ok_or_else(|| invalid("no_directory"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(markdown.as_bytes())?;
    tmp.persist(path).map_err(|e| ApiError::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod report_tests;
