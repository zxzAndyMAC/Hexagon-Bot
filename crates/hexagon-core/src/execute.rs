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

/// 交给 Jev 的状态。提案是产物文件里的原文。有回放围栏时，证据是那份原文，
/// 并附上主机已经算好的回放分；没有回放时，证据是上级复审已经通过。
pub(crate) fn judgment_state(proposal_id: &str, surface: &str, body: &str) -> String {
    let evidence = match crate::proposals::fenced(body, "replay") {
        Some(raw) => {
            let scores = crate::proposals::replay_from_body(body)
                .and_then(|r| r.ok())
                .map(|r| {
                    format!(
                        "\n回放分（主机已算，不要重算）：现任 {}，候选 {}",
                        crate::replay::score(&r.baseline),
                        crate::replay::score(&r.candidate)
                    )
                })
                .unwrap_or_default();
            format!("回放证据：\n{raw}{scores}")
        }
        None => "上级复审已通过。这份提案没有回放证据。".to_string(),
    };
    format!(
        "执行判定。只决定写不写。不要改写提案，不要重算回放分。\n\
         proposal {proposal_id}\n\
         surface {surface}\n\
         ## 已落盘的提案\n{body}\n\
         ## 已落盘的证据\n{evidence}\n"
    )
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
            None,
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
            None,
            "execute_judgment",
            "replay_score",
            started,
        );
        return Err(PropError::Rejected(reason));
    }

    // 输入就是已经落盘的提案正文和证据。Jev 只选写不写，这里不改这两样。
    let state = judgment_state(proposal_id, surface, body);
    // 没配或这次调用失败是槽位失败，记 Warn。对不上三个字才是判定上的交给负责人。
    // 被否决：两种都记成「判定 / 交给负责人」。那样日志里看不出 Jev 根本没跑。
    let (parsed, slot_failure) = match jev {
        Some(p) if p.uses_decision_api() => match p.decide(
            &state,
            &[
                (EXECUTE, "按提案写盘"),
                (REJECT, "不改项目"),
                (OWNER, "交给负责人"),
            ],
        ) {
            Ok(resp) => {
                let text = crate::pm_route::choice_text(&resp);
                (choice_of(&text), false)
            }
            Err(_) => (None, true),
        },
        _ => (None, true),
    };
    if slot_failure {
        let code = match jev {
            Some(p) if p.uses_decision_api() => "jev_call_failed",
            _ => "jev_unbound",
        };
        crate::diag::note(
            "槽位",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "execute_judgment",
            code,
            started,
        );
    }
    let choice = parsed.unwrap_or(OWNER);
    if !slot_failure {
        crate::diag::note(
            "判定",
            false,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "execute_judgment",
            match choice {
                EXECUTE => "execute",
                REJECT => "reject",
                _ => "hand_to_owner",
            },
            started,
        );
    }
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

#[cfg(test)]
mod tests {
    use super::{choice_of, mechanical_block, pack_score_block, EXECUTE, OWNER, REJECT};
    use proptest::prelude::*;

    proptest! {
        /// 机械拒绝只覆盖那五个面。其它表面，包括技能和流程包，不在这里挡。
        #[test]
        fn only_the_closed_surfaces_are_blocked(
            surface in "(permission|permissions|permission_rule|grant|grants|builtin_never|remote_publish|final_acceptance|skill|pack_copy|agents_md|role_def|[a-z]{1,8})",
        ) {
            let blocked = matches!(
                surface.as_str(),
                "permission"
                    | "permissions"
                    | "permission_rule"
                    | "grant"
                    | "grants"
                    | "builtin_never"
                    | "remote_publish"
                    | "final_acceptance"
            );
            prop_assert_eq!(mechanical_block(&surface), blocked);
        }

        /// 封闭选择只认三个词的逐字相等（首尾空白可去）。多一个字就不是选择。
        /// 漏接一个词 = 明确的执行被交给负责人；多认一个词 = 闲聊被当成执行。
        #[test]
        fn closed_choice_is_those_three_words(raw in "\\PC{0,40}") {
            let got = choice_of(&raw);
            match raw.trim() {
                "执行" => prop_assert_eq!(got, Some(EXECUTE)),
                "驳回" => prop_assert_eq!(got, Some(REJECT)),
                "交给负责人" => prop_assert_eq!(got, Some(OWNER)),
                _ => prop_assert!(got.is_none()),
            }
        }

        /// 回放分不高于现任就挡在判定前。其它表面、没有回放围栏，不在这里挡。
        #[test]
        fn replay_score_blocks_only_a_weaker_pack(
            surface in "(pack_copy|skill|role_def|agents_md)",
            base in 0u32..30,
            cand in 0u32..30,
            fenced in proptest::bool::ANY,
        ) {
            let body = if fenced {
                format!(
                    "```replay\n{{\"schema\":1,\"baseline\":{{\"stages_done\":{base}}},\"candidate\":{{\"stages_done\":{cand}}}}}\n```"
                )
            } else {
                format!("stages {base} {cand}")
            };
            let blocked = pack_score_block(&surface, &body);
            let expect = surface == "pack_copy" && fenced && cand <= base;
            prop_assert_eq!(blocked.is_some(), expect);
        }
    }
}
