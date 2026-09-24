//! 复审与打回：裁决回路。
//!
//! - 复审：复审意见产物（parse 档）→ pass/reject；reject = 本阶段返工不退阶段。
//!   复审不实现、不盖章、不自行推进——它只是阶段成功判定的输入。
//! - 打回：结构化异议（目标产物+小节+理由）→ 路由给该产物的声明复审者裁决。
//!   同意 → 指针拨回产物所在阶段、产出 Agent 重激活（打回内容进其简报）。
//!   驳回 → 本阶段继续。
//! - 升级（直给负责人）：产物已盖章 / 复审者缺席 / 同 Agent 对同产物第 2 次打回。
//! - 自动路径：声明回填边命中且自治 ≥L1 → 无需等复审者，直接拨回。
//! - 盖章点驳回：非最终盖章点退上一阶段。最终验收必须点名阶段并写修改意见，
//!   只重开那一阶段（票 02）。与打回是两个通道，事件分开。

use crate::artifacts::{self, TierMap};
use crate::db::Db;
use crate::orchestra::{self, OrchError, PackDef};
use crate::tools::ToolContext;
use crate::trace::{EventKind, TraceError};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error(transparent)]
    Artifact(#[from] Box<artifacts::ArtifactError>),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error(transparent)]
    Orch(#[from] OrchError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("artifact not found: {0}")]
    NoArtifact(String),
    #[error("flag not found: {0}")]
    NoFlag(String),
    /// 最终验收退回缺阶段/修改意见，或退回的不是最后一道盖章点。
    #[error("{0}")]
    BadReject(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Reject,
}
impl Verdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum FlagRoute {
    /// 路由给复审者裁决
    ToReviewer { reviewer_role: String },
    /// 升级负责人（pending_questions kind=escalation）
    Escalated { reason: String, question_id: String },
    /// 回填边命中 + L1+：自动同意拨回
    AutoAdjudicated { to_seq: i64 },
    /// L2 协调自治：复审者被唤醒直接裁决，不等负责人
    AutoWoken {
        reviewer_role: String,
        agent_id: String,
    },
}

fn role_of(db: &Db, agent_id: &str) -> Result<String, ReviewError> {
    Ok(db
        .conn()
        .query_row("SELECT role FROM agents WHERE id=?1", [agent_id], |r| {
            r.get(0)
        })?)
}

fn autonomy_rank(db: &Db, project_id: &str) -> Result<u8, ReviewError> {
    // 执行档，不是存储档。票 01：L3/L4 在此封顶为 2，决策门上的数字同 L2。
    Ok(crate::autonomy::execution_rank(db, project_id)?)
}

/// 打回路由决策的落盘形状（rsi-research 票 01）：每次路由记
/// eligible（声明结构允许的路由集）/chosen/alternatives/gates（裁决输入），
/// 回放可按「当时可选集」diff 决策而非只看去向。
fn route_decision(chosen: &str, eligible: &[&str], gates: Value) -> Value {
    json!({
        "kind": "flag_route",
        "eligible": eligible,
        "chosen": chosen,
        "alternatives": eligible
            .iter()
            .filter(|r| **r != chosen)
            .map(|r| json!({"option": r, "reason": "not_chosen"}))
            .collect::<Vec<_>>(),
        "gates": gates,
    })
}

/// 升级（直给负责人）：code 是闭集理由（票 02），reason 是给人看的文本。
/// 路由被迫收敛——eligible 只剩 escalated。
fn escalate(
    db: &Db,
    ctx: &ToolContext,
    code: crate::trace::FailureCode,
    reason: &str,
    flag_payload: Value,
) -> Result<FlagRoute, ReviewError> {
    // 卡表写口归 cards.rs（arch-review 票 04）
    let qid = crate::cards::enqueue(
        db,
        &ctx.project_id,
        Some(&ctx.agent_id),
        crate::cards::CardKind::Escalation,
        flag_payload.clone(),
        None,
    )?;
    db.append_event(
        &ctx.project_id,
        EventKind::Escalated,
        json!({"reason": reason, "code": code.as_str(), "question_id": qid,
               "flag": flag_payload,
               "decision_kind": "flag_route",
               "decision": route_decision("escalated", &["escalated"], json!({}))}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(FlagRoute::Escalated {
        reason: reason.into(),
        question_id: qid,
    })
}

/// 提交复审：复审意见产物 + ReviewPassed/ReviewRejected 事件。
/// `verdict` 写进产物头，事件带 artifact_kind 供阶段判定消费。
pub fn submit_review(
    db: &Db,
    ctx: &ToolContext,
    target_artifact_id: &str,
    verdict: Verdict,
    body: &str,
) -> Result<String, ReviewError> {
    let (tpath, tkind, trun): (String, String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT path, kind, stage_run_id FROM artifacts WHERE id=?1",
            [target_artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| ReviewError::NoArtifact(target_artifact_id.into()))?;
    let reviewer = role_of(db, &ctx.agent_id)?;
    let content = format!(
        "---\nkind: 复审意见\nauthor: {}\ntarget: {}\nverdict: {}\n---\n{}",
        ctx.agent_id,
        tpath,
        verdict.as_str(),
        body
    );
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*)+1 FROM artifacts WHERE project_id=?1 AND kind='复审意见'",
        [&ctx.project_id],
        |r| r.get(0),
    )?;
    let path = format!("reviews/{}-{}.md", tpath.replace('/', "_"), n);
    let aid =
        artifacts::deliver(db, ctx, &TierMap::new(), &path, &content, None).map_err(Box::new)?;
    db.append_event(
        &ctx.project_id,
        match verdict {
            Verdict::Pass => EventKind::ReviewPassed,
            Verdict::Reject => EventKind::ReviewRejected,
        },
        json!({"artifact_kind": tkind, "artifact_id": target_artifact_id,
               "artifact_path": tpath, "reviewer": reviewer,
               "review_artifact": aid, "stage_run_id": trun}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref().or(trun.as_deref()),
    )?;
    Ok(aid)
}

/// 提交打回：登记打回产物 + 路由（复审者 / 自动 / 升级）。
pub fn submit_flag(
    db: &Db,
    ctx: &ToolContext,
    pack: &PackDef,
    target_path: &str,
    section: &str,
    reason: &str,
) -> Result<FlagRoute, ReviewError> {
    // 目标产物（最新版）
    let (_art_id, art_kind, art_status, art_run, art_author): (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = db
        .conn()
        .query_row(
            "SELECT id, kind, status, stage_run_id, author_agent_id FROM artifacts
             WHERE project_id=?1 AND path=?2 ORDER BY version DESC LIMIT 1",
            rusqlite::params![ctx.project_id, target_path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|_| ReviewError::NoArtifact(target_path.into()))?;

    let flagger = role_of(db, &ctx.agent_id)?;
    let content = format!(
        "---\nkind: 打回\nauthor: {}\ntarget: {}\nsection: {}\n---\n{}",
        ctx.agent_id, target_path, section, reason
    );
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*)+1 FROM artifacts WHERE project_id=?1 AND kind='打回'",
        [&ctx.project_id],
        |r| r.get(0),
    )?;
    let flag_path = format!("flags/flag-{n}.md");
    let flag_id = artifacts::deliver(db, ctx, &TierMap::new(), &flag_path, &content, None)
        .map_err(Box::new)?;

    db.append_event(
        &ctx.project_id,
        EventKind::FlagSubmitted,
        json!({"flag_id": flag_id, "flagger": flagger, "target": target_path,
               "target_kind": art_kind, "section": section, "reason": reason}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;

    // ---- 升级判定 ----
    if art_status == "stamped" {
        return escalate(
            db,
            ctx,
            crate::trace::FailureCode::HardBlocked,
            "target artifact already stamped",
            json!({"flag_id": flag_id, "target": target_path}),
        );
    }
    // 找声明复审者：全包搜 reviews 声明（复审可声明在后续阶段，如「接口」阶段复审「界面稿」）
    let reviewer_role: Option<String> = pack.stages.iter().find_map(|st| {
        st.reviews
            .iter()
            .find(|r| r.artifact_kind == art_kind)
            .map(|r| r.reviewer.clone())
    });
    match reviewer_role {
        None => {
            return escalate(
                db,
                ctx,
                crate::trace::FailureCode::Ambiguous,
                "no declared reviewer for artifact kind",
                json!({"flag_id": flag_id, "target": target_path}),
            );
        }
        Some(ref rr) => {
            let present: bool = db.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM agents WHERE project_id=?1 AND role=?2)",
                rusqlite::params![ctx.project_id, rr],
                |r| r.get(0),
            )?;
            if !present {
                return escalate(
                    db,
                    ctx,
                    crate::trace::FailureCode::Ambiguous,
                    "declared reviewer absent from team",
                    json!({"flag_id": flag_id, "target": target_path}),
                );
            }
        }
    }
    // 重复打回：同 Agent 对同产物第 2 次起 → 升级
    // （阈值可被 pack.knobs.flag_patience 覆盖——票 04 策略旋钮）
    let prior: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM events WHERE project_id=?1 AND kind='flag_submitted'
         AND json_extract(payload,'$.flagger')=?2 AND json_extract(payload,'$.target')=?3",
        rusqlite::params![ctx.project_id, flagger, target_path],
        |r| r.get(0),
    )?;
    if prior >= pack.knobs.flag_patience() as i64 {
        return escalate(
            db,
            ctx,
            crate::trace::FailureCode::Ambiguous,
            "repeat flag on same artifact",
            json!({"flag_id": flag_id, "target": target_path}),
        );
    }

    // ---- 自动路径：回填边命中 + 自治 ≥L1 + 旋钮未关 ----
    let author_role = art_author.as_deref().and_then(|a| role_of(db, a).ok());
    let cur_run = ctx.stage_run_id.as_deref().and_then(|rid| {
        db.conn()
            .query_row("SELECT seq FROM stage_runs WHERE id=?1", [rid], |r| {
                r.get::<_, i64>(0)
            })
            .ok()
    });
    let edge_hit = cur_run
        .and_then(|seq| pack.stages.get(seq as usize))
        .map(|st| {
            st.backfill_edges
                .iter()
                .any(|(f, t)| f == &flagger && Some(t) == author_role.as_ref())
        })
        .unwrap_or(false);
    let rank = autonomy_rank(db, &ctx.project_id)?;
    // 路由决策的裁决输入：闸门事实原样落盘（票 01）——回放可比对的
    // 是这些布尔量，不是「路由叫啥」的自由文本。
    let gates = json!({
        "edge_hit": edge_hit,
        "autonomy": rank,
        "prior_flags": prior,
        "auto_backfill_enabled": pack.knobs.auto_backfill(),
        "consult_auto_wake_enabled": pack.knobs.consult_auto_wake(),
    });
    let mut eligible: Vec<&str> = vec!["to_reviewer"];
    if edge_hit {
        eligible.push("auto_backfill");
    }
    if edge_hit && rank >= 1 && pack.knobs.auto_backfill() {
        let to_seq = art_run
            .as_deref()
            .and_then(|rid| {
                db.conn()
                    .query_row("SELECT seq FROM stage_runs WHERE id=?1", [rid], |r| {
                        r.get(0)
                    })
                    .ok()
            })
            .unwrap_or(0);
        // 回填边命中即拨回——BackfillExecuted 落盘（事件类早已声明，
        // 此前无发射点）；decision 记 eligible/chosen/gates。
        db.append_event(
            &ctx.project_id,
            EventKind::BackfillExecuted,
            json!({"flag_id": flag_id, "flagger": flagger, "target": target_path,
                   "to_seq": to_seq, "code": crate::trace::FailureCode::Repairable.as_str(),
                   "decision_kind": "flag_route",
                   "decision": route_decision("auto_backfill", &eligible, gates.clone())}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        adjudicate_flag(db, ctx, pack, &flag_id, true)?;
        return Ok(FlagRoute::AutoAdjudicated { to_seq });
    }

    // L2 协调自治：打回路由裁决自动跑——复审者直接唤醒进入裁决，
    // 不等负责人（盖章点/安全网/新权限不受影响，照常在别处排队）。
    let reviewer = reviewer_role.unwrap();
    eligible.push("auto_wake");
    if rank >= 2 && pack.knobs.consult_auto_wake() {
        if let Ok(agent_id) = db.conn().query_row(
            "SELECT id FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id LIMIT 1",
            rusqlite::params![ctx.project_id, reviewer],
            |r| r.get::<_, String>(0),
        ) {
            crate::orchestra::write_agent_status(db, &ctx.project_id, &agent_id, false)?;
            db.append_event(
                &ctx.project_id,
                EventKind::ConsultWakeup,
                json!({"agent": agent_id, "role": reviewer, "reason": "flag adjudication",
                       "flag_id": flag_id, "code": crate::trace::FailureCode::Repairable.as_str(),
                       "decision_kind": "flag_route",
                       "decision": route_decision("auto_wake", &eligible, gates.clone())}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            return Ok(FlagRoute::AutoWoken {
                reviewer_role: reviewer,
                agent_id,
            });
        }
    }

    // 默认路由：交复审者排队裁决——唯一没有专属领域事件的路由,
    // 落 System{kind:"flag_routed"} 补齐决策留痕。
    db.append_event(
        &ctx.project_id,
        EventKind::System,
        json!({"kind": "flag_routed", "flag_id": flag_id, "target": target_path,
               "reviewer": reviewer, "code": crate::trace::FailureCode::Repairable.as_str(),
               "decision_kind": "flag_route",
               "decision": route_decision("to_reviewer", &eligible, gates)}),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    )?;
    Ok(FlagRoute::ToReviewer {
        reviewer_role: reviewer,
    })
}

/// 裁决打回：同意 → 指针拨回产物所在阶段（产出 Agent 经激活名单重激活，
/// 打回内容进其简报的 notices）；驳回 → 本阶段继续。
/// 打回裁决回执（ADR 0054）：serde(tag="adjudicated") 标号联合。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "adjudicated", rename_all = "snake_case")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum FlagOutcome {
    /// 同意：回退到目标产物所属阶段重跑。
    Agreed { rewind: orchestra::StageAction },
    /// 驳回：仅留痕，本阶段继续。
    Rejected,
}

pub fn adjudicate_flag(
    db: &Db,
    ctx: &ToolContext,
    pack: &PackDef,
    flag_id: &str,
    agree: bool,
) -> Result<FlagOutcome, ReviewError> {
    // flag 产物 → 目标产物 → 目标阶段 seq
    let target: String = db
        .conn()
        .query_row(
            "SELECT json_extract(
                (SELECT payload FROM events WHERE json_extract(payload,'$.flag_id')=?1
                  AND kind='flag_submitted' LIMIT 1), '$.target')",
            [flag_id],
            |r| r.get(0),
        )
        .map_err(|_| ReviewError::NoFlag(flag_id.into()))?;
    let art_run: Option<String> = db
        .conn()
        .query_row(
            "SELECT stage_run_id FROM artifacts WHERE project_id=?1 AND path=?2
             ORDER BY version DESC LIMIT 1",
            rusqlite::params![ctx.project_id, target],
            |r| r.get(0),
        )
        .map_err(|_| ReviewError::NoArtifact(target.clone()))?;

    // 打回裁决决策点（票 01）：同意/驳回连同可选集落盘。
    let adjudication = |chosen: &str| {
        json!({"kind": "flag_adjudication",
               "eligible": ["agree", "reject"],
               "chosen": chosen,
               "alternatives": [{"option": if chosen == "agree" { "reject" } else { "agree" },
                                  "reason": "not_chosen"}]})
    };
    if agree {
        let to_seq: i64 = art_run
            .as_deref()
            .and_then(|rid| {
                db.conn()
                    .query_row("SELECT seq FROM stage_runs WHERE id=?1", [rid], |r| {
                        r.get(0)
                    })
                    .ok()
            })
            .unwrap_or(0);
        db.append_event(
            &ctx.project_id,
            EventKind::FlagAdjudicated,
            json!({"flag_id": flag_id, "agree": true, "to_seq": to_seq, "target": target,
                   "decision_kind": "adjudication",
                   "decision": adjudication("agree")}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        let r = orchestra::rewind(db, &ctx.project_id, pack, to_seq as usize)?;
        Ok(FlagOutcome::Agreed { rewind: r })
    } else {
        db.append_event(
            &ctx.project_id,
            EventKind::FlagAdjudicated,
            json!({"flag_id": flag_id, "agree": false, "target": target,
                   "decision_kind": "adjudication",
                   "decision": adjudication("reject")}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        Ok(FlagOutcome::Rejected)
    }
}

fn waiting_stamp(db: &Db, project_id: &str) -> Result<(String, i64, String), ReviewError> {
    db.conn()
        .query_row(
            "SELECT id, seq, stage_name FROM stage_runs
             WHERE project_id=?1 AND state='waiting_stamp' ORDER BY seq DESC LIMIT 1",
            [project_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| ReviewError::Orch(OrchError::NoActiveStage(project_id.into())))
}

fn stamp_flags(pack: &PackDef) -> Vec<bool> {
    pack.stages.iter().map(|s| s.stamp_point).collect()
}

/// 盖章点驳回：非最终盖章点退上一阶段——与打回不同通道（事件分开）。
/// 最终验收不走这条：缺阶段名和修改意见就拒绝，改走 `reject_final`。
pub fn reject_stamp(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
) -> Result<orchestra::StageAction, ReviewError> {
    let (rid, seq, name) = waiting_stamp(db, project_id)?;
    if seq >= 0 && crate::stampgate::is_final_stamp(&stamp_flags(pack), seq as usize) {
        return Err(ReviewError::BadReject(
            "final acceptance reject requires a stage name and a revision note".into(),
        ));
    }
    db.conn().execute(
        "UPDATE stage_runs SET state='rejected', finished_at=datetime('now') WHERE id=?1",
        [&rid],
    )?;
    db.append_event(
        project_id,
        EventKind::StampRejected,
        json!({"stage": name, "seq": seq}),
        None,
        Some(&rid),
    )?;
    let prev = (seq - 1).max(0) as usize;
    let (new_rid, _) = orchestra::open_stage(db, project_id, pack, prev)?;
    Ok(orchestra::StageAction::StampRejected {
        reopened_seq: prev,
        run_id: new_rid,
    })
}

/// 最终验收退回。阶段名和修改意见都必填（空白不算）。只重开被点名的阶段，
/// 其余 stage_runs 保持原状态，不把整条流程打回起点。
pub fn reject_final(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    stage_name: &str,
    note: &str,
) -> Result<orchestra::StageAction, ReviewError> {
    let stage_name = stage_name.trim();
    let note = note.trim();
    if stage_name.is_empty() || note.is_empty() {
        return Err(ReviewError::BadReject(
            "final acceptance reject requires a stage name and a revision note".into(),
        ));
    }
    let (rid, seq, name) = waiting_stamp(db, project_id)?;
    if seq < 0 || !crate::stampgate::is_final_stamp(&stamp_flags(pack), seq as usize) {
        return Err(ReviewError::BadReject(
            "only the final acceptance gate takes a named-stage reject".into(),
        ));
    }
    let to_seq = pack
        .stages
        .iter()
        .position(|s| s.name == stage_name)
        .ok_or_else(|| ReviewError::BadReject(format!("unknown stage: {stage_name}")))?;
    db.conn().execute(
        "UPDATE stage_runs SET state='rejected', finished_at=datetime('now') WHERE id=?1",
        [&rid],
    )?;
    // 销掉本 run 的盖章卡。提案型 stamp 卡没有 run_id，按 run_id 匹配不碰提案面。
    crate::cards::answer_queued_where(
        db,
        project_id,
        crate::cards::CardKind::Stamp,
        "run_id",
        &rid,
        "owner",
    )?;
    db.append_event(
        project_id,
        EventKind::StampRejected,
        json!({
            "stage": name,
            "seq": seq,
            "to_stage": stage_name,
            "to_seq": to_seq,
            "note": note,
            "by": "owner",
        }),
        None,
        Some(&rid),
    )?;
    let (new_rid, _) = orchestra::open_stage(db, project_id, pack, to_seq)?;
    Ok(orchestra::StageAction::StampRejected {
        reopened_seq: to_seq,
        run_id: new_rid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::TierMap;

    fn setup(roles: &[&str]) -> (Db, tempfile::TempDir, PackDef) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES ('p1','/tmp/x','x','pack','L0')",
                [],
            )
            .unwrap();
        for (i, r) in roles.iter().enumerate() {
            db.conn()
                .execute(
                    "INSERT INTO agents (id, project_id, role) VALUES (?1,'p1',?2)",
                    rusqlite::params![format!("a{i}"), r],
                )
                .unwrap();
        }
        let pack: PackDef = serde_json::from_value(json!({
        "name":"t","version":1,"stages":[
            {"name":"界面","roles":["UI"],"due":["界面稿"]},
            {"name":"接口","roles":["前端","架构师"],"due":["接口说明"],
             "reviews":[{"artifact_kind":"界面稿","reviewer":"架构师"}],
             "backfill_edges":[["前端","UI"]]}
        ]}))
        .unwrap();
        (db, tempfile::tempdir().unwrap(), pack)
    }

    fn ctx(project: &str, agent: &str, dir: &std::path::Path, run: Option<&str>) -> ToolContext {
        ToolContext {
            project_id: project.into(),
            agent_id: agent.into(),
            repo_root: dir.to_path_buf(),
            stage_run_id: run.map(str::to_string),
            owned_globs: vec![],
            tiers: TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        }
    }

    /// 在 seq0 交付一个界面稿（由 a_ui 产出），返回 (run0_id, artifact_id)
    fn seed_artifact(
        db: &Db,
        dir: &std::path::Path,
        pack: &PackDef,
        ui_id: &str,
    ) -> (String, String) {
        let (r0, _) = orchestra::open_stage(db, "p1", pack, 0).unwrap();
        let c = ctx("p1", ui_id, dir, Some(&r0));
        let aid = artifacts::deliver(
            db,
            &c,
            &TierMap::new(),
            "ui/screens.md",
            "---\nkind: 界面稿\nauthor: x\n---\nbody",
            None,
        )
        .unwrap();
        (r0, aid)
    }

    #[test]
    fn review_pass_and_reject_events() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (r0, art) = seed_artifact(&db, dir.path(), &pack, "a0");
        let c = ctx("p1", "a2", dir.path(), Some(&r0));
        submit_review(&db, &c, &art, Verdict::Pass, "可以").unwrap();
        submit_review(&db, &c, &art, Verdict::Reject, "分页不一致").unwrap();
        let items = db
            .timeline(
                "p1",
                None,
                50,
                Some(&[EventKind::ReviewPassed, EventKind::ReviewRejected]),
            )
            .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].event.kind, EventKind::ReviewRejected);
        // 复审产物本身是 parse 档登记件
        let revs = artifacts::query(&db, "p1", Some("复审意见"), None, None, None).unwrap();
        assert_eq!(revs.len(), 2);
    }

    #[test]
    fn flag_routes_to_declared_reviewer() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        // 推进到接口阶段（前端在里面）：先关 seq0 run
        db.conn()
            .execute("UPDATE stage_runs SET state='done' WHERE seq=0", [])
            .unwrap();
        orchestra::open_next(&db, "p1", &pack, 1).unwrap();
        let r1: String = db
            .conn()
            .query_row(
                "SELECT id FROM stage_runs WHERE seq=1 AND state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let _ = r0;
        let c = ctx("p1", "a1", dir.path(), Some(&r1));
        let route =
            submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "折叠屏无布局规范").unwrap();
        // 回填边命中，执行档恒为 2，所以自动同意拨回，不再停在复审者。
        assert!(matches!(route, FlagRoute::AutoAdjudicated { .. }));
    }

    #[test]
    fn flag_escalates_when_stamped() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (r0, art) = seed_artifact(&db, dir.path(), &pack, "a0");
        db.conn()
            .execute("UPDATE artifacts SET status='stamped' WHERE id=?1", [&art])
            .unwrap();
        let c = ctx("p1", "a1", dir.path(), Some(&r0));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "s", "r").unwrap();
        assert!(matches!(route, FlagRoute::Escalated { .. }));
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::Escalated]))
            .unwrap();
        assert_eq!(
            items[0].event.payload["reason"],
            "target artifact already stamped"
        );
    }

    #[test]
    fn flag_escalates_when_reviewer_absent() {
        let (db, dir, pack) = setup(&["UI", "前端"]); // 无架构师
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        let c = ctx("p1", "a1", dir.path(), Some(&r0));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "s", "r").unwrap();
        assert!(matches!(route, FlagRoute::Escalated { .. }));
    }

    #[test]
    fn repeat_flag_escalates() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        let c = ctx("p1", "a1", dir.path(), Some(&r0));
        let r1 = submit_flag(&db, &c, &pack, "ui/screens.md", "s", "r1").unwrap();
        // 执行档恒为 2：第一次唤醒复审者，不再停成 ToReviewer。
        assert!(matches!(r1, FlagRoute::AutoWoken { .. }));
        let r2 = submit_flag(&db, &c, &pack, "ui/screens.md", "s", "r2").unwrap();
        assert!(matches!(r2, FlagRoute::Escalated { .. }));
    }

    #[test]
    fn backfill_edge_auto_adjudicates_at_l1() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        // ADR 0069：门面不再设档。本夹具写列，锁存储秩分支。
        db.conn()
            .execute("UPDATE projects SET autonomy='L1' WHERE id='p1'", [])
            .unwrap();
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        // 当前阶段 = 接口(seq1)：声明了回填边 前端→UI
        db.conn()
            .execute("UPDATE stage_runs SET state='done' WHERE seq=0", [])
            .unwrap();
        orchestra::open_next(&db, "p1", &pack, 1).unwrap();
        let r1: String = db
            .conn()
            .query_row(
                "SELECT id FROM stage_runs WHERE seq=1 AND state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let _ = r0;
        let c = ctx("p1", "a1", dir.path(), Some(&r1)); // a1=前端
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "无法实现").unwrap();
        assert_eq!(route, FlagRoute::AutoAdjudicated { to_seq: 0 });
        // 指针已拨回 seq0：新 active run 是界面
        let cur: i64 = db
            .conn()
            .query_row("SELECT seq FROM stage_runs WHERE state='active'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cur, 0);
        // 裁决事件留痕
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::FlagAdjudicated]))
            .unwrap();
        assert_eq!(items[0].event.payload["agree"], true);
    }

    #[test]
    fn flag_at_l2_auto_wakes_reviewer() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        db.conn()
            .execute("UPDATE projects SET autonomy='L2' WHERE id='p1'", [])
            .unwrap();
        let (_r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        // 架构师(a2) 质疑界面稿：架构师→UI 不在回填边里 → 不自动裁决，走 L2 唤醒
        let c = ctx("p1", "a2", dir.path(), Some(&_r0));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "规范冲突").unwrap();
        assert_eq!(
            route,
            FlagRoute::AutoWoken {
                reviewer_role: "架构师".into(),
                agent_id: "a2".into()
            }
        );
        // 复审者被激活 + ConsultWakeup 事件
        let st: String = db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "active");
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::ConsultWakeup]))
            .unwrap();
        assert_eq!(items[0].event.payload["reason"], "flag adjudication");
    }

    /// 票 01：L3/L4 存储是高档，执行与 L2 相同——非回填边只自动唤醒复审者，
    /// 不自动裁决，决策门上的 autonomy 是执行档 2 不是存储档 3/4。
    #[test]
    fn flag_at_l3_and_l4_executes_like_l2() {
        for lv in ["L3", "L4"] {
            let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
            db.conn()
                .execute("UPDATE projects SET autonomy=?1 WHERE id='p1'", [lv])
                .unwrap();
            assert_eq!(crate::autonomy::level(&db, "p1").unwrap(), lv);
            let (_r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
            let c = ctx("p1", "a2", dir.path(), Some(&_r0));
            let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "规范冲突").unwrap();
            assert_eq!(
                route,
                FlagRoute::AutoWoken {
                    reviewer_role: "架构师".into(),
                    agent_id: "a2".into()
                }
            );
            let items = db
                .timeline("p1", None, 50, Some(&[EventKind::ConsultWakeup]))
                .unwrap();
            assert_eq!(items[0].event.payload["decision"]["gates"]["autonomy"], 2);
        }
    }

    /// ADR 0069：夹具把列写成 L0 也不再把回填边留给人。命中即自动裁决并拨回。
    #[test]
    fn stored_l0_still_auto_backfills() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (_r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        db.conn()
            .execute("UPDATE stage_runs SET state='done' WHERE seq=0", [])
            .unwrap();
        orchestra::open_next(&db, "p1", &pack, 1).unwrap();
        let r1: String = db
            .conn()
            .query_row(
                "SELECT id FROM stage_runs WHERE seq=1 AND state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let c = ctx("p1", "a1", dir.path(), Some(&r1));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "无法实现").unwrap();
        assert!(matches!(route, FlagRoute::AutoAdjudicated { .. }));
        let cur: i64 = db
            .conn()
            .query_row("SELECT seq FROM stage_runs WHERE state='active'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cur, 0);
    }

    #[test]
    fn stamp_reject_reopens_previous_stage() {
        let (db, _dir, _pack) = setup(&["UI", "前端"]);
        let pack: PackDef = serde_json::from_value(json!({
        "name":"t","version":1,"stages":[
            {"name":"规格","roles":["UI"],"due":[]},
            {"name":"设计","roles":["前端"],"due":[],"stamp_point":true},
            {"name":"合入","roles":["前端"],"due":[],"stamp_point":true}
        ]}))
        .unwrap();
        // 设计不是最后一道盖章点。ADR 0069 之后 advance 会把它自动通过，
        // 到不了 waiting。这里把该行钉成 waiting，锁的是裸驳回仍退上一阶段。
        orchestra::open_stage(&db, "p1", &pack, 0).unwrap();
        orchestra::advance(&db, "p1", &pack).unwrap(); // seq0 done→seq1
        db.conn()
            .execute(
                "UPDATE stage_runs SET state='waiting_stamp' WHERE seq=1",
                [],
            )
            .unwrap();
        let r = serde_json::to_value(reject_stamp(&db, "p1", &pack).unwrap()).unwrap();
        assert_eq!(r["action"], "stamp_rejected");
        assert_eq!(r["reopened_seq"], 0);
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StampRejected]))
            .unwrap();
        assert_eq!(items.len(), 1);
    }

    // ---- rsi-research 票 01/02/04：决策点 + 闭集 code + 旋钮 ----

    #[test]
    fn flag_route_records_decision_with_eligible_and_gates() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        let (_r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        db.conn()
            .execute("UPDATE stage_runs SET state='done' WHERE seq=0", [])
            .unwrap();
        orchestra::open_next(&db, "p1", &pack, 1).unwrap();
        let r1: String = db
            .conn()
            .query_row(
                "SELECT id FROM stage_runs WHERE seq=1 AND state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 列是 L0，放行秩仍是原先的执行档 2。回填边命中就自动回填，决策门记 2。
        let c = ctx("p1", "a1", dir.path(), Some(&r1));
        submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "无法实现").unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::BackfillExecuted]))
            .unwrap();
        let d = &items[0].event.payload["decision"];
        assert_eq!(d["chosen"], "auto_backfill");
        assert_eq!(d["kind"], "flag_route");
        let eligible: Vec<&str> = d["eligible"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(eligible.contains(&"auto_backfill"));
        assert_eq!(items[0].event.payload["code"], "repairable");
        assert_eq!(d["gates"]["edge_hit"], true);
        assert_eq!(d["gates"]["autonomy"], 2);
    }

    #[test]
    fn auto_backfill_emits_backfill_executed_with_decision() {
        let (db, dir, pack) = setup(&["UI", "前端", "架构师"]);
        // ADR 0069：门面不再设档。本夹具写列，锁存储秩分支。
        db.conn()
            .execute("UPDATE projects SET autonomy='L1' WHERE id='p1'", [])
            .unwrap();
        let (_r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        db.conn()
            .execute("UPDATE stage_runs SET state='done' WHERE seq=0", [])
            .unwrap();
        orchestra::open_next(&db, "p1", &pack, 1).unwrap();
        let r1: String = db
            .conn()
            .query_row(
                "SELECT id FROM stage_runs WHERE seq=1 AND state='active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let c = ctx("p1", "a1", dir.path(), Some(&r1));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "无法实现").unwrap();
        assert!(matches!(route, FlagRoute::AutoAdjudicated { .. }));
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::BackfillExecuted]))
            .unwrap();
        assert_eq!(items.len(), 1);
        let p = &items[0].event.payload;
        assert_eq!(p["code"], "repairable");
        assert_eq!(p["decision"]["chosen"], "auto_backfill");
        // 夹具写成 L1，门上的数字仍是执行档 2，不再跟着列走。
        assert_eq!(p["decision"]["gates"]["autonomy"], 2);
    }

    #[test]
    fn escalation_carries_closed_code() {
        let (db, dir, pack) = setup(&["UI", "前端"]);
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        // 无声明复审者 → ambiguous 升级
        let c = ctx("p1", "a1", dir.path(), Some(&r0));
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "x").unwrap();
        assert!(matches!(route, FlagRoute::Escalated { .. }));
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::Escalated]))
            .unwrap();
        assert_eq!(items[0].event.payload["code"], "ambiguous");
        assert_eq!(items[0].event.payload["decision"]["chosen"], "escalated");
    }

    #[test]
    fn flag_patience_knob_controls_repeat_escalation() {
        let (db, dir, _pack) = setup(&["UI", "前端", "架构师"]);
        // flag_patience=4：同 Agent 第 2、3 次仍路由复审者,第 4 次才升级
        let pack: PackDef = serde_json::from_value(json!({
        "name":"t","version":1,"knobs":{"flag_patience":4},"stages":[
            {"name":"界面","roles":["UI"],"due":["界面稿"]},
            {"name":"接口","roles":["前端","架构师"],"due":["接口说明"],
             "reviews":[{"artifact_kind":"界面稿","reviewer":"架构师"}]}
        ]}))
        .unwrap();
        let (r0, _art) = seed_artifact(&db, dir.path(), &pack, "a0");
        let c = ctx("p1", "a1", dir.path(), Some(&r0));
        // 前三次唤醒复审者（执行档恒为 2，无回填边）。第四次才升级。
        for _ in 0..3 {
            let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "x").unwrap();
            assert!(matches!(route, FlagRoute::AutoWoken { .. }), "{route:?}");
        }
        // 第四次（prior=4 达到阈值）升级
        let route = submit_flag(&db, &c, &pack, "ui/screens.md", "断点", "x").unwrap();
        assert!(matches!(route, FlagRoute::Escalated { .. }));
    }
}
