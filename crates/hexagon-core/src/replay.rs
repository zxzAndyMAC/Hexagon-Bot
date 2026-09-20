//! 回放报告（rsi-research 票 06）：同一 scenario 脚本世界跑
//! 基线包 × 候选包，diff 两侧事件流产出版本化 `ReplayReport`——
//! 提案「验证方法」节的可盖章证据。
//!
//! 分层：纯函数（`metrics_from_events` / `decision_diffs` / `signals_of`）
//! 与副作用层（`replay` 跑两次 scenario）分开——指标口径一次钉死在
//! 纯函数里,`schema` 版本随口径变,保证跨时间的报告可比。
//! 边界：报告是证据不是授权——它只描述「这场景里两包差多少」,
//! 不外推「所有场景都好」;激活仍走既有机械校验+负责人盖章。

use crate::api::ApiError;
use crate::orchestra::{self, PackDef};
use crate::scenario::{run_scenario_capture, Scenario};
use crate::trace::{Event, EventKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 报告口径版本：指标集合/聚合规则变了就 bump,老报告不与新口径混比。
pub const REPLAY_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    /// 回合数（turn_started）
    pub turns: u32,
    /// 模型派发数（request_envelope 信封）
    pub model_calls: u32,
    /// 打回提交数
    pub flags: u32,
    /// 升级负责人裁决数
    pub escalations: u32,
    /// 自动回填执行数
    pub backfills: u32,
    /// 会诊唤醒数
    pub consult_wakes: u32,
    /// 复审驳回数
    pub review_rejects: u32,
    /// 闭集失败理由计数（code → 次数）
    pub failures_by_code: BTreeMap<String, u32>,
    /// 检验执行/失败数（test_ran 事件按 ok 字段分桶）
    pub checks_run: u32,
    pub checks_failed: u32,
    /// 决策点事件数（payload.decision 存在）
    pub decisions: u32,
    /// 完成阶段数 / 盖章数
    pub stages_done: u32,
    pub stamps: u32,
    /// 用量：输入/输出 token、成本（毫美分）
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_mc: i64,
    /// 跑完场景的墙钟耗时（毫秒）——成本信号之一
    pub wall_ms: u64,
    /// 不变量伴随件违规数（票 07 挂接）：>0 = 这侧证据不可信
    pub invariant_violations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub schema: u32,
    /// 场景指纹（脚本内容的 fnv64——同名不同内容不混比）
    pub scenario_fingerprint: String,
    pub baseline_pack: String,
    pub candidate_pack: String,
    /// 策略面 diff（票 04 policy_diff：只列旋钮差异）
    pub policy_diff: Vec<Value>,
    /// 非策略面差异路径——候选包动了流程定义时如实列出
    /// （policy-dev 的合规边界由 proposals 侧强制,这里只报告事实）
    pub non_policy_changes: Vec<String>,
    pub baseline: Metrics,
    pub candidate: Metrics,
    /// 决策点分歧：按 kind+序位配对,逐条记 chosen 是否不同
    pub decision_diffs: Vec<Value>,
    /// 机械信号行：{metric, baseline, candidate, delta}——judge/UX 的原料,
    /// 本层不做「好/坏」判断。
    pub signals: Vec<Value>,
}

/// 事件流 → 指标。口径即本函数——改这里必须 bump REPLAY_SCHEMA。
pub fn metrics_from_events(events: &[Event], usage: &[crate::usage::UsageRow]) -> Metrics {
    let mut m = Metrics::default();
    for e in events {
        let p = &e.payload;
        match e.kind {
            EventKind::TurnStarted => m.turns += 1,
            EventKind::FlagSubmitted => m.flags += 1,
            EventKind::Escalated => m.escalations += 1,
            EventKind::BackfillExecuted => m.backfills += 1,
            EventKind::ConsultWakeup => m.consult_wakes += 1,
            EventKind::ReviewRejected => m.review_rejects += 1,
            EventKind::StageFinished => m.stages_done += 1,
            EventKind::Stamped => m.stamps += 1,
            EventKind::TestRan => {
                m.checks_run += 1;
                if p["ok"] == false || p["exit_code"].as_i64().map(|c| c != 0).unwrap_or(false) {
                    m.checks_failed += 1;
                }
            }
            EventKind::System if p["kind"] == "request_envelope" => m.model_calls += 1,
            _ => {}
        }
        // 失败理由码只在终态类事件上聚合：flag_routed/backfill_executed
        // 上的 code 是「打回理由分类」不是失败本身（该量已由 flags/
        // backfills 指标覆盖）——曾把路由码也计入,一个多自动回填的包
        // 会假装「失败变多」误导判定层。
        if matches!(
            e.kind,
            EventKind::TurnFailed | EventKind::Escalated | EventKind::UsageCapHit
        ) {
            if let Some(code) = p.get("code").and_then(|c| c.as_str()) {
                *m.failures_by_code.entry(code.to_string()).or_insert(0) += 1;
            }
        }
        if p.get("decision").map(|d| d.is_object()).unwrap_or(false) {
            m.decisions += 1;
        }
    }
    for u in usage {
        m.tokens_in += u.prompt_tokens;
        m.tokens_out += u.completion_tokens;
        m.cost_mc += u.cost_mc;
    }
    m
}

