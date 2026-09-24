//! 判定后端缝（rsi-research 票 08）：提案盖章前的可换判定层。
//!
//! 闭集判定 {stamp, reject, needs-human}——「放行」永远只是建议,
//! 授权仍走负责人盖章。两个实现：
//!   MechanicalJudge — 纯函数,看回放报告的客观信号,永远可用（兜底）
//!   LlmJudge        — 读报告出大白话建议;畸形/超时/报错一律 needs-human
//! 判定结果落 System{kind:"judge_verdict"} 事件,标 deterministic 与否：
//! 机械判定可重放,LLM 判定只能对照不能重放——证据分级显式。
//! 代价模型：误判「可过」= 负责人盖了不该盖的章;误判「需人看」=
//! 多一次人工——所有存疑路径偏 needs-human。

use crate::db::Db;
use crate::orchestra::PackDef;
use crate::provider::{ChatRequest, ContentBlock, Message, ModelProvider, Role};
use crate::replay::ReplayReport;
use crate::trace::EventKind;
use serde_json::{json, Value};
use std::sync::Arc;

/// 判定层错误（D08 清尾）：judge 是叶子模块，不再以门面 `ApiError`
/// 为货币。`evaluate` 本身不返错（畸形/超时/报错一律 needs-human，
/// 见模块头代价模型），错误只来自落库/读文件/卡注解。
#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Trace(#[from] crate::trace::TraceError),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
}

/// 判定闭集：只增不改。needs-human 是所有异常路径的归宿。
/// 名 Judgement 而非 Verdict——review.rs 的复审裁定也叫 Verdict,
/// 同名易混（review 裁决产物,judge 裁决提案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    Stamp,
    Reject,
    NeedsHuman,
}

impl Judgement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stamp => "stamp",
            Self::Reject => "reject",
            Self::NeedsHuman => "needs-human",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "stamp" => Some(Self::Stamp),
            "reject" => Some(Self::Reject),
            "needs-human" | "needs_human" | "needsHuman" => Some(Self::NeedsHuman),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JudgeVerdict {
    pub verdict: Judgement,
    /// 给负责人看的一句话理由（大白话,不带术语）
    pub rationale: String,
    /// 后端标识：mechanical / llm:<slot>
    pub backend: String,
    /// 判定所用模型槽（机械后端为 None——票 08:verdict 事件要可溯源）
    pub model: Option<String>,
    /// 提示词/规则版本——同一后端换 prompt 后判定不可混比
    pub prompt_version: &'static str,
    /// 机械判定可重放（同输入同输出）;LLM 判定只能对照。
    pub deterministic: bool,
}

pub struct JudgeInput {
    pub proposal_id: String,
    /// 提案作者 agent——LLM 判定的信封/用量账记在它头上
    /// （判定的成本由提案方承担,账目归属可对账）。
    pub author_agent_id: String,
    pub surface: String,
    pub warnings: Vec<String>,
    pub report: ReplayReport,
}

/// 判定侧落盘上下文（票 03/08）：LLM judge 的派发也要落
/// request_envelope + 用量账——派发路径 100% 覆盖,判定不是特权通道。
pub struct JudgeObs<'a> {
    pub db: &'a Db,
    pub project_id: &'a str,
    pub repo_root: &'a std::path::Path,
}

pub trait JudgeBackend {
    fn evaluate(&self, input: &JudgeInput) -> JudgeVerdict;
}

/// 机械后端：只看报告里的客观计数。规则有意保守——
/// 打回/升级/检验失败/失败码任一变差 → reject 建议（明确退步）;
/// 全部不退步且有改善 → stamp 建议;无变化/缺证据 → needs-human。
/// 它不读语义,所以「看起来都好但流程定义变了」也归 needs-human。
pub struct MechanicalJudge;

