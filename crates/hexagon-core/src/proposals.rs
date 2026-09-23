//! 改进提案管道（票 18 · Harness-RSI）：受控、可审计、可回滚的自我改进。
//!
//! - 提案 = parse 档产物（kind=改进提案），头带 `surface`/`target`，正文必备
//!   动机/改动面/预期收益/验证方法 + ```diff 块；走队列不走聊天。
//! - **生效面白名单**：skill（skills/**）、pack_copy（.hexagon/pack*.json）、
//!   agents_md（AGENTS.md）。**禁止面**：角色定义/权限规则/授权名单/工作台
//!   约束/.git/库文件——submit 即拒；技能 diff 含 mcp:/permission/grant
//!   字样即拒（技能不授予 MCP、不得扩权）。
//! - 生效路径：上级复审 → 负责人盖章 → 生效。提案者无上级（或上级缺席）
//!   直达负责人。同 Agent 同生效面在途上限 1。
//! - 生效 = 版本化快照 + git apply diff；rollback 一键还原快照。
//!   包副本改动下次运行生效（版本钉住，不热改运行中实体）。
//! - 盖章卡显著标注：扩大自治面（新增回填边等）/ 削弱复审者的提案。

use rusqlite::params;
use serde_json::{json, Value};

use crate::db::Db;
use crate::tools::ToolContext;
use crate::trace::EventKind;

#[derive(Debug, thiserror::Error)]
pub enum PropError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("db: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("git: {0}")]
    Git(#[from] crate::git::GitError),
    #[error("proposal rejected: {0}")]
    Rejected(String),
    #[error("proposal not found: {0}")]
    NotFound(String),
    #[error("bad proposal state: {0}")]
    BadState(String),
}

/// surface → 允许的目标根。禁区的判定 = 根白名单 + 危险段黑名单双查。
fn allowed_root(surface: &str) -> Option<&'static str> {
    match surface {
        "skill" => Some("skills/"),
        "pack_copy" => Some(".hexagon/pack"),
        "agents_md" => Some("AGENTS.md"),
        _ => None,
    }
}

const FORBIDDEN_SEGMENTS: &[&str] = &[
    ".git",
    "permission",
    "grant",
    "role",
    "constraint",
    "state.db",
    "prices.json",
];

fn forbidden_target(target: &str) -> Option<&'static str> {
    let t = target.to_lowercase();
    FORBIDDEN_SEGMENTS.iter().find(|s| t.contains(**s)).copied()
}

/// 提案正文的必备 `## ` 节。
const REQUIRED_SECTIONS: &[&str] = &["动机", "改动面", "预期收益", "验证方法"];

/// 上级映射：复审者 = 声明上级；无上级/上级缺席 → 直达负责人。
/// v1 静态表：技术线归架构师，其余（含架构师本人）直达负责人。
/// 内置上级映射（预置底稿的兜底；项目 role_defs 行优先，见 roles::superior_of）。
pub fn builtin_superior(role: &str) -> Option<&'static str> {
    match role {
        "前端" | "后端" | "QA" | "UI" | "UX" | "运维" => Some("架构师"),
        _ => None,
    }
}

/// 从产物内容解析 (surface, target, diff)。
pub(crate) fn parse_proposal(content: &str) -> Result<(String, String, String), PropError> {
    let (meta, body) = crate::artifacts::parse_header(content)
        .ok_or_else(|| PropError::Rejected("missing header".into()))?;
    let surface = meta
        .extra
        .get("surface")
        .cloned()
        .ok_or_else(|| PropError::Rejected("missing surface".into()))?;
    let target = meta
        .extra
        .get("target")
        .cloned()
        .ok_or_else(|| PropError::Rejected("missing target".into()))?;
    for sec in REQUIRED_SECTIONS {
        let needle = format!("## {sec}");
        if !body
            .lines()
            .any(|l| l.trim_start() == needle || l.trim_start().starts_with(&format!("{needle} ")))
        {
            return Err(PropError::Rejected(format!("missing section: {sec}")));
        }
    }
    // 提取 ```diff 块
    let diff = extract_fenced(body, "diff")
        .ok_or_else(|| PropError::Rejected("missing ```diff block".into()))?;
    Ok((surface, target, diff))
}