/// 决策点配对 diff：两侧各取带 decision 的事件,按
/// (decision.kind, 该 kind 内序位) 对齐——同脚本世界下决策序列
/// 可比;序位错位（一侧某类决策更多）记为 extra 分歧。
pub fn decision_diffs(a: &[Event], b: &[Event]) -> Vec<Value> {
    fn decisions_of(events: &[Event]) -> BTreeMap<String, Vec<Value>> {
        let mut by_kind: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for e in events {
            if let Some(d) = e.payload.get("decision") {
                if let Some(k) = d["kind"].as_str() {
                    by_kind.entry(k.to_string()).or_default().push(d.clone());
                }
            }
        }
        by_kind
    }
    let (da, db) = (decisions_of(a), decisions_of(b));
    let kinds: std::collections::BTreeSet<&String> = da.keys().chain(db.keys()).collect();
    let mut out = Vec::new();
    for kind in kinds {
        let (va, vb) = (
            da.get(kind.as_str()).cloned().unwrap_or_default(),
            db.get(kind.as_str()).cloned().unwrap_or_default(),
        );
        let n = va.len().max(vb.len());
        for i in 0..n {
            match (va.get(i), vb.get(i)) {
                (Some(x), Some(y)) => {
                    if x["chosen"] != y["chosen"] {
                        out.push(json!({
                            "kind": kind, "index": i,
                            "baseline_chosen": x["chosen"],
                            "candidate_chosen": y["chosen"],
                            "baseline_eligible": x["eligible"],
                            "candidate_eligible": y["eligible"],
                        }));
                    }
                }
                (x, y) => out.push(json!({
                    "kind": kind, "index": i, "extra": true,
                    "baseline": x.map(|d| &d["chosen"]),
                    "candidate": y.map(|d| &d["chosen"]),
                })),
            }
        }
    }
    out
}

/// 机械信号行：把指标差拍平成 {metric, baseline, candidate, delta}。
/// 不做好坏判断——「打回更少」是否值得由判定层/负责人裁决。
fn signals_of(b: &Metrics, c: &Metrics) -> Vec<Value> {
    let mut out = Vec::new();
    let mut push = |metric: &str, x: i64, y: i64| {
        if x != y {
            out.push(json!({"metric": metric, "baseline": x, "candidate": y, "delta": y - x}));
        }
    };
    push("turns", b.turns as i64, c.turns as i64);
    push("model_calls", b.model_calls as i64, c.model_calls as i64);
    push("flags", b.flags as i64, c.flags as i64);
    push("escalations", b.escalations as i64, c.escalations as i64);
    push("backfills", b.backfills as i64, c.backfills as i64);
    push(
        "review_rejects",
        b.review_rejects as i64,
        c.review_rejects as i64,
    );
    push(
        "checks_failed",
        b.checks_failed as i64,
        c.checks_failed as i64,
    );
    push("stages_done", b.stages_done as i64, c.stages_done as i64);
    push(
        "consult_wakes",
        b.consult_wakes as i64,
        c.consult_wakes as i64,
    );
    push("checks_run", b.checks_run as i64, c.checks_run as i64);
    push("tokens_in", b.tokens_in, c.tokens_in);
    push("tokens_out", b.tokens_out, c.tokens_out);
    push("cost_mc", b.cost_mc, c.cost_mc);
    push("wall_ms", b.wall_ms as i64, c.wall_ms as i64);
    out
}