impl JudgeBackend for MechanicalJudge {
    fn evaluate(&self, input: &JudgeInput) -> JudgeVerdict {
        let base = |v: Judgement, r: String| JudgeVerdict {
            verdict: v,
            rationale: r,
            backend: "mechanical".into(),
            model: None,
            prompt_version: "mech-v1",
            deterministic: true,
        };
        let r = &input.report;
        // 证据完整性先于指标：回放轨迹被伴随件标过违规 → 证据不可信,
        // 任何「改善/退步」结论都立不住——落 needs-human 让人看原始轨迹。
        if r.baseline.invariant_violations > 0 || r.candidate.invariant_violations > 0 {
            return base(
                Judgement::NeedsHuman,
                format!(
                    "回放轨迹完整性有疑点（基线 {} 处、候选 {} 处违规）——这份证据不能直接作数,需要人看原始事件流",
                    r.baseline.invariant_violations, r.candidate.invariant_violations
                ),
            );
        }
        if !r.non_policy_changes.is_empty() {
            return base(
                Judgement::NeedsHuman,
                format!(
                    "改动动了流程定义（{} 处），不只是调旋钮——这类改动该人逐条看",
                    r.non_policy_changes.len()
                ),
            );
        }
        let (b, c) = (&r.baseline, &r.candidate);
        let regressed: Vec<String> = [
            ("escalations", b.escalations, c.escalations),
            ("checks_failed", b.checks_failed, c.checks_failed),
            ("review_rejects", b.review_rejects, c.review_rejects),
            ("flags", b.flags, c.flags),
        ]
        .into_iter()
        .filter(|(_, x, y)| y > x)
        .map(|(m, x, y)| format!("{m} {x}→{y}"))
        .collect();
        // 失败码总数变差也算退步
        let fail_delta: i64 = c.failures_by_code.values().map(|v| *v as i64).sum::<i64>()
            - b.failures_by_code.values().map(|v| *v as i64).sum::<i64>();
        if !regressed.is_empty() || fail_delta > 0 {
            let mut why = regressed;
            if fail_delta > 0 {
                why.push(format!("失败类计数 +{fail_delta}"));
            }
            return base(
                Judgement::Reject,
                format!("回放里有指标变差：{}", why.join("；")),
            );
        }
        if r.signals.is_empty() {
            return base(
                Judgement::NeedsHuman,
                "回放结果与基线完全一致——没有可量化的收益,建议人定夺".into(),
            );
        }
        base(
            Judgement::Stamp,
            format!(
                "回放里无退步指标,{} 项有变化（如打回/升级/成本下降）。建议盖章,仍由你最终确认",
                r.signals.len()
            ),
        )
    }
}

/// LLM 后端：把报告摘要交给模型,要闭集 JSON 判定。
/// 一切异常路径——provider 错、输出非 JSON、verdict 词表外——
/// 都收敛为 needs-human（fail-closed 不对称:judge 说「过」只是建议,
/// 说不了「过」不伤害安全,说错「过」才危险,所以宁多不少）。
pub struct LlmJudge<'a> {
    pub provider: &'a dyn ModelProvider,
    pub slot: &'a str,
    /// 落盘上下文：Some 时派发前落信封、响应后记用量账。
    /// None（纯测试）= 静默判定,不留 trace。
    pub obs: Option<JudgeObs<'a>>,
}

impl LlmJudge<'_> {
    /// 给模型的提示词：只给机械事实,让它做「翻译+建议」而非重判。
    fn prompt(input: &JudgeInput) -> String {
        let r = &input.report;
        format!(
            "你是项目变更的判定助手。下面是同一场景回放基线包与候选包的对比报告(JSON)。\
             只输出一行 JSON: {{\"verdict\":\"stamp|reject|needs-human\",\"rationale\":\"一句话大白话理由,不带术语\"}}。\n\
             规则:指标退步→reject;全部不退步且有改善→stamp;拿不准/无变化/报告异常→needs-human。\n\
             报告:\n{}",
            serde_json::to_string(&json!({
                "surface": input.surface,
                "warnings": input.warnings,
                "policy_diff": r.policy_diff,
                "non_policy_changes": r.non_policy_changes,
                "baseline": r.baseline,
                "candidate": r.candidate,
                "signals": r.signals,
                "decision_diffs": r.decision_diffs,
            }))
            .unwrap_or_default()
        )
    }
}

