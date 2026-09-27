//! 策略研发（rsi-research 票 10）：policy-dev 角色的机械打包路径。
//! 读世界池/证据 → 在钉住包的**副本**上改旋钮 → 跑回放 → 携
//! ReplayReport 进提案队列。这条路径没有任何特权：
//!   - 产出物是普通「改进提案」产物,走 proposals::submit 全校验
//!     （白名单/禁区/节流/盖章卡一样不少）;
//!   - KnobEdit 类型只能表达策略旋钮字段——想改流程定义在类型层
//!     就说不出来,再叠 non_policy_changes 双保险（防序列化绕行）;
//!   - 部署=负责人盖章,judge 判定只是卡上的建议行。
//!
//! 不做的事：不自动激活、不改钉住包、不碰 running 实例。

use crate::db::Db;
use crate::orchestra::{self, PackDef};
use crate::replay::{self, ReplayReport};
use crate::scenario::Scenario;
use crate::tools::ToolContext;

#[derive(Debug)]
pub enum PolicyDevError {
    /// 旋钮编辑指向不存在的阶段
    UnknownStage(String),
    /// 词表外的旋钮编辑（from_value 反序列化面拒绝未知 kind）
    BadEdit(String),
    /// 编辑集没产生任何策略面差异（空提案不入队）
    NoPolicyChange,
    /// 双保险触发：候选包出现了非策略面差异——正常编辑集到不了这里,
    /// 到了说明有代码绕过了 KnobEdit 类型,必须拒（fail-closed）。
    ProcessChanged(Vec<String>),
    /// ApiError 不内嵌——否则 ApiError→PolicyDevError→ApiError 递归无限大小
    Replay(String),
    Proposal(crate::proposals::PropError),
    Deliver(crate::artifacts::ArtifactError),
    Db(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for PolicyDevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownStage(s) => write!(f, "unknown stage: {s}"),
            Self::BadEdit(s) => write!(f, "unknown knob edit: {s}"),
            Self::NoPolicyChange => write!(f, "knob edits produced no policy diff"),
            Self::ProcessChanged(paths) => {
                write!(f, "candidate pack changed process definitions: {paths:?}")
            }
            Self::Replay(e) => write!(f, "replay failed: {e}"),
            Self::Proposal(e) => write!(f, "proposal rejected: {e}"),
            Self::Deliver(e) => write!(f, "artifact deliver failed: {e}"),
            Self::Db(e) => write!(f, "db: {e}"),
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}
impl std::error::Error for PolicyDevError {}

/// 旋钮编辑闭集：只能指向 Knobs/阶段策略字段——这是「policy-dev
/// 只能调策略不能改流程」的类型层表达。
#[derive(Debug, Clone)]
pub enum KnobEdit {
    Judge(String),
    FlagPatience(u32),
    AutoBackfill(bool),
    ConsultAutoWake(bool),
    StageStampPoint {
        stage: String,
        value: bool,
    },
    StageBackfillEdges {
        stage: String,
        edges: Vec<(String, String)>,
    },
    StageConsultWake {
        stage: String,
        roles: Vec<String>,
    },
}

/// 定名「流程优化」（2026-09-23，原「政策研发」）。
pub const ROLE: &str = "流程优化";

/// 团队里流程优化角色的 agent_id（票 10）：由负责人在建项目或调团队时勾选。
pub fn policy_dev_agent(db: &Db, project_id: &str) -> Result<String, rusqlite::Error> {
    db.conn().query_row(
        "SELECT id FROM agents WHERE project_id=?1 AND role=?2 ORDER BY created_at, id LIMIT 1",
        [project_id, ROLE],
        |r| r.get(0),
    )
}

impl KnobEdit {
    /// JSON → 旋钮编辑（IPC/测试同一入口）。词表外的键拒绝——
    /// 反序列化面也只吃旋钮,多一层防注入。
    pub fn from_value(v: &serde_json::Value) -> Result<Self, PolicyDevError> {
        let k = v["kind"].as_str().unwrap_or("");
        let st = v["stage"].as_str().map(|s| s.to_string());
        let val = &v["value"];
        Ok(match k {
            "judge" => Self::Judge(val.as_str().unwrap_or("off").into()),
            "flag_patience" => Self::FlagPatience(val.as_u64().unwrap_or(0) as u32),
            "auto_backfill" => Self::AutoBackfill(val.as_bool().unwrap_or(false)),
            "consult_auto_wake" => Self::ConsultAutoWake(val.as_bool().unwrap_or(false)),
            "stage_stamp_point" => Self::StageStampPoint {
                stage: st.unwrap_or_default(),
                value: val.as_bool().unwrap_or(false),
            },
            "stage_backfill_edges" => Self::StageBackfillEdges {
                stage: st.unwrap_or_default(),
                edges: val
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| {
                                Some((e[0].as_str()?.to_string(), e[1].as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "stage_consult_wake" => Self::StageConsultWake {
                stage: st.unwrap_or_default(),
                roles: val
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|r| r.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            _ => return Err(PolicyDevError::BadEdit(k.into())),
        })
    }
}

/// 阶段级编辑的公共前段：按名找阶段,找不到即拒。
fn stage_mut<'a>(
    cand: &'a mut PackDef,
    stage: &str,
) -> Result<&'a mut orchestra::StageDef, PolicyDevError> {
    cand.stages
        .iter_mut()
        .find(|s| s.name == stage)
        .ok_or_else(|| PolicyDevError::UnknownStage(stage.into()))
}

/// 应用编辑集到包的克隆上。返回 (候选包, 人类可读的改动描述行)。
pub fn apply_knob_edits(
    base: &PackDef,
    edits: &[KnobEdit],
) -> Result<(PackDef, Vec<String>), PolicyDevError> {
    let mut cand = base.clone();
    let mut desc = Vec::new();
    for e in edits {
        match e {
            KnobEdit::Judge(v) => {
                desc.push(format!("knobs.judge: {} → {v}", cand.knobs.judge()));
                cand.knobs.judge = Some(v.clone());
            }
            KnobEdit::FlagPatience(v) => {
                desc.push(format!(
                    "knobs.flag_patience: {} → {v}",
                    cand.knobs.flag_patience()
                ));
                cand.knobs.flag_patience = Some(*v);
            }
            KnobEdit::AutoBackfill(v) => {
                desc.push(format!(
                    "knobs.auto_backfill: {} → {v}",
                    cand.knobs.auto_backfill()
                ));
                cand.knobs.auto_backfill = Some(*v);
            }
            KnobEdit::ConsultAutoWake(v) => {
                desc.push(format!(
                    "knobs.consult_auto_wake: {} → {v}",
                    cand.knobs.consult_auto_wake()
                ));
                cand.knobs.consult_auto_wake = Some(*v);
            }
            KnobEdit::StageStampPoint { stage, value } => {
                let st = stage_mut(&mut cand, stage)?;
                desc.push(format!(
                    "stages.{stage}.stamp_point: {} → {value}",
                    st.stamp_point
                ));
                st.stamp_point = *value;
            }
            KnobEdit::StageBackfillEdges { stage, edges } => {
                let st = stage_mut(&mut cand, stage)?;
                desc.push(format!(
                    "stages.{stage}.backfill_edges: {:?} → {:?}",
                    st.backfill_edges, edges
                ));
                st.backfill_edges = edges.clone();
            }
            KnobEdit::StageConsultWake { stage, roles } => {
                let st = stage_mut(&mut cand, stage)?;
                desc.push(format!(
                    "stages.{stage}.consult_wake: {:?} → {:?}",
                    st.consult_wake, roles
                ));
                st.consult_wake = roles.clone();
            }
        }
    }
    Ok((cand, desc))
}

/// 全流程：副本改旋钮 → 守卫校验 → 回放 → 提案入队。
/// `scenario` 是钉死的评估世界（自带脚本供应商）。沙箱目录由内部
/// tempdir 承担——回放不碰项目仓。
/// `sandbox`：两个回放跑各自建子目录（a=基线,b=候选）——
/// 建议传项目 `.hexagon/replay/` 下的新目录;回放产物留在仓内可查。
pub fn propose(
    db: &Db,
    ctx: &ToolContext,
    base: &PackDef,
    edits: &[KnobEdit],
    scenario: &Scenario,
    motive: &str,
    sandbox: &std::path::Path,
) -> Result<String, PolicyDevError> {
    let body = prepare_body(ctx, base, edits, scenario, motive, sandbox)?;
    // 产物交付 + 提案入队——全走既有受管路径
    let aid = crate::artifacts::deliver(
        db,
        ctx,
        &ctx.tiers,
        "proposals/policy-dev.md",
        &body,
        Some("改进提案"),
    )
    .map_err(PolicyDevError::Deliver)?;
    crate::proposals::submit(db, ctx, &aid, &body).map_err(PolicyDevError::Proposal)
}

// Evaluation 17: delivery persists its own file intent before source/proposal
// commit. Preparing the diagnostic body separately avoids nesting transactions.
pub(crate) fn prepare_body(
    ctx: &ToolContext,
    base: &PackDef,
    edits: &[KnobEdit],
    scenario: &Scenario,
    motive: &str,
    sandbox: &std::path::Path,
) -> Result<String, PolicyDevError> {
    let (cand, desc) = apply_knob_edits(base, edits)?;

    // 双保险（票 10 强制点）：KnobEdit 类型层只许旋钮,
    // 仍跑 non_policy_changes——绕行者（序列化注入/未来代码改动）
    // 在这道墙前现形。
    let npc = orchestra::non_policy_changes(base, &cand);
    if !npc.is_empty() {
        return Err(PolicyDevError::ProcessChanged(npc));
    }
    if orchestra::policy_diff(base, &cand).is_empty() {
        return Err(PolicyDevError::NoPolicyChange);
    }

    // 回放：基线场景的世界以 base 包跑,候选包跑同一世界
    let dir_a = sandbox.join("baseline");
    let dir_b = sandbox.join("candidate");
    std::fs::create_dir_all(&dir_a).map_err(PolicyDevError::Io)?;
    std::fs::create_dir_all(&dir_b).map_err(PolicyDevError::Io)?;
    let mut sc_base = scenario.clone();
    sc_base.pack = Some(base.clone());
    let report = replay::replay(&dir_a, &dir_b, &sc_base, &cand)
        .map_err(|e| PolicyDevError::Replay(e.to_string()))?;

    // 提案正文：四必备节 + diff 块（旋钮 diff 的文本形）+ replay 证据块
    let diff_text = desc
        .iter()
        .map(|d| format!("+ {d}"))
        .collect::<Vec<_>>()
        .join("\n");
    let benefit = summarize_signals(&report);
    let mut body = format!(
        "---\nkind: 改进提案\nauthor: {}\nsurface: pack_copy\ntarget: .hexagon/pack.active.json\n---\n\
         ## 动机\n{motive}\n\n## 改动面\n```diff\n{diff_text}\n```\n\n\
         ## 预期收益\n{benefit}\n\n## 验证方法\n场景回放（指纹 {}）\n",
        ctx.agent_id, report.scenario_fingerprint
    );
    // 机械判定进提案正文（票 10 验收：提案必附 judge verdict）。
    // 用机械后端——证据层判定保持可重放;LLM 判定留在盖章卡扫尾。
    let verdict = crate::judge::JudgeBackend::evaluate(
        &crate::judge::MechanicalJudge,
        &crate::judge::JudgeInput {
            proposal_id: String::new(),
            author_agent_id: ctx.agent_id.clone(),
            surface: "pack_copy".into(),
            warnings: vec![],
            report: report.clone(),
        },
    );
    body = crate::proposals::attach_replay_evidence(&body, &report);
    body.push_str(&format!(
        "\n```judge\n{{\"verdict\":\"{}\",\"rationale\":\"{}\",\"backend\":\"{}\"}}\n```\n",
        verdict.verdict.as_str(),
        verdict.rationale.replace('"', "'"),
        verdict.backend,
    ));

    // Reliability 21: structured candidate data is applied by the host. The
    // human-readable knob diff is not a shell patch or authority to edit files.
    body.push_str(&format!(
        "\n```policy\n{}\n```\n",
        serde_json::json!({"baseline":base,"candidate":cand})
    ));

    Ok(body)
}

/// 信号行 → 人话收益句（提案「预期收益」节的原料）。
fn summarize_signals(r: &ReplayReport) -> String {
    if r.signals.is_empty() {
        return "回放与基线无显著差异".into();
    }
    r.signals
        .iter()
        .map(|s| {
            format!(
                "{}: {} → {}",
                s["metric"].as_str().unwrap_or("?"),
                s["baseline"],
                s["candidate"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::TierMap;
    use serde_json::json;

    fn setup() -> (Db, ToolContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".hexagon")).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p',?1,'x','pack')",
                [dir.path().to_string_lossy().to_string()],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status)
                 VALUES ('a0','p','流程优化','active')",
                [],
            )
            .unwrap();
        (
            db,
            ToolContext {
                project_id: "p".into(),
                agent_id: "a0".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: TierMap::new(),
                sessions: Default::default(),
                caps: Default::default(),
                ..Default::default()
            },
            dir,
        )
    }

    fn base_pack() -> PackDef {
        serde_json::from_value(json!({
            "name": "t", "version": 1,
            "stages": [{"name": "做", "roles": ["后端"], "due": ["代码"]}]
        }))
        .unwrap()
    }

    fn scenario() -> Scenario {
        serde_json::from_value(json!({
            "roles": ["后端"],
            "pack": serde_json::to_value(base_pack()).unwrap(),
            "scripts": {"default": [{"text": "done"}]},
            "steps": [
                {"do": "open_stage", "seq": 0},
                {"do": "run_all_active", "input": "做"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn knob_proposal_flows_through_governed_queue() {
        let (db, ctx, dir) = setup();
        let sandbox = dir.path().join(".hexagon/replay/t1");
        // ADR 0069：这段脚本两边回放分一样（都是 0）。不高于现任就到不了执行判定。
        let err = propose(
            &db,
            &ctx,
            &base_pack(),
            &[KnobEdit::FlagPatience(5)],
            &scenario(),
            "减少重复打回打扰",
            &sandbox,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not higher"), "{err}");
    }

    #[test]
    fn empty_edit_set_rejected() {
        let (db, ctx, dir) = setup();
        let sandbox = dir.path().join(".hexagon/replay/t2");
        let e = propose(&db, &ctx, &base_pack(), &[], &scenario(), "m", &sandbox).unwrap_err();
        assert!(matches!(e, PolicyDevError::NoPolicyChange));
    }

    #[test]
    fn unknown_stage_edit_rejected() {
        let (db, ctx, dir) = setup();
        let e = apply_knob_edits(
            &base_pack(),
            &[KnobEdit::StageStampPoint {
                stage: "不存在".into(),
                value: true,
            }],
        )
        .unwrap_err();
        assert!(matches!(e, PolicyDevError::UnknownStage(_)));
        let _ = (db, ctx, dir);
    }
}