/// 同一场景跑两遍：dir_a 用 sc.pack（基线），dir_b 用候选包。
/// 两侧都在 ScriptedProvider 假世界——无网络、无真实副作用。
/// 返回版本化报告；跑失败把 step 错误上抛（报告不产半成品）。
pub fn replay(
    dir_a: &std::path::Path,
    dir_b: &std::path::Path,
    sc: &Scenario,
    candidate: &PackDef,
) -> Result<ReplayReport, ApiError> {
    let t0 = std::time::Instant::now();
    let ra = run_scenario_capture(dir_a, sc)?;
    let t_a = t0.elapsed();
    let sc_b = Scenario {
        roles: sc.roles.clone(),
        pack: Some(candidate.clone()),
        scripts: sc.scripts.clone(),
        steps: sc.steps.clone(),
    };
    let rb = run_scenario_capture(dir_b, &sc_b)?;
    let t_b = t0.elapsed() - t_a;

    let baseline_pack = sc
        .pack
        .as_ref()
        .map(|p| format!("{}@v{}", p.name, p.version))
        .unwrap_or_else(|| "fastpath".into());
    let policy_diff = sc
        .pack
        .as_ref()
        .map(|p| orchestra::policy_diff(p, candidate))
        .unwrap_or_default();
    let non_policy_changes = sc
        .pack
        .as_ref()
        .map(|p| orchestra::non_policy_changes(p, candidate))
        .unwrap_or_default();

    let mut baseline = metrics_from_events(&ra.events, &ra.usage.rows);
    baseline.wall_ms = t_a.as_millis() as u64;
    baseline.invariant_violations = ra.violations as u32;
    let mut candidate_m = metrics_from_events(&rb.events, &rb.usage.rows);
    candidate_m.wall_ms = t_b.as_millis() as u64;
    candidate_m.invariant_violations = rb.violations as u32;
    Ok(ReplayReport {
        schema: REPLAY_SCHEMA,
        // 场景指纹盖全输入：steps+scripts+roles——只 hash steps 曾让
        // 「同步骤不同脚本」的两个世界撞指纹（review 发现）。
        scenario_fingerprint: format!(
            "{:016x}",
            crate::tools::fnv64(
                &serde_json::to_string(&json!({
                    "steps": sc.steps, "scripts": sc.scripts, "roles": sc.roles,
                }))
                .unwrap_or_default()
            )
        ),
        baseline_pack,
        candidate_pack: format!("{}@v{}", candidate.name, candidate.version),
        policy_diff,
        non_policy_changes,
        signals: signals_of(&baseline, &candidate_m),
        baseline,
        candidate: candidate_m,
        decision_diffs: decision_diffs(&ra.events, &rb.events),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Event;

    fn ev(kind: EventKind, payload: Value) -> Event {
        Event {
            id: 0,
            project_id: "p".into(),
            stage_run_id: None,
            agent_id: None,
            kind,
            payload,
            created_at: String::new(),
        }
    }

    #[test]
    fn metrics_aggregate_failures_by_code() {
        let events = vec![
            ev(EventKind::TurnStarted, json!({})),
            ev(EventKind::TurnStarted, json!({})),
            ev(EventKind::FlagSubmitted, json!({})),
            ev(
                EventKind::Escalated,
                json!({"code": "ambiguous", "decision": {"kind": "flag_route"}}),
            ),
            ev(
                EventKind::BackfillExecuted,
                json!({"code": "repairable", "decision": {"kind": "flag_route"}}),
            ),
            ev(EventKind::System, json!({"kind": "request_envelope"})),
            ev(EventKind::System, json!({"kind": "request_envelope"})),
            ev(EventKind::TestRan, json!({"ok": false})),
            ev(EventKind::Stamped, json!({})),
        ];
        let usage = vec![crate::usage::UsageRow {
            agent_id: None,
            model: None,
            stage: None,
            prompt_tokens: 10,
            completion_tokens: 4,
            tool_output_tokens: 0,
            cost_mc: 7,
            calls: 1,
        }];
        let m = metrics_from_events(&events, &usage);
        assert_eq!(m.turns, 2);
        assert_eq!(m.model_calls, 2);
        assert_eq!(m.flags, 1);
        assert_eq!(m.escalations, 1);
        assert_eq!(m.backfills, 1);
        assert_eq!(m.decisions, 2);
        // 终态类事件的 code 进失败分布
        assert_eq!(m.failures_by_code["ambiguous"], 1);
        // 路由类 code（backfill 的 repairable）是打回理由分类,不是失败——
        // 不计入 failures_by_code（否则自动回填多的包会假装退步）。
        assert!(!m.failures_by_code.contains_key("repairable"));
        assert_eq!(m.checks_failed, 1);
        assert_eq!(m.tokens_in, 10);
        assert_eq!(m.cost_mc, 7);
    }

    #[test]
    fn decision_diffs_align_by_kind_and_index() {
        let d = |kind: &str, chosen: &str| json!({"kind": kind, "eligible": ["a","b"], "chosen": chosen});
        let a = vec![
            ev(
                EventKind::StageStarted,
                json!({"decision": d("roster","a")}),
            ),
            ev(
                EventKind::System,
                json!({"decision": d("flag_route","to_reviewer")}),
            ),
        ];
        let b = vec![
            ev(
                EventKind::StageStarted,
                json!({"decision": d("roster","a")}),
            ),
            ev(
                EventKind::System,
                json!({"decision": d("flag_route","auto_backfill")}),
            ),
        ];
        let diffs = decision_diffs(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0]["kind"], "flag_route");
        assert_eq!(diffs[0]["baseline_chosen"], "to_reviewer");
        assert_eq!(diffs[0]["candidate_chosen"], "auto_backfill");
    }

    #[test]
    fn replay_same_scenario_two_packs() {
        let base: PackDef = serde_json::from_value(json!({
            "name": "t", "version": 1,
            "stages": [{"name": "做", "roles": ["后端"], "due": ["代码"]}]
        }))
        .unwrap();
        let mut cand = base.clone();
        cand.knobs.flag_patience = Some(9); // 旋钮差：本场景不影响行为
        let sc: Scenario = serde_json::from_value(json!({
            "roles": ["后端"],
            "pack": serde_json::to_value(&base).unwrap(),
            "scripts": {"default": [{"text": "done"}]},
            "steps": [
                {"do": "open_stage", "seq": 0},
                {"do": "run_all_active", "input": "做"}
            ]
        }))
        .unwrap();
        let da = tempfile::tempdir().unwrap();
        let db_ = tempfile::tempdir().unwrap();
        let r = replay(da.path(), db_.path(), &sc, &cand).unwrap();
        assert_eq!(r.schema, REPLAY_SCHEMA);
        assert_eq!(r.baseline_pack, "t@v1");
        assert_eq!(r.policy_diff.len(), 1);
        assert_eq!(r.policy_diff[0]["path"], "knobs.flag_patience");
        assert!(r.non_policy_changes.is_empty());
        // 两侧同脚本同世界：指标相等、无决策分歧
        assert_eq!(r.baseline.turns, r.candidate.turns);
        assert!(r.decision_diffs.is_empty());
        assert!(r.baseline.model_calls >= 1);
        // 信封指纹可序列化（提案证据挂载用）
        assert!(serde_json::to_string(&r).is_ok());
    }

    #[test]
    fn replay_reports_process_changes_separately() {
        let base: PackDef = serde_json::from_value(json!({
            "name": "t", "version": 1,
            "stages": [{"name": "做", "roles": ["后端"], "due": ["代码"]}]
        }))
        .unwrap();
        let mut cand = base.clone();
        cand.stages[0].roles = vec!["架构师".into()]; // 流程改动
        let sc: Scenario = serde_json::from_value(json!({
            "roles": ["后端"],
            "pack": serde_json::to_value(&base).unwrap(),
            "scripts": {"default": [{"text": "done"}]},
            "steps": [{"do": "open_stage", "seq": 0}]
        }))
        .unwrap();
        let da = tempfile::tempdir().unwrap();
        let db_ = tempfile::tempdir().unwrap();
        let r = replay(da.path(), db_.path(), &sc, &cand).unwrap();
        assert!(r.policy_diff.is_empty());
        assert!(!r.non_policy_changes.is_empty());
        // 票 06 验收：行为发散的两包产出非空 decision_diffs——
        // 候选包把角色换成团队没有的「架构师」,roster 决策必然分歧
        // （基线 chosen=[后端],候选 chosen=[] + stage skipped）。
        assert!(
            !r.decision_diffs.is_empty(),
            "角色不同的两包 roster 决策应分歧: {:?}",
            r.decision_diffs
        );
        assert_eq!(r.decision_diffs[0]["kind"], "roster");
    }
}
