//! 执行判定（ADR 0069）。
//!
//! 证据齐了之后的一道封闭选择：执行、驳回、或交给负责人。只由 Jev 做。
//! 概率摊平、置信度不足、没配、这次调用失败，都是交给负责人。不改提案正文，
//! 不重算回放分，不推翻前面的机械拒绝。不用聊天模型再试一次。
//!
//! 代价：把拿不准判成执行 = 一次没人看过的改动生效（false execute）。
//! 把明确的执行判成交给负责人 = 多一张待决卡（false wait）。偏向交给负责人。

use crate::db::Db;
use crate::proposals::{self, PropError};
use crate::provider::ModelProvider;
use crate::tools::ToolContext;
use crate::trace::EventKind;
use serde_json::{json, Value};

pub const EXECUTE: &str = "执行";
pub const REJECT: &str = "驳回";
pub const OWNER: &str = "交给负责人";

/// 这些面在调用 Jev 之前拒绝。它们不是执行判定的输入。
pub fn mechanical_block(surface: &str) -> bool {
    matches!(
        surface,
        "permission"
            | "permissions"
            | "permission_rule"
            | "grant"
            | "grants"
            | "builtin_never"
            | "remote_publish"
            | "final_acceptance"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Executed,
    Rejected,
    Handed { question_id: String },
}

fn choice_of(text: &str) -> Option<&'static str> {
    match text.trim() {
        EXECUTE => Some(EXECUTE),
        REJECT => Some(REJECT),
        OWNER => Some(OWNER),
        _ => None,
    }
}

/// 上级复审已经通过。本函数决定写不写。`jev` 必须是决策接口；
/// 调用方不得把聊天槽传进来。
#[allow(clippy::too_many_arguments)]
pub fn judge_passed(
    db: &Db,
    ctx: &ToolContext,
    proposal_id: &str,
    surface: &str,
    diff: &str,
    body: &str,
    evidence: Option<&Value>,
    jev: Option<&dyn ModelProvider>,
) -> Result<Effect, PropError> {
    let started = std::time::Instant::now();
    if mechanical_block(surface) {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            "execute_judgment",
            "mechanical_block",
            started,
        );
        return Err(PropError::Rejected(format!(
            "surface is not an execute-judgment input: {surface}"
        )));
    }
    if let Some(reason) = pack_score_block(surface, body) {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            "execute_judgment",
            "replay_score",
            started,
        );
        return Err(PropError::Rejected(reason));
    }

    let choice = match jev {
        Some(p) if p.uses_decision_api() => match p.decide(
            &format!("proposal {proposal_id} surface {surface}"),
            &[
                (EXECUTE, "按提案写盘"),
                (REJECT, "不改项目"),
                (OWNER, "交给负责人"),
            ],
        ) {
            Ok(resp) => {
                let text = crate::pm_route::choice_text(&resp);
                choice_of(&text)
            }
            Err(_) => None,
        },
        _ => None,
    };
    let choice = choice.unwrap_or(OWNER);
    crate::diag::note(
        "判定",
        false,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        "execute_judgment",
        match choice {
            EXECUTE => "execute",
            REJECT => "reject",
            _ => "hand_to_owner",
        },
        started,
    );
    match choice {
        EXECUTE => {
            let effective = proposals::materialize_for_judgment(db, ctx, proposal_id)?;
            db.append_event(
                &ctx.project_id,
                EventKind::ProposalStamped,
                json!({"proposal_id": proposal_id, "by": "execute_judgment", "evidence": evidence}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            db.append_event(
                &ctx.project_id,
                EventKind::ProposalActivated,
                json!({"proposal_id": proposal_id, "effective_path": effective, "by": "execute_judgment"}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            Ok(Effect::Executed)
        }
        REJECT => {
            db.conn().execute(
                "UPDATE proposals SET status='rejected', decided_at=datetime('now') WHERE id=?1",
                [proposal_id],
            )?;
            db.append_event(
                &ctx.project_id,
                EventKind::ProposalRejected,
                json!({"proposal_id": proposal_id, "by": "execute_judgment"}),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            let _ = diff;
            Ok(Effect::Rejected)
        }
        _ => {
            let qid = proposals::queue_for_owner(db, ctx, proposal_id, diff, surface, evidence)?;
            Ok(Effect::Handed { question_id: qid })
        }
    }
}

/// 编排策略：回放分不高于现任则到不了执行判定。没有回放块时不在这里拦
/// （提交口已经要求流程优化附回放）。
pub fn pack_score_block(surface: &str, body: &str) -> Option<String> {
    if surface != "pack_copy" {
        return None;
    }
    let report = match proposals::replay_from_body(body) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return Some(e),
        None => return None,
    };
    let base = crate::replay::score(&report.baseline);
    let cand = crate::replay::score(&report.candidate);
    if cand <= base {
        Some(format!(
            "replay score {cand} is not higher than incumbent {base}"
        ))
    } else {
        None
    }
}
