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

/// 发给 Jev 的选项键（prompt-engineering 票 10 / ADR 0071）：语言无关，
/// 解析后映射回上面三个常量。上面三个是诊断与界面的写法，不改。
const EXECUTE_TOKEN: &str = "execute";
const REJECT_TOKEN: &str = "reject";
const OWNER_TOKEN: &str = "hand_to_owner";

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
                        "\nReplay scores (computed by the host, do not recompute): incumbent {}, candidate {}",
                        crate::replay::score(&r.baseline),
                        crate::replay::score(&r.candidate)
                    )
                })
                .unwrap_or_default();
            format!("Replay evidence:\n{raw}{scores}")
        }
        None => {
            "The superior's review has passed. This proposal has no replay evidence.".to_string()
        }
    };
    format!(
        "Execute judgment. Decide only whether to write it. Do not rewrite the proposal and do not recompute replay scores.\n\
         proposal {proposal_id}\n\
         surface {surface}\n\
         ## Proposal as written to disk\n{body}\n\
         ## Evidence as written to disk\n{evidence}\n"
    )
}

/// 封闭集合精确匹配：英文令牌（现行）与中文原文（旧脚本/旧回放）都收，
/// 其余一律 None → 交给负责人。
fn choice_of(text: &str) -> Option<&'static str> {
    match text.trim() {
        EXECUTE_TOKEN | EXECUTE => Some(EXECUTE),
        REJECT_TOKEN | REJECT => Some(REJECT),
        OWNER_TOKEN | OWNER => Some(OWNER),
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
    if let Some(reason) = pack_replay_format_block(surface, body) {
        crate::diag::note(
            "拒绝",
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "execute_judgment",
            "replay_format_invalid",
            started,
        );
        return Err(PropError::Rejected(reason));
    }

    // Reliability 21 / Q7: this also catches pre-upgrade in-review candidates.
    // False waiting costs one owner decision; false execution changes policy
    // without an independent quality gate, so no model score may bypass it.
    if surface == "pack_copy" {
        crate::diag::note(
            crate::diag::CLASS_JUDGE,
            false,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "policy_candidate",
            "owner_required",
            started,
        );
        let question_id =
            proposals::queue_for_owner(db, ctx, proposal_id, diff, surface, evidence)?;
        return Ok(Effect::Handed { question_id });
    }

    // 输入就是已经落盘的提案正文和证据。Jev 只选写不写，这里不改这两样。
    let state = judgment_state(proposal_id, surface, body);
    // 没配或这次调用失败是槽位失败，记 Warn。对不上三个字才是判定上的交给负责人。
    // 被否决：两种都记成「判定 / 交给负责人」。那样日志里看不出 Jev 根本没跑。
    let (parsed, slot_failure) = match jev {
        Some(p) if p.uses_decision_api() => match crate::usage::request(
            db,
            ctx,
            crate::provider_config::JEV_SLOT,
            "execute_judgment",
            p,
            None,
            || {
                p.decide(
                    &state,
                    &[
                        (EXECUTE_TOKEN, "write the proposal to disk"),
                        (REJECT_TOKEN, "leave the project unchanged"),
                        (OWNER_TOKEN, "hand the decision to the owner"),
                    ],
                )
            },
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

// Evaluation 16 / D13: replay remains diagnostic history. The previous
// score comparison discarded correct escalation and admitted no quality proof.
// Only malformed legacy evidence is rejected here; scores grant no authority.
pub fn pack_replay_format_block(surface: &str, body: &str) -> Option<String> {
    if surface != "pack_copy" {
        return None;
    }
    match proposals::replay_from_body(body) {
        Some(Err(error)) => Some(error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{choice_of, mechanical_block, pack_replay_format_block, EXECUTE, OWNER, REJECT};
    use proptest::prelude::*;

    /// prompt-engineering 票 10：Jev 选项键英文令牌，映射回原常量；中文原文仍收。
    #[test]
    fn english_tokens_map_to_the_same_choices() {
        assert_eq!(choice_of("execute"), Some(EXECUTE));
        assert_eq!(choice_of(" reject\n"), Some(REJECT));
        assert_eq!(choice_of("hand_to_owner"), Some(OWNER));
        assert_eq!(choice_of("执行"), Some(EXECUTE));
        assert_eq!(choice_of("Execute"), None);
    }

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

        // Evaluation 16/D13 replaces the old score gate: every valid score
        // remains available for independent evaluation, never grants adoption.
        #[test]
        fn replay_score_never_discards_a_valid_candidate(
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
            let blocked = pack_replay_format_block(&surface, &body);
            prop_assert!(blocked.is_none());
        }
    }
}