impl JudgeBackend for LlmJudge<'_> {
    fn evaluate(&self, input: &JudgeInput) -> JudgeVerdict {
        let needs_human = |why: &str| JudgeVerdict {
            verdict: Judgement::NeedsHuman,
            rationale: format!("判定模型没能给出可用建议（{why}）——按惯例由人看"),
            backend: format!("llm:{}", self.slot),
            model: Some(self.slot.into()),
            prompt_version: "judge-v1",
            deterministic: false,
        };
        let req = ChatRequest {
            model_slot: self.slot.to_string(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: Self::prompt(input),
                }],
            }],
            tools: vec![],
        };
        // 票 03/08：judge 的模型调用也是派发——信封+用量照常落库,
        // 记提案作者头上（判定成本由提案方承担）。信封无 layer 层
        // （judge 不经过 PromptLayer 装配）,messages/指纹照常可重算。
        if let Some(obs) = &self.obs {
            let env = crate::turn::prompt::request_envelope(0, &req, &req.messages, &[]);
            let _ = obs.db.append_event(
                obs.project_id,
                EventKind::System,
                env,
                Some(&input.author_agent_id),
                None,
            );
        }
        // 超时由 provider 层 timeout_global 兜底（provider.rs:767，180s）——
        // 同步调用不另起超时机制,超了走 Err → needs-human。
        let mut sink = |_d: &crate::provider::StreamDelta| true;
        let resp = match self.provider.stream(&req, &mut sink) {
            Ok(r) => r,
            Err(e) => return needs_human(&e.to_string()),
        };
        if let Some(obs) = &self.obs {
            let ctx = crate::tools::ToolContext {
                project_id: obs.project_id.into(),
                agent_id: input.author_agent_id.clone(),
                repo_root: obs.repo_root.to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: crate::artifacts::TierMap::new(),
                sessions: Default::default(),
                caps: Default::default(),
                ..Default::default()
            };
            let _ = crate::usage::record(obs.db, &ctx, self.slot, &resp.usage, 0);
        }
        let text: String = resp
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        // 抽取首个 JSON 对象——模型多说的话不进判定。
        // 「}…{」形状（} 在 { 前）曾让 text[start..=end] 越界 panic——
        // fail-closed 被绕过；arch 票 08 属性测试抓到，guard 归 needs-human。
        let Some(start) = text.find('{') else {
            return needs_human("no json");
        };
        let Some(end) = text.rfind('}').filter(|e| *e > start) else {
            return needs_human("no json");
        };
        let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) else {
            return needs_human("bad json");
        };
        let Some(verdict) = v["verdict"].as_str().and_then(Judgement::parse) else {
            return needs_human("verdict outside closed set");
        };
        JudgeVerdict {
            verdict,
            rationale: v["rationale"]
                .as_str()
                .unwrap_or("（判定模型没给理由）")
                .to_string(),
            backend: format!("llm:{}", self.slot),
            model: Some(self.slot.into()),
            prompt_version: "judge-v1",
            deterministic: false,
        }
    }
}

/// 判定面（票 09）：每个面的后端独立可选,但不是一个全局开关。
/// Activation 是生效面校验（白名单/禁区/节流）——安全边界,永远机械,
/// 不接受任何配置;ProposalStamp 是提案盖章建议——可由 pack.knobs.judge 选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeFacet {
    /// 生效面校验：模型不得参与「能不能改」
    Activation,
    /// 提案盖章建议：「改得好不好、值不值得」可让模型发言
    ProposalStamp,
}

impl JudgeFacet {
    /// 该面允许配置后端吗？Activation 焊死——knobs 里写什么都没用,
    /// 这是不可绕过的不对称（B1）。
    fn configurable(self) -> bool {
        matches!(self, Self::ProposalStamp)
    }
}