fn extract_fenced(body: &str, lang: &str) -> Option<String> {
    let open = format!("```{lang}");
    let start = body.find(&open)?;
    let rest = &body[start + open.len()..];
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

/// 回放证据块（rsi-research 票 06）：提案「验证方法」节里的
/// `replay` 语言围栏 JSON——版本化 ReplayReport 序列化体。
/// 证据不参与生效面校验（授权仍是盖章的事）；它只回答
/// 「凭什么信这改动」——畸形证据按 Rejected fail-closed,不许
/// 「看起来像有证据」的脏数据混进盖章卡。
pub fn attach_replay_evidence(body: &str, report: &crate::replay::ReplayReport) -> String {
    let block = format!(
        "\n```replay\n{}\n```\n",
        serde_json::to_string_pretty(report).unwrap_or_default()
    );
    // 插进「验证方法」节末尾（下一个 ## 节前）
    if let Some(pos) = body.find("## 验证方法") {
        let rest = &body[pos..];
        if let Some(next) = rest[2..].find("\n## ") {
            let at = pos + 2 + next;
            return format!("{}{}{}", &body[..at], block, &body[at..]);
        }
        return format!("{body}{block}");
    }
    format!("{body}\n## 验证方法\n{block}")
}

/// 从提案正文提取 judge 判定块（票 10：policy-dev 提案必附）。
/// ```judge {verdict,rationale,backend} ```——机械判定产物;
/// verdict 词表外的块视为畸形证据。
pub(crate) fn judge_evidence(body: &str) -> Option<Result<Value, String>> {
    extract_fenced(body, "judge").map(|raw| {
        let v: Value =
            serde_json::from_str(&raw).map_err(|e| format!("malformed judge block: {e}"))?;
        match v["verdict"].as_str() {
            Some("stamp") | Some("reject") | Some("needs-human") => Ok(v),
            _ => Err("judge verdict outside closed set".into()),
        }
    })
}

/// 从提案正文提取回放证据：无块 → None；有块 → 解析结果。
/// 调用方把 Err 映射成 Rejected——脏证据不许过。
pub(crate) fn replay_evidence(body: &str) -> Option<Result<crate::replay::ReplayReport, String>> {
    extract_fenced(body, "replay").map(|raw| {
        serde_json::from_str::<crate::replay::ReplayReport>(&raw)
            .map_err(|e| format!("malformed replay evidence: {e}"))
            .and_then(|r| {
                if r.schema != crate::replay::REPLAY_SCHEMA {
                    Err(format!(
                        "replay schema {} != supported {}",
                        r.schema,
                        crate::replay::REPLAY_SCHEMA
                    ))
                } else {
                    Ok(r)
                }
            })
    })
}

/// 证据摘要：进事件载荷与盖章卡的轻量形（不带全量指标）。
fn evidence_summary(r: &crate::replay::ReplayReport) -> Value {
    json!({
        "kind": "replay",
        "schema": r.schema,
        "scenario": r.scenario_fingerprint,
        "baseline_pack": r.baseline_pack,
        "candidate_pack": r.candidate_pack,
        "signals": r.signals.len(),
        "decision_diffs": r.decision_diffs.len(),
    })
}

/// 风险标注：扩大自治面（回填边/戳章点改动）与削弱复审者。
pub(crate) fn risk_flags(surface: &str, diff: &str) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if surface == "pack_copy"
        && (diff.contains("backfill_edges")
            || diff.contains("stamp_point")
            || diff.contains("autonomy"))
    {
        flags.push("expands_autonomy");
    }
    let removes_review = diff
        .lines()
        .any(|l| l.starts_with('-') && (l.contains("review") || l.contains("reviewer")));
    if removes_review {
        flags.push("weakens_review");
    }
    flags
}