/// 给负责人看的大白话判定行（票 09）：只用词典词（打回/盖章/回放/
/// 提案/返工），给具体后果,附「不确定怎么办」。不走术语、不报指标名
/// 原文——机械信号已经在 rationale 里翻成了人话,这里再包一层行动建议。
pub fn plain_line(v: &JudgeVerdict) -> String {
    match v.verdict {
        Judgement::Stamp => format!(
            "建议盖章。{}。拿不准也可以先盖——流程包的改动能回退。",
            v.rationale
        ),
        Judgement::Reject => format!(
            "建议驳回。{}。拿不准就先别盖,让改的人把变差的地方处理好再提。",
            v.rationale
        ),
        Judgement::NeedsHuman => format!(
            "机器给不了确定建议,需要你定。{}。认可收益就盖章,拿不准就驳回让对方补证据。",
            v.rationale
        ),
    }
}

/// 按面+包旋钮选后端：Activation → 永远机械（可配置面才读旋钮）;
/// ProposalStamp → off 关 / mechanical / llm（无 provider 落机械兜底——
/// 判定层 fail-closed 的最后一条命）。
pub fn backend_for<'a>(
    facet: JudgeFacet,
    pack: Option<&'a PackDef>,
    providers: &'a std::collections::HashMap<String, Arc<dyn ModelProvider>>,
    slot: &'a str,
    obs: Option<JudgeObs<'a>>,
) -> Option<Box<dyn JudgeBackend + 'a>> {
    if !facet.configurable() {
        return Some(Box::new(MechanicalJudge));
    }
    match pack.map(|p| p.knobs.judge()).unwrap_or("off") {
        "off" => None,
        "mechanical" => Some(Box::new(MechanicalJudge)),
        "llm" => {
            let started = std::time::Instant::now();
            if let Some(p) = crate::provider_config::resolve_slot(providers, slot) {
                // diagnostic-records 票 03：判定槽没绑，落到 default 是正常回退。
                if crate::provider_config::fell_back_to_default(providers, slot) {
                    crate::diag::slot_fallback(
                        obs.as_ref().map(|o| o.project_id),
                        None,
                        None,
                        "judge_backend",
                        slot,
                        started,
                    );
                }
                Some(Box::new(LlmJudge {
                    provider: p.as_ref(),
                    slot,
                    obs,
                }))
            } else {
                Some(Box::new(MechanicalJudge))
            }
        }
        _ => Some(Box::new(MechanicalJudge)),
    }
}

/// 提案判定：读产物正文里的回放证据 → 后端判定 → 落 judge_verdict
/// 事件 + 回填待决卡载荷（负责人开卡即见建议）。
/// 无证据/后端关闭 → Ok(None)——judge 只服务有回放证据的提案。
pub fn judge_proposal(
    db: &Db,
    project_id: &str,
    repo_root: &std::path::Path,
    backend: &dyn JudgeBackend,
    proposal_id: &str,
) -> Result<Option<JudgeVerdict>, JudgeError> {
    let (artifact_id, author): (Option<String>, String) = db.conn().query_row(
        "SELECT artifact_id, author_agent_id FROM proposals WHERE id=?1 AND project_id=?2",
        rusqlite::params![proposal_id, project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let Some(aid) = artifact_id else {
        return Ok(None);
    };
    let path: String =
        db.conn()
            .query_row("SELECT path FROM artifacts WHERE id=?1", [&aid], |r| {
                r.get(0)
            })?;
    let body = std::fs::read_to_string(repo_root.join(".hexagon").join(&path))?;
    let Some(Ok(report)) = crate::proposals::replay_evidence(&body) else {
        // 无证据提案标 judge_skip——不然 sweep 每回合都重读一遍产物
        // 文件（证据在文件里,查询层看不见）。
        let _ = crate::cards::annotate_queued_where(
            db,
            project_id,
            crate::cards::CardKind::Stamp,
            "proposal_id",
            proposal_id,
            &[("judge_skip", json!("no-evidence"))],
        );
        return Ok(None);
    };
    let (surface, _, diff) = crate::proposals::parse_proposal(&body)
        .unwrap_or_else(|_| ("unknown".into(), String::new(), String::new()));
    let warnings: Vec<String> = crate::proposals::risk_flags(&surface, &diff)
        .iter()
        .map(|s| s.to_string())
        .collect();
    let input = JudgeInput {
        proposal_id: proposal_id.into(),
        author_agent_id: author.clone(),
        surface,
        warnings,
        report,
    };
    let verdict = backend.evaluate(&input);
    // 判定落 trace——票 08 溯源四件：后端/模型/prompt 版本/报告指纹;
    // deterministic 标证据分级（false 的判定只能对照不能重放）。
    db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "judge_verdict",
            "proposal_id": proposal_id,
            "verdict": verdict.verdict.as_str(),
            "rationale": verdict.rationale,
            "backend": verdict.backend,
            "model": verdict.model,
            "prompt_version": verdict.prompt_version,
            "report_fp": input.report.scenario_fingerprint,
            "report_schema": input.report.schema,
            "line": plain_line(&verdict),
            "deterministic": verdict.deterministic,
        }),
        Some(&author),
        None,
    )?;
    // 回填待决卡：judge_verdict + judge_advice 进卡载荷
    crate::cards::annotate_queued_where(
        db,
        project_id,
        crate::cards::CardKind::Stamp,
        "proposal_id",
        proposal_id,
        &[
            ("judge_verdict", json!(verdict.verdict.as_str())),
            ("judge_advice", json!(verdict.rationale)),
            ("judge_backend", json!(verdict.backend)),
            ("judge_line", json!(plain_line(&verdict))),
        ],
    )?;
    Ok(Some(verdict))
}