/// 提交提案：校验 → 节流 → 入队 → 自动路由上级/直达负责人。
/// 返回 proposal id；校验不过抛 Rejected（产物本身已被交付，但提案不入队）。
pub fn submit(
    db: &Db,
    ctx: &ToolContext,
    artifact_id: &str,
    content: &str,
) -> Result<String, PropError> {
    let (surface, target, diff) = parse_proposal(content)?;

    // 回放证据（票 06）：存在即须可解析——脏证据视同坏提案拒收。
    let evidence = match replay_evidence(content) {
        Some(Ok(r)) => Some(evidence_summary(&r)),
        Some(Err(e)) => return Err(PropError::Rejected(e)),
        None => None,
    };

    // 票 10 强制：流程优化作者的提案必附回放证据 + judge 判定块——
    // 「缺附件不可提交」。作者身份从 agents 表查，不靠正文自述。
    let author_role: Option<String> = db
        .conn()
        .query_row(
            "SELECT role FROM agents WHERE id=?1 AND project_id=?2",
            params![ctx.agent_id, ctx.project_id],
            |r| r.get(0),
        )
        .ok();
    if author_role.as_deref() == Some(crate::policydev::ROLE) {
        if evidence.is_none() {
            return Err(PropError::Rejected(
                format!("{}提案必须附 ```replay 回放证据块", crate::policydev::ROLE),
            ));
        }
        match judge_evidence(content) {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(PropError::Rejected(e)),
            None => {
                return Err(PropError::Rejected(format!(
                    "{}提案必须附 ```judge 判定块",
                    crate::policydev::ROLE
                )))
            }
        }
    }

    // 生效面白名单 + 禁区双查
    let root = allowed_root(&surface)
        .ok_or_else(|| PropError::Rejected(format!("surface not in whitelist: {surface}")))?;
    if !target.starts_with(root) {
        return Err(PropError::Rejected(format!(
            "target outside surface root: {target} !~ {root}"
        )));
    }
    if let Some(seg) = forbidden_target(&target) {
        return Err(PropError::Rejected(format!(
            "forbidden target segment '{seg}' in {target}"
        )));
    }
    // 技能 diff 不得引入 MCP 授权 / 权限扩大
    if surface == "skill" {
        for tok in ["mcp:", "permission", "grant", "allow "] {
            if diff.to_lowercase().contains(tok) {
                return Err(PropError::Rejected(format!(
                    "skill diff must not introduce '{tok}'"
                )));
            }
        }
    }

    // 节流：同 Agent 同生效面在途上限 1
    let inflight: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM proposals
         WHERE project_id=?1 AND author_agent_id=?2 AND surface=?3
           AND status IN ('queued','in_review','awaiting_stamp')",
        params![ctx.project_id, ctx.agent_id, surface],
        |r| r.get(0),
    )?;
    if inflight > 0 {
        return Err(PropError::Rejected(
            "in-flight cap: one proposal per surface per agent".into(),
        ));
    }

    let pid = format!("prop{}", db.next_id("prop")?);
    db.conn().execute(
        "INSERT INTO proposals (id, project_id, author_agent_id, surface, artifact_id,
                                effective_path, status)
         VALUES (?1,?2,?3,?4,?5,?6,'queued')",
        params![
            pid,
            ctx.project_id,
            ctx.agent_id,
            surface,
            artifact_id,
            target
        ],
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalQueued,
        json!({"proposal_id": pid, "surface": surface, "target": target,
               "evidence": evidence}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    log::info!("proposal queued: {pid} surface={surface} target={target}");

    // 路由：上级在场 → in_review；否则直达负责人（awaiting_stamp + 盖章卡）
    let author_role: String = db.conn().query_row(
        "SELECT role FROM agents WHERE id=?1",
        [&ctx.agent_id],
        |r| r.get(0),
    )?;
    let reviewer = crate::roles::superior_of(db, &ctx.project_id, &author_role).and_then(|r| {
        db.conn()
            .query_row(
                "SELECT id FROM agents WHERE project_id=?1 AND role=?2",
                params![ctx.project_id, r],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .map(|aid| (r, aid))
    });
    match reviewer {
        Some((role, aid)) => {
            db.conn().execute(
                "UPDATE proposals SET status='in_review' WHERE id=?1",
                [&pid],
            )?;
            db.append_event(
                &ctx.project_id,
                EventKind::System,
                json!({"note": "proposal_in_review", "proposal_id": pid,
                       "reviewer_role": role, "reviewer_agent": aid}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
        }
        // 无上级（或上级缺席）时负责人是唯一复审者。L4 只自动通过
        // 「上级已经通过」之后的那一关，这里还没有复审结论，不放行。
        // 被否决：无上级也直接生效——那是一次没人看过的改动。
        None => to_stamp_queue(db, ctx, &pid, &diff, &surface, evidence.as_ref())?,
    }
    Ok(pid)
}

/// in_review → awaiting_stamp + 盖章卡（带风险标注）。
fn to_stamp_queue(
    db: &Db,
    ctx: &ToolContext,
    pid: &str,
    diff: &str,
    surface: &str,
    evidence: Option<&Value>,
) -> Result<(), PropError> {
    db.conn().execute(
        "UPDATE proposals SET status='awaiting_stamp' WHERE id=?1",
        [pid],
    )?;
    let flags = risk_flags(surface, diff);
    // 提案确认卡搭 kind='stamp'（卡种过载见 cards.rs 模块注记）
    crate::cards::enqueue(
        db,
        &ctx.project_id,
        Some(&ctx.agent_id),
        crate::cards::CardKind::Stamp,
        json!({"proposal_id": pid, "surface": surface,
        "evidence": evidence,
        "warnings": flags,
        "warning_text": if flags.is_empty() { Value::Null } else {
            json!(format!("此提案{}", flags.join(" + ")))
        }}),
        None,
    )?;
    Ok(())
}

/// 上级复审结论：pass → awaiting_stamp；reject → rejected + 原因。
pub fn review(
    db: &Db,
    ctx: &ToolContext,
    proposal_id: &str,
    pass: bool,
    reason: &str,
) -> Result<(), PropError> {
    let (status, artifact_id): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT status, artifact_id FROM proposals WHERE id=?1 AND project_id=?2",
            params![proposal_id, ctx.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| PropError::NotFound(proposal_id.into()))?;
    if status != "in_review" {
        return Err(PropError::BadState(status));
    }
    if !pass {
        // 驳回不生效。L4 也不把驳回翻成通过。
        db.conn().execute(
            "UPDATE proposals SET status='rejected', decided_at=datetime('now') WHERE id=?1",
            [proposal_id],
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalRejected,
            json!({"proposal_id": proposal_id, "pass": false, "reason": reason}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        return Ok(());
    }
    // 读产物内容取 diff/surface 做风险标注
    let (surface, _target, diff, body) = artifact_proposal_parts(db, ctx, artifact_id.as_deref())?;
    let evidence = replay_evidence(&body)
        .and_then(|r| r.ok())
        .map(|r| evidence_summary(&r));
    // 读档失败按等人。不把一次查询故障升成未审提案生效。
    let rank = crate::autonomy::rank(db, &ctx.project_id).unwrap_or(0);
    if crate::harnessgate::auto_passes(rank, crate::harnessgate::HarnessAction::ProposalOwnerStamp)
    {
        // 上级已经通过。负责人这一关自动过，不入待决卡。
        // 先落盘再记轨迹：apply 失败则状态仍是 in_review，没有「已盖章但没生效」。
        let effective = materialize(db, ctx, proposal_id)?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalReviewed,
            json!({"proposal_id": proposal_id, "pass": true, "reason": reason}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalStamped,
            json!({"proposal_id": proposal_id, "by": "autonomy", "evidence": evidence}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        db.append_event(
            &ctx.project_id,
            EventKind::ProposalActivated,
            json!({"proposal_id": proposal_id, "effective_path": effective, "by": "autonomy"}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        return Ok(());
    }
    to_stamp_queue(db, ctx, proposal_id, &diff, &surface, evidence.as_ref())?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalReviewed,
        json!({"proposal_id": proposal_id, "pass": true, "reason": reason}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(())
}

fn artifact_proposal_parts(
    db: &Db,
    ctx: &ToolContext,
    artifact_id: Option<&str>,
) -> Result<(String, String, String, String), PropError> {
    let path: String = db
        .conn()
        .query_row(
            "SELECT path FROM artifacts WHERE id=?1",
            [artifact_id.unwrap_or("")],
            |r| r.get(0),
        )
        .map_err(|_| PropError::NotFound(format!("artifact {artifact_id:?}")))?;
    let content = std::fs::read_to_string(ctx.repo_root.join(".hexagon").join(&path))?;
    let (s, t, d) = parse_proposal(&content)?;
    Ok((s, t, d, content))
}

/// 提案盖章驳回：qid 定位提案 → rejected + 原因 + 问题已答。与复审驳回同事件类。
pub fn reject_at_stamp(
    db: &Db,
    ctx: &ToolContext,
    qid: &str,
    reason: &str,
) -> Result<(), PropError> {
    let card = crate::cards::get_queued(db, qid, crate::cards::CardKind::Stamp)
        .ok()
        .filter(|c| c.project_id == ctx.project_id)
        .ok_or_else(|| PropError::NotFound(format!("question {qid}")))?;
    let pid = card.payload["proposal_id"]
        .as_str()
        .ok_or_else(|| PropError::Rejected("question is not a proposal stamp".into()))?
        .to_string();
    crate::cards::answer(db, qid, "owner")?;
    db.conn().execute(
        "UPDATE proposals SET status='rejected', decided_at=datetime('now') WHERE id=?1",
        [&pid],
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalRejected,
        json!({"proposal_id": pid, "pass": false, "reason": reason, "question_id": qid}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(())
}

/// 盖章确认：快照 → 应用 diff → active。qid 是 kind='stamp' 且 payload 带 proposal_id 的卡。
pub fn activate(db: &Db, ctx: &ToolContext, qid: &str) -> Result<String, PropError> {
    let card = crate::cards::get_queued(db, qid, crate::cards::CardKind::Stamp)
        .ok()
        .filter(|c| c.project_id == ctx.project_id)
        .ok_or_else(|| PropError::NotFound(format!("question {qid}")))?;
    let p = &card.payload;
    let pid = p["proposal_id"]
        .as_str()
        .ok_or_else(|| PropError::Rejected("question is not a proposal stamp".into()))?
        .to_string();
    crate::cards::answer(db, qid, "owner")?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalStamped,
        // by=owner 与 L4 自动通过的 by=autonomy 区分。
        json!({"proposal_id": pid, "question_id": qid, "by": "owner"}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;

    let target = materialize(db, ctx, &pid)?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalActivated,
        json!({"proposal_id": pid, "effective_path": target, "by": "owner"}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    log::info!("proposal activated: {pid} -> {target}");
    Ok(pid)
}

/// 版本化快照 → git apply diff → active。不写盖章事件。
/// 不热改运行中实体——pack 副本下次生效。
fn materialize(db: &Db, ctx: &ToolContext, pid: &str) -> Result<String, PropError> {
    let (artifact_id, target): (Option<String>, String) = db.conn().query_row(
        "SELECT artifact_id, effective_path FROM proposals WHERE id=?1 AND project_id=?2",
        params![pid, ctx.project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (_surface, _t, diff, _body) = artifact_proposal_parts(db, ctx, artifact_id.as_deref())?;
    let backup_dir = ctx.repo_root.join(".hexagon/proposals").join(pid);
    std::fs::create_dir_all(&backup_dir)?;
    let target_path = ctx.repo_root.join(&target);
    if target_path.exists() {
        std::fs::copy(&target_path, backup_dir.join("before")).ok();
    }
    if !crate::git::is_repo(&ctx.repo_root) {
        return Err(PropError::Rejected(
            "proposal activation requires git repo".into(),
        ));
    }
    std::fs::write(backup_dir.join("change.diff"), format!("{diff}\n"))?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&ctx.repo_root)
        .args(["apply", "--whitespace=nowarn"])
        .arg(backup_dir.join("change.diff"))
        .output()?;
    if !out.status.success() {
        return Err(PropError::Rejected(format!(
            "git apply failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    db.conn().execute(
        "UPDATE proposals SET status='active', decided_at=datetime('now') WHERE id=?1",
        [pid],
    )?;
    Ok(target)
}

/// 一键回滚：快照还原 → rolled_back。
pub fn rollback(db: &Db, ctx: &ToolContext, proposal_id: &str) -> Result<(), PropError> {
    let (status, target): (String, String) = db
        .conn()
        .query_row(
            "SELECT status, effective_path FROM proposals WHERE id=?1 AND project_id=?2",
            params![proposal_id, ctx.project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| PropError::NotFound(proposal_id.into()))?;
    if status != "active" {
        return Err(PropError::BadState(status));
    }
    let backup = ctx
        .repo_root
        .join(".hexagon/proposals")
        .join(proposal_id)
        .join("before");
    let target_path = ctx.repo_root.join(&target);
    if backup.exists() {
        std::fs::copy(&backup, &target_path)?;
    } else if target_path.exists() {
        std::fs::remove_file(&target_path)?; // 生效前不存在 = 回滚即删除
    }
    db.conn().execute(
        "UPDATE proposals SET status='rolled_back' WHERE id=?1",
        [proposal_id],
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::ProposalRolledBack,
        json!({"proposal_id": proposal_id, "restored": target}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    log::info!("proposal rolled back: {proposal_id}");
    Ok(())
}

/// 待审/在途提案队列（UI 提案卡数据源）。
/// 提案队列行（ADR 0054）：proposals×artifacts 联表读模型。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProposalRow {
    pub id: String,
    #[ts(type = "'skill' | 'pack_copy' | 'agents_md'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub surface: String,
    pub target: String,
    #[ts(
        type = "'queued' | 'in_review' | 'rejected' | 'awaiting_stamp' | 'active' | 'rolled_back'"
    )] // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub status: String,
    pub author: String,
    pub artifact_path: Option<String>,
}

pub fn list(db: &Db, project_id: &str) -> Result<Vec<ProposalRow>, PropError> {
    let mut st = db.conn().prepare(
        "SELECT p.id, p.surface, p.effective_path, p.status, p.author_agent_id, a.path
         FROM proposals p LEFT JOIN artifacts a ON a.id = p.artifact_id
         WHERE p.project_id=?1 ORDER BY p.created_at",
    )?;
    let rows = st
        .query_map([project_id], |r| {
            Ok(ProposalRow {
                id: r.get(0)?,
                surface: r.get(1)?,
                target: r.get(2)?,
                status: r.get(3)?,
                author: r.get(4)?,
                artifact_path: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolContext;

    fn proposal_md(surface: &str, target: &str, diff: &str) -> String {
        format!(
            "---\nkind: 改进提案\nauthor: a0\nsurface: {surface}\ntarget: {target}\n---\n\
             ## 动机\n改进提示词\n\n## 改动面\n```diff\n{diff}\n```\n\n## 预期收益\n更准\n\n## 验证方法\n跑测\n"
        )
    }

    /// git 仓 + 项目 + Agent；architect_in_team 控制架构师是否在场。
    fn setup(roles: &[&str]) -> (Db, tempfile::TempDir, ToolContext) {
        let dir = tempfile::tempdir().unwrap();
        crate::git::init(dir.path(), "main").unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "line1\n").unwrap();
        crate::git::commit_all(dir.path(), "seed").unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p',?1,'x','pack')",
                [dir.path().to_string_lossy().to_string()],
            )
            .unwrap();
        for (i, r) in roles.iter().enumerate() {
            db.conn()
                .execute(
                    "INSERT INTO agents (id, project_id, role, status) VALUES (?1,'p',?2,'active')",
                    params![format!("a{i}"), r],
                )
                .unwrap();
        }
        let ctx = ToolContext {
            project_id: "p".into(),
            agent_id: "a0".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: Default::default(),
            sessions: Default::default(),
            caps: Default::default(),
        };
        (db, dir, ctx)
    }

    /// 占位产物行 + 产物文件（proposals.artifact_id 有外键；生效路径要读文件）。
    fn mkart(db: &Db, dir: &std::path::Path, id: &str, content: &str) {
        let path = format!("props/{id}.md");
        db.conn()
            .execute(
                "INSERT INTO artifacts (id, project_id, path, kind, tier, author_agent_id, version)
                 VALUES (?1,'p',?2,'改进提案','parse','a0',1)",
                params![id, path],
            )
            .unwrap();
        let f = dir.join(".hexagon").join(&path);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, content).unwrap();
    }

    const DIFF: &str = "--- a/AGENTS.md\n+++ b/AGENTS.md\n@@ -1 +1,2 @@\n line1\n+line2";

    #[test]
    fn forbidden_surface_and_target_rejected() {
        let (db, _d, ctx) = setup(&["前端"]);
        // 生效面不在白名单
        let e = submit(&db, &ctx, "art-x", &proposal_md("role_def", "x", DIFF)).unwrap_err();
        assert!(e.to_string().contains("whitelist"));
        // 目标越面根
        let e = submit(&db, &ctx, "art-x", &proposal_md("skill", "AGENTS.md", DIFF)).unwrap_err();
        assert!(e.to_string().contains("outside surface root"));
        // 禁区段：权限规则文件
        let e = submit(
            &db,
            &ctx,
            "art-x",
            &proposal_md("pack_copy", ".hexagon/pack-permission.json", DIFF),
        )
        .unwrap_err();
        assert!(e.to_string().contains("forbidden"));
        // 技能 diff 引入 mcp 授权
        let e = submit(
            &db,
            &ctx,
            "art-x",
            &proposal_md("skill", "skills/x/SKILL.md", "+grant mcp:foo"),
        )
        .unwrap_err();
        assert!(e.to_string().contains("must not introduce"));
    }

    #[test]
    fn full_lifecycle_direct_to_owner_then_rollback() {
        let (db, dir, ctx) = setup(&["前端"]); // 无架构师 → 直达负责人
        let content = proposal_md("agents_md", "AGENTS.md", DIFF);
        mkart(&db, dir.path(), "art1", &content);
        let pid = submit(&db, &ctx, "art1", &content).unwrap();
        // 直达负责人：awaiting_stamp + 排队 stamp 卡
        let (status,): (String,) = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid], |r| {
                Ok((r.get(0)?,))
            })
            .unwrap();
        assert_eq!(status, "awaiting_stamp");
        let qid = crate::cards::first_queued(&db, "p", crate::cards::CardKind::Stamp)
            .unwrap()
            .unwrap();
        // 盖章 → 生效：文件被改
        activate(&db, &ctx, &qid).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            "line1\nline2\n"
        );
        let status: String = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "active");
        // 回滚：快照还原
        rollback(&db, &ctx, &pid).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            "line1\n"
        );
        let status: String = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "rolled_back");
    }

    #[test]
    fn review_path_and_inflight_cap() {
        let (db, d, ctx) = setup(&["前端", "前端技术负责人"]); // 上级在场（preset 链：前端→前端技术负责人）
        let c1 = proposal_md("agents_md", "AGENTS.md", DIFF);
        mkart(&db, d.path(), "art1", &c1);
        let pid = submit(&db, &ctx, "art1", &c1).unwrap();
        // 路由给前端技术负责人复审
        let status: String = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "in_review");
        // 节流：同 Agent 同面再提一条被拒
        let e = submit(
            &db,
            &ctx,
            "art-x",
            &proposal_md("agents_md", "AGENTS.md", DIFF),
        )
        .unwrap_err();
        assert!(e.to_string().contains("in-flight cap"));
        // 复审驳回带原因
        review(&db, &ctx, &pid, false, "方向不对").unwrap();
        let status: String = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "rejected");
        // 驳回后不再占在途额
        mkart(&db, d.path(), "art3", &c1);
        let pid2 = submit(&db, &ctx, "art3", &c1).unwrap();
        let status: String = db
            .conn()
            .query_row("SELECT status FROM proposals WHERE id=?1", [&pid2], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "in_review");
    }

    #[test]
    fn pack_copy_autonomy_flag_marked_on_stamp_card() {
        let (db, d, ctx) = setup(&["前端"]);
        let diff = "--- a/.hexagon/pack.json\n+++ b/.hexagon/pack.json\n@@ -1 +1,2 @@\n x\n+\"backfill_edges\": []";
        let content = proposal_md("pack_copy", ".hexagon/pack.json", diff);
        mkart(&db, d.path(), "art1", &content);
        let pid = submit(&db, &ctx, "art1", &content).unwrap();
        let qid = crate::cards::first_queued(&db, "p", crate::cards::CardKind::Stamp)
            .unwrap()
            .unwrap();
        let card = crate::cards::get(&db, &qid).unwrap();
        assert!(card.payload.to_string().contains("expands_autonomy"));
        let _ = pid;
    }

    #[test]
    fn replay_evidence_roundtrip_and_reject_malformed() {
        let r = crate::replay::ReplayReport {
            schema: crate::replay::REPLAY_SCHEMA,
            scenario_fingerprint: "abc".into(),
            baseline_pack: "t@v1".into(),
            candidate_pack: "t@v2".into(),
            policy_diff: vec![],
            non_policy_changes: vec![],
            baseline: Default::default(),
            candidate: Default::default(),
            decision_diffs: vec![],
            signals: vec![],
        };
        let body = "## 动机\nm\n## 改动面\nc\n## 预期收益\nb\n## 验证方法\nv\n";
        let with = attach_replay_evidence(body, &r);
        let parsed = replay_evidence(&with).expect("block").expect("parse");
        assert_eq!(parsed.candidate_pack, "t@v2");
        // 脏证据：块在但 JSON 坏 → Err（submit 侧据此 Rejected）
        let bad = "## 验证方法\n```replay\n{not json}\n```\n";
        assert!(matches!(replay_evidence(bad), Some(Err(_))));
        // 无块 → None
        assert!(replay_evidence(body).is_none());
        // schema 不符 → Err
        let mut r2 = r.clone();
        r2.schema = 999;
        let with2 = attach_replay_evidence(body, &r2);
        assert!(matches!(replay_evidence(&with2), Some(Err(_))));
    }
}