/// 回合后扫尾（票 08 挂接点）：凡 awaiting_stamp 且带未判定回放证据的
/// 提案,跑一遍判定。从 turn 循环外调——provider 在那里可用。
/// 判定过的提案由卡载荷上的 judge_verdict 字段去重。
pub fn sweep(
    db: &Db,
    project_id: &str,
    repo_root: &std::path::Path,
    backend: &dyn JudgeBackend,
) -> Result<usize, JudgeError> {
    let ids = crate::cards::unjudged_stamp_proposals(db, project_id)?;
    let mut n = 0;
    for pid in ids {
        if judge_proposal(db, project_id, repo_root, backend, &pid)?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::Metrics;

    fn report(b_flags: u32, c_flags: u32, npc: usize) -> ReplayReport {
        let mut b = Metrics::default();
        let mut c = Metrics::default();
        b.flags = b_flags;
        c.flags = c_flags;
        ReplayReport {
            schema: crate::replay::REPLAY_SCHEMA,
            scenario_fingerprint: "fp".into(),
            baseline_pack: "t@v1".into(),
            candidate_pack: "t@v2".into(),
            policy_diff: vec![],
            non_policy_changes: vec!["".into(); npc],
            baseline: b,
            candidate: c,
            decision_diffs: vec![],
            signals: if b_flags != c_flags {
                vec![
                    json!({"metric": "flags", "baseline": b_flags, "candidate": c_flags, "delta": c_flags as i64 - b_flags as i64}),
                ]
            } else {
                vec![]
            },
        }
    }

    fn input(r: ReplayReport) -> JudgeInput {
        JudgeInput {
            proposal_id: "prop1".into(),
            author_agent_id: "a0".into(),
            surface: "pack_copy".into(),
            warnings: vec![],
            report: r,
        }
    }

    #[test]
    fn mechanical_verdicts() {
        // 退步 → reject
        let v = MechanicalJudge.evaluate(&input(report(1, 3, 0)));
        assert_eq!(v.verdict, Judgement::Reject);
        assert!(v.deterministic);
        // 改善 → stamp 建议
        let v = MechanicalJudge.evaluate(&input(report(3, 1, 0)));
        assert_eq!(v.verdict, Judgement::Stamp);
        // 无变化 → needs-human
        let v = MechanicalJudge.evaluate(&input(report(1, 1, 0)));
        assert_eq!(v.verdict, Judgement::NeedsHuman);
        // 动了流程定义 → needs-human（机械不审语义）
        let v = MechanicalJudge.evaluate(&input(report(3, 1, 2)));
        assert_eq!(v.verdict, Judgement::NeedsHuman);
        // 证据完整性违规 → needs-human（改善指标不作数）
        let mut r = report(3, 1, 0);
        r.candidate.invariant_violations = 1;
        let v = MechanicalJudge.evaluate(&input(r));
        assert_eq!(v.verdict, Judgement::NeedsHuman);
        assert!(v.rationale.contains("违规"));
    }

    #[test]
    fn llm_judge_closed_set_and_fail_closed() {
        // 正常：闭集 verdict + rationale
        let provider = crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(
            "{\"verdict\":\"stamp\",\"rationale\":\"打回从5降到1\"}",
        )]);
        let j = LlmJudge {
            provider: &provider,
            slot: "default",
            obs: None,
        };
        let v = j.evaluate(&input(report(5, 1, 0)));
        assert_eq!(v.verdict, Judgement::Stamp);
        assert!(!v.deterministic);
        assert_eq!(v.backend, "llm:default");

        // 畸形输出 → needs-human
        let provider =
            crate::provider::ScriptedProvider::new(vec![crate::turn::text_response("我觉得还行")]);
        let j = LlmJudge {
            provider: &provider,
            slot: "default",
            obs: None,
        };
        assert_eq!(
            j.evaluate(&input(report(5, 1, 0))).verdict,
            Judgement::NeedsHuman
        );

        // 词表外 verdict → needs-human
        let provider = crate::provider::ScriptedProvider::new(vec![crate::turn::text_response(
            "{\"verdict\":\"definitely_ship_it\"}",
        )]);
        let j = LlmJudge {
            provider: &provider,
            slot: "default",
            obs: None,
        };
        assert_eq!(
            j.evaluate(&input(report(5, 1, 0))).verdict,
            Judgement::NeedsHuman
        );
    }

    #[test]
    fn llm_judge_provider_error_is_needs_human() {
        // provider 空脚本 → stream Err → needs-human（不可用≠放行）
        let provider = crate::provider::ScriptedProvider::new(vec![]);
        let j = LlmJudge {
            provider: &provider,
            slot: "default",
            obs: None,
        };
        assert_eq!(
            j.evaluate(&input(report(5, 1, 0))).verdict,
            Judgement::NeedsHuman
        );
    }

    #[test]
    fn backend_selection_respects_knob() {
        use std::collections::HashMap;
        let pack: PackDef = serde_json::from_value(json!({
            "name": "t", "version": 1,
            "knobs": {"judge": "mechanical"},
            "stages": [{"name": "s", "roles": ["r"], "due": []}]
        }))
        .unwrap();
        let providers: HashMap<String, Arc<dyn ModelProvider>> = HashMap::new();
        assert!(backend_for(
            JudgeFacet::ProposalStamp,
            Some(&pack),
            &providers,
            "default",
            None,
        )
        .is_some());
        let mut off = pack.clone();
        off.knobs.judge = Some("off".into());
        assert!(backend_for(
            JudgeFacet::ProposalStamp,
            Some(&off),
            &providers,
            "default",
            None
        )
        .is_none());
        // llm 无 provider → 机械兜底（判定层永远有命）
        let mut llm = pack.clone();
        llm.knobs.judge = Some("llm".into());
        let b = backend_for(
            JudgeFacet::ProposalStamp,
            Some(&llm),
            &providers,
            "default",
            None,
        )
        .unwrap();
        let v = b.evaluate(&input(report(3, 1, 0)));
        assert_eq!(v.backend, "mechanical");
    }

    #[test]
    fn activation_facet_ignores_knob() {
        // B1:生效面校验不可配置——knobs.judge=llm 也只给机械后端。
        use std::collections::HashMap;
        let pack: PackDef = serde_json::from_value(json!({
            "name": "t", "version": 1,
            "knobs": {"judge": "llm"},
            "stages": [{"name": "s", "roles": ["r"], "due": []}]
        }))
        .unwrap();
        let providers: HashMap<String, Arc<dyn ModelProvider>> = HashMap::new();
        let b = backend_for(
            JudgeFacet::Activation,
            Some(&pack),
            &providers,
            "default",
            None,
        )
        .unwrap();
        let v = b.evaluate(&input(report(3, 1, 0)));
        assert_eq!(v.backend, "mechanical");
        assert!(v.deterministic);
    }

    #[test]
    fn plain_line_speaks_owner_language() {
        let v = JudgeVerdict {
            verdict: Judgement::Reject,
            rationale: "回放里有指标变差".into(),
            backend: "mechanical".into(),
            model: None,
            prompt_version: "mech-v1",
            deterministic: true,
        };
        let line = plain_line(&v);
        assert!(line.contains("驳回") && line.contains("拿不准"));
        let v = JudgeVerdict {
            verdict: Judgement::NeedsHuman,
            rationale: "x".into(),
            backend: "m".into(),
            model: None,
            prompt_version: "mech-v1",
            deterministic: true,
        };
        let line = plain_line(&v);
        assert!(line.contains("需要你定") && line.contains("盖章"));
    }

    // ---------- 判定面属性测试（arch 票 08）----------
    //
    // 打法照 permissions.rs prop_tests：任意 ReplayReport → 判定三条不变量
    // 恒成立（fail-closed 不对称：judge 说「过」只是建议，说不出「过」不伤
    // 安全，说错「过」才危险——所有疑点路径必须归 needs-human）。

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;
        use proptest::{collection, sample};

        /// 任意指标侧：judge 只读 flags/escalations/checks_failed/
        /// review_rejects/failures_by_code/invariant_violations。
        fn metrics() -> impl Strategy<Value = Metrics> {
            (
                0..20u32,
                0..10u32,
                0..10u32,
                0..10u32,
                0..2u32,
                collection::btree_map(sample::select(vec!["x", "y", "z"]), 0..10u32, 0..3),
            )
                .prop_map(|(flags, esc, cf, rr, inv, fails)| Metrics {
                    flags,
                    escalations: esc,
                    checks_failed: cf,
                    review_rejects: rr,
                    invariant_violations: inv,
                    failures_by_code: fails.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                    ..Default::default()
                })
        }

        fn arb_report() -> impl Strategy<Value = ReplayReport> {
            (
                metrics(),
                metrics(),
                0..3usize,
                collection::vec(0..5i64, 0..3),
            )
                .prop_map(|(baseline, candidate, npc, sig)| ReplayReport {
                    schema: crate::replay::REPLAY_SCHEMA,
                    scenario_fingerprint: "fp".into(),
                    baseline_pack: "t@v1".into(),
                    candidate_pack: "t@v2".into(),
                    policy_diff: vec![],
                    non_policy_changes: vec!["".into(); npc],
                    baseline,
                    candidate,
                    decision_diffs: vec![],
                    signals: sig
                        .into_iter()
                        .map(|d| json!({"metric":"m","baseline":0,"candidate":d,"delta":d}))
                        .collect(),
                })
        }

        /// 失败码总数差（机械判定口径）。
        fn fail_delta(r: &ReplayReport) -> i64 {
            r.candidate
                .failures_by_code
                .values()
                .map(|v| *v as i64)
                .sum::<i64>()
                - r.baseline
                    .failures_by_code
                    .values()
                    .map(|v| *v as i64)
                    .sum::<i64>()
        }

        fn regressed(r: &ReplayReport) -> bool {
            let (b, c) = (&r.baseline, &r.candidate);
            c.escalations > b.escalations
                || c.checks_failed > b.checks_failed
                || c.review_rejects > b.review_rejects
                || c.flags > b.flags
                || fail_delta(r) > 0
        }

        proptest! {
            /// 不变量①：任一側证据被标违规 → 恒 needs-human（证据不可信，
            /// 改善/退步结论都立不住）。
            #[test]
            fn violations_always_needs_human(
                mut r in arb_report(),
                side in any::<bool>(),
                n in 1..10u32,
            ) {
                if side { r.baseline.invariant_violations = n }
                else { r.candidate.invariant_violations = n }
                let v = MechanicalJudge.evaluate(&input(r));
                prop_assert_eq!(v.verdict, Judgement::NeedsHuman);
            }

            /// 不变量②：动了流程定义（non_policy_changes 非空）→ 恒
            /// needs-human——机械不审语义，人逐条看。
            #[test]
            fn non_policy_always_needs_human(
                mut r in arb_report(),
                extra in 1..4usize,
            ) {
                r.baseline.invariant_violations = 0;
                r.candidate.invariant_violations = 0;
                r.non_policy_changes = vec!["p".into(); extra];
                let v = MechanicalJudge.evaluate(&input(r));
                prop_assert_eq!(v.verdict, Judgement::NeedsHuman);
            }

            /// 不变量③：证据干净且未动流程时——任一盯防指标退步或失败码
            /// 总数变差 → 恒 reject。
            #[test]
            fn regression_always_rejects(mut r in arb_report()) {
                r.baseline.invariant_violations = 0;
                r.candidate.invariant_violations = 0;
                r.non_policy_changes = vec![];
                prop_assume!(regressed(&r));
                let v = MechanicalJudge.evaluate(&input(r));
                prop_assert_eq!(v.verdict, Judgement::Reject);
            }

            /// 不变量④（事后条件）：恒不 panic、verdict 恒在闭集、
            /// stamp 只在「零退步」下出现——改善证据不足时永不放行。
            #[test]
            fn verdict_closed_and_stamp_needs_no_regression(r in arb_report()) {
                let stampable = !regressed(&r);
                let v = MechanicalJudge.evaluate(&input(r));
                prop_assert!(matches!(
                    v.verdict,
                    Judgement::Stamp | Judgement::Reject | Judgement::NeedsHuman
                ));
                prop_assert!(v.deterministic);
                prop_assert_eq!(v.backend, "mechanical");
                if v.verdict == Judgement::Stamp {
                    prop_assert!(stampable, "stamp 出现在退步报告上");
                }
            }

            /// LLM 面 fail-closed：任意响应文本 → verdict 恒在闭集；
            /// 词表外/非 JSON 一律 needs-human，不放行。
            #[test]
            fn llm_verdict_closed_set(text in prop_oneof![
                // 多数：纯噪声文本（无 JSON / 词表外）
                any::<String>(),
                // 少数：合法 JSON + 混合词表（合法别名 + 杜撰词）
                (sample::select(vec![
                    "stamp", "reject", "needs-human", "needs_human",
                    "needsHuman", "definitely_ship_it", "STAMP", "",
                ])).prop_map(|w| format!("{{\"verdict\":\"{w}\",\"rationale\":\"r\"}}")),
            ]) {
                let provider = crate::provider::ScriptedProvider::new(vec![
                    crate::turn::text_response(&text),
                ]);
                let j = LlmJudge { provider: &provider, slot: "default", obs: None };
                let v = j.evaluate(&input(report(5, 1, 0)));
                prop_assert!(matches!(
                    v.verdict,
                    Judgement::Stamp | Judgement::Reject | Judgement::NeedsHuman
                ));
                prop_assert!(!v.deterministic);
                prop_assert!(v.backend.starts_with("llm:"));
                // 闭集词正常翻译；词表外/畸形恒 needs-human
                let parsed = text
                    .find('{')
                    .and_then(|s| {
                        text.rfind('}').filter(|e| *e > s).map(|e| &text[s..=e])
                    })
                    .and_then(|j| serde_json::from_str::<Value>(j).ok())
                    .and_then(|v| v["verdict"].as_str().and_then(Judgement::parse));
                match parsed {
                    Some(w) => prop_assert_eq!(v.verdict, w),
                    None => prop_assert_eq!(v.verdict, Judgement::NeedsHuman),
                }
            }
        }
    }
}
