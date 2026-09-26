//! 编排内核：流程包驱动阶段推进。
//!
//! - 版本钉住：开跑时把包副本快照到 `.hexagon/pack.active.json`，运行中实例
//!   只读这份；提案改 `.hexagon/pack.json` 下次开跑生效。
//! - 阶段推进：按包声明名单激活/休眠 Agent；阶段名单与团队交集为空 → 跳过；
//!   阶段内并行默认。
//! - 成功判定（工作台裁决）：应交产物齐且登记 + 检验命令全过 + 声明复审全过。
//! - 声明式回路：回填边与会诊唤醒名单是包内声明数据（票 10 消费裁决语义）；
//!   盖章点到点停为待决问题。L3/L4 的非最终盖章点自动通过（`stampgate`），
//!   最后一个盖章点仍停。自动通过的轨迹 `by=autonomy`，人工盖章 `by=owner`。
//! - 阶段动作：退回/跳过/暂停/恢复，全落事件。

mod exception;
pub use exception::{
    accept as accept_delivery_exception, cancel as cancel_acceptance_exception,
    request as request_acceptance_exception,
};
pub use exception::{
    ExceptionAcceptance, ExceptionCandidate, ExceptionRequest, ExceptionRequirement,
};

use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OrchError {
    #[error("resolve unknown tool outcomes before accepting delivery exceptions")]
    UnresolvedDeliveryAction,
    #[error("select current requirements and provide a reason in an owner exception card")]
    InvalidAcceptanceException,
    #[error("delivery changed; request a new exception for the current version")]
    StaleAcceptanceVersion,
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no active stage run for project {0}")]
    NoActiveStage(String),
    #[error("invalid stage seq: {0}")]
    BadSeq(i64),
    #[error("project paused")]
    Paused,
    #[error("stage run not interrupted: {0}")]
    NotInterrupted(String),
    #[error("no failing checks to override on run {0}")]
    NothingToOverride(String),
}

// ---------- 流程包定义 ----------

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReviewDecl {
    /// 复审对象：产物 kind
    pub artifact_kind: String,
    /// 复审者角色
    pub reviewer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct StageDef {
    pub name: String,
    /// 本阶段激活的角色名单（与团队交集为空则整阶段跳过）
    pub roles: Vec<String>,
    /// 应交产物 kind 列表
    pub due: Vec<String>,
    /// 检验命令（包内声明 = 负责人批准，直接执行不过权限管线）
    #[serde(default)]
    pub checks: Vec<String>,
    /// 声明复审
    #[serde(default)]
    pub reviews: Vec<ReviewDecl>,
    /// 盖章点：达成后到点停等负责人
    #[serde(default)]
    pub stamp_point: bool,
    /// 自动回填边 (from_role → to_role)：下游打回命中即拨回
    #[serde(default)]
    pub backfill_edges: Vec<(String, String)>,
    /// 会诊唤醒名单：声明可被唤醒咨询的休眠角色
    #[serde(default)]
    pub consult_wake: Vec<String>,
}

/// 策略旋钮（rsi-research 票 04）：pack 里「怎么编排」的可调面，
/// 与「流程定义」（角色/阶段/验收/检验）分开——前者是回放评估与
/// policy-dev 可改的旋钮集，后者动它们等于改流程本身。
/// 全部 Option：缺席 = 内核默认。词表见 docs/glossary.html「策略旋钮」。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct Knobs {
    /// 判定后端选择："mechanical" / "llm" / "off"（默认 off——
    /// 未声明的包不跑判定，judge 输出只服务盖章建议不授权）。
    #[serde(default)]
    pub judge: Option<String>,
    /// 同 Agent 对同产物的打回升级阈值（默认 2：第 2 次起升级负责人）。
    #[serde(default)]
    pub flag_patience: Option<u32>,
    /// 回填边自动裁决总开关（默认 true；false 时 L1+ 也不自动拨回）。
    #[serde(default)]
    pub auto_backfill: Option<bool>,
    /// L2 会诊唤醒总开关（默认 true；false 时 L2 也不自动唤醒复审者）。
    #[serde(default)]
    pub consult_auto_wake: Option<bool>,
}

impl Knobs {
    pub fn flag_patience(&self) -> u32 {
        self.flag_patience.unwrap_or(2).max(1)
    }
    pub fn auto_backfill(&self) -> bool {
        self.auto_backfill.unwrap_or(true)
    }
    pub fn consult_auto_wake(&self) -> bool {
        self.consult_auto_wake.unwrap_or(true)
    }
    pub fn judge(&self) -> &str {
        self.judge.as_deref().unwrap_or("off")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PackDef {
    pub name: String,
    pub version: u32,
    pub stages: Vec<StageDef>,
    /// 策略旋钮——策略面字段（区别于流程定义）。
    #[serde(default)]
    pub knobs: Knobs,
}

impl PackDef {
    /// 阶段级策略面字段：与 pack.knobs 同属「旋钮」。stages[i] 的
    /// name/roles/due/reviews/checks 是流程定义,不在此列。
    const STAGE_KNOB_FIELDS: &[&str] = &["stamp_point", "backfill_edges", "consult_wake"];

    pub fn load(path: impl AsRef<Path>) -> Result<Self, OrchError> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
    /// 钉住副本：快照为 pack.active.json（运行实例只读它）。
    pub fn pin(&self, repo_root: &Path) -> Result<(), OrchError> {
        let dir = repo_root.join(".hexagon");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("pack.active.json"),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }
    /// 读运行实例钉住的副本。
    pub fn pinned(repo_root: &Path) -> Result<Self, OrchError> {
        Self::load(repo_root.join(".hexagon/pack.active.json"))
    }
}

fn knob_changes(path: &str, a: &Value, b: &Value, out: &mut Vec<Value>) {
    if a != b {
        out.push(json!({"path": path, "from": a, "to": b}));
    }
}

/// 策略面 diff（票 04）：只比旋钮——pack 级 knobs 四字段 +
/// 按阶段名配对的 stamp_point/backfill_edges/consult_wake。
/// 流程定义的增删改不进入本结果（用 `non_policy_changes` 取另一面）。
pub fn policy_diff(a: &PackDef, b: &PackDef) -> Vec<Value> {
    let mut out = Vec::new();
    let (ka, kb) = (
        serde_json::to_value(&a.knobs).unwrap_or_default(),
        serde_json::to_value(&b.knobs).unwrap_or_default(),
    );
    for key in [
        "judge",
        "flag_patience",
        "auto_backfill",
        "consult_auto_wake",
    ] {
        knob_changes(&format!("knobs.{key}"), &ka[key], &kb[key], &mut out);
    }
    for st in &a.stages {
        let Some(other) = b.stages.iter().find(|s| s.name == st.name) else {
            continue; // 阶段增删是流程改动,不算旋钮 diff
        };
        let (sa, sb) = (
            serde_json::to_value(st).unwrap_or_default(),
            serde_json::to_value(other).unwrap_or_default(),
        );
        for f in PackDef::STAGE_KNOB_FIELDS {
            knob_changes(
                &format!("stages.{}.{}", st.name, f),
                &sa[f],
                &sb[f],
                &mut out,
            );
        }
    }
    out
}

fn diff_tree(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let keys: std::collections::BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
            for k in keys {
                diff_tree(
                    ma.get(k).unwrap_or(&Value::Null),
                    mb.get(k).unwrap_or(&Value::Null),
                    &format!("{path}.{k}"),
                    out,
                );
            }
        }
        (Value::Array(va), Value::Array(vb)) => {
            for i in 0..va.len().max(vb.len()) {
                diff_tree(
                    va.get(i).unwrap_or(&Value::Null),
                    vb.get(i).unwrap_or(&Value::Null),
                    &format!("{path}[{i}]"),
                    out,
                );
            }
        }
        _ => {
            if a != b {
                out.push(path.to_string());
            }
        }
    }
}

/// 非策略面差异（票 04/10 强制点）：剥掉旋钮字段后对剩余树做深 diff,
/// 返回叶路径集——非空即「流程定义变了」,policy-dev 提案必须被拒。
pub fn non_policy_changes(a: &PackDef, b: &PackDef) -> Vec<String> {
    let strip = |p: &PackDef| -> Value {
        let mut v = serde_json::to_value(p).unwrap_or_default();
        if let Value::Object(ref mut m) = v {
            m.remove("knobs");
        }
        if let Some(Value::Array(stages)) = v.get_mut("stages") {
            for st in stages {
                if let Value::Object(ref mut m) = st {
                    for f in PackDef::STAGE_KNOB_FIELDS {
                        m.remove(*f);
                    }
                }
            }
        }
        v
    };
    let mut out = Vec::new();
    diff_tree(&strip(a), &strip(b), "", &mut out);
    out
}

// ---------- 阶段状态 ----------

#[derive(Debug, Clone)]
pub struct StageRun {
    pub id: String,
    pub seq: i64,
    pub stage_name: String,
    pub state: String,
}

#[derive(Debug, PartialEq)]
pub enum StageEval {
    /// 未齐：缺产物 / 检验未过 / 复审未全过
    Incomplete { missing: Vec<String> },
    /// 齐了
    Ready,
}

impl Db {
    pub(crate) fn active_stage_run(&self, project_id: &str) -> rusqlite::Result<Option<StageRun>> {
        let mut st = self.conn().prepare(
            "SELECT id, seq, stage_name, state FROM stage_runs
             WHERE project_id=?1 AND state IN ('active','waiting_stamp')
             ORDER BY seq DESC LIMIT 1",
        )?;
        let mut rows = st.query_map([project_id], |r| {
            Ok(StageRun {
                id: r.get(0)?,
                seq: r.get(1)?,
                stage_name: r.get(2)?,
                state: r.get(3)?,
            })
        })?;
        rows.next().transpose()
    }
}

/// 暂停看事件流：最后一个 Paused/Resumed 决定。
pub fn is_paused(db: &Db, project_id: &str) -> Result<bool, OrchError> {
    // 两个 MAX(id) 索引探针而非 IN+ORDER BY：后者被规划器选成
    // idx_events_project 全项目倒扫（10 万事件实测 50ms/次——arch-review
    // 附录 B2；流式循环每 delta 调一次）。MAX 走 idx_events_kind
    // (project_id,kind,id) 末项定位，O(log n) 且不依赖规划器。
    let latest = |kind: &str| -> Option<i64> {
        db.conn()
            .query_row(
                "SELECT MAX(id) FROM events WHERE project_id=?1 AND kind=?2",
                rusqlite::params![project_id, kind],
                |r| r.get(0),
            )
            .ok()
            .flatten()
    };
    Ok(match (latest("paused"), latest("resumed")) {
        (Some(p), Some(r)) => p > r,
        (Some(_), None) => true,
        _ => false,
    })
}

/// 开一个阶段：建 stage_run、按名单激活/休眠、缺席判跳过。
/// 返回 (run_id, 是否被跳过)。
pub fn open_stage(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    seq: usize,
) -> Result<(String, bool), OrchError> {
    let stage = pack.stages.get(seq).ok_or(OrchError::BadSeq(seq as i64))?;
    let rid = format!("sr{}", db.next_id("sr")?);

    // 团队名单：项目 agents 表 role 集合
    let mut st = db
        .conn()
        .prepare("SELECT id, role FROM agents WHERE project_id=?1")?;
    let team: Vec<(String, String)> = st
        .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let present: Vec<&(String, String)> = team
        .iter()
        .filter(|(_, role)| stage.roles.contains(role))
        .collect();
    let skipped = present.is_empty();

    // 决策点记录（rsi-research 票 01）：激活名单的选择连同「当时可选集」
    // 落盘——回放可按 eligible/alternatives diff 决策,而非只看结果。
    // eligible = 包声明名单;chosen = 实际激活(名单∩团队);
    // alternatives = 声明但缺席的角色(被否决项+理由)。
    let roster_decision = json!({
        "kind": "roster",
        "eligible": stage.roles,
        "chosen": present.iter().map(|(_, r)| r).collect::<Vec<_>>(),
        "alternatives": stage.roles
            .iter()
            .filter(|r| !team.iter().any(|(_, tr)| tr == *r))
            .map(|r| json!({"option": r, "reason": "absent_from_team"}))
            .collect::<Vec<_>>(),
    });

    db.conn().execute(
        "INSERT INTO stage_runs (id, project_id, stage_name, seq, state, started_at)
         VALUES (?1,?2,?3,?4,?5,datetime('now'))",
        rusqlite::params![
            rid,
            project_id,
            stage.name,
            seq as i64,
            if skipped { "skipped" } else { "active" }
        ],
    )?;

    if skipped {
        db.append_event(
            project_id,
            EventKind::StageSkipped,
            json!({"stage": stage.name, "seq": seq, "reason": "no listed role in team",
                   "decision_kind": "roster", "decision": roster_decision}),
            None,
            Some(&rid),
        )?;
        return Ok((rid, true));
    }

    db.append_event(
        project_id,
        EventKind::StageStarted,
        json!({"stage": stage.name, "seq": seq, "due": stage.due,
               "stamp_point": stage.stamp_point,
               "decision_kind": "roster", "decision": roster_decision}),
        None,
        Some(&rid),
    )?;
    // 票 03：激活冻结 known world——必问卡/reviewer 拿「会话开始时」的
    // remote 基线比对，防 agent 先 remote add 再 push 显得目的地本就熟悉
    crate::provenance::snapshot_known_world(db, project_id, &rid);

    // 激活/休眠
    for (aid, role) in &team {
        let on = stage.roles.contains(role);
        write_agent_status(db, project_id, aid, !on)?;
        db.append_event(
            project_id,
            if on {
                EventKind::AgentActivated
            } else {
                EventKind::AgentSlept
            },
            json!({"role": role, "stage": stage.name,
                   "due": if on { stage.due.clone() } else { vec![] }}),
            Some(aid),
            Some(&rid),
        )?;
    }
    Ok((rid, false))
}

/// 代码产物的路径（或 kind 为「代码」）必须在仓库根有同路径文件。
/// 只有 `.hexagon/<path>` 时，这一阶段仍缺这份产物。
fn missing_artifact_files(
    db: &Db,
    project_id: &str,
    run_id: &str,
    kind: &str,
) -> Result<Vec<String>, OrchError> {
    let dir: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
                r.get(0)
            })?;
    let mut st = db.conn().prepare("SELECT path FROM artifacts WHERE project_id=?1 AND stage_run_id=?2 AND kind=?3 AND status IN ('valid','stamped')")?;
    let paths: Vec<String> = st
        .query_map(rusqlite::params![project_id, run_id, kind], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut missing = Vec::new();
    for path in paths {
        let relative = path.trim_start_matches('/');
        let mut required = vec![format!(".hexagon/{relative}")];
        if kind == "代码" || runnable_source(&path) {
            required.push(relative.to_string());
        }
        for file in required {
            // D08 / reliability 17: any(one file) previously attested a whole
            // delivery. Every required body must be readable, not just registered.
            let readable = crate::tools::readable_repo_path(Path::new(&dir), &file)
                .is_ok_and(|p| p.is_file() && std::fs::File::open(p).is_ok());
            if !readable {
                missing.push(format!("artifact:{kind}:{file}"));
            }
        }
    }
    Ok(missing)
}

use crate::artifacts::evidence::runnable_source;

// D07 / reliability 16: a pending replacement invalidates the old registered
// body too. Abandoning its unknown tool action does not attest artifact delivery.
fn materialization_gaps(
    db: &Db,
    project: &str,
    run: &str,
    due: &[String],
) -> Result<Vec<String>, OrchError> {
    let started = std::time::Instant::now();
    let mut missing = Vec::new();
    let dir: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let root = Path::new(&dir);
    let mut pending_query = db.conn().prepare("SELECT path,json_extract(intent_json,'$.resolved'),json_extract(intent_json,'$.stage'),json_extract(intent_json,'$.kind') FROM artifact_materializations WHERE project_id=?1 AND state='pending'")?;
    let pending = pending_query
        .query_map([project], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for kind in due {
        let mut query = db.conn().prepare("SELECT path FROM artifacts WHERE project_id=?1 AND stage_run_id=?2 AND kind=?3 AND status IN ('valid','stamped')")?;
        let paths = query
            .query_map(rusqlite::params![project, run, kind], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // Reliability 16 upgrade: old rows kept /x and ./x spellings. Compare
        // physical identities without rewriting historical records or merging
        // colliding version chains. An unresolvable identity cannot attest safety.
        let blocked = pending
            .iter()
            .any(|(pending_path, resolved, stage, pending_kind)| {
                (stage.as_deref() == Some(run) && pending_kind == kind)
                    || paths.iter().any(|path| {
                        path == pending_path
                            || crate::tools::repo_path(
                                root,
                                &format!(".hexagon/{}", path.trim_start_matches('/')),
                            )
                            .map_or(true, |target| target == Path::new(resolved))
                    })
            });
        if blocked {
            missing.push(format!("artifact:{kind}"));
        }
    }
    crate::diag::note(
        if missing.is_empty() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !missing.is_empty(),
        Some(project),
        None,
        Some(run),
        None,
        "artifact_materialization_gate",
        if missing.is_empty() {
            "no_pending_delivery"
        } else {
            "pending_delivery"
        },
        started,
    );
    Ok(missing)
}

/// 阶段成功判定（工作台裁决，非 Agent）。
pub fn evaluate(db: &Db, project_id: &str, pack: &PackDef) -> Result<StageEval, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    let runs = evidence_runs(db, project_id, pack, &run)?;
    let mut missing = Vec::new();
    let started = std::time::Instant::now();
    let unresolved = crate::actions::unresolved_delivery_actions(
        db,
        project_id,
        if is_final_acceptance(pack, &run) {
            None
        } else {
            Some(&run.id)
        },
    )?;
    for id in unresolved {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(project_id),
            None,
            Some(&run.id),
            None,
            "delivery_evidence",
            "unknown_action",
            started,
        );
        missing.push(format!("action:{id}"));
    }
    if is_final_acceptance(pack, &run) {
        for seq in 0..=run.seq {
            if !runs.iter().any(|r| r.seq == seq) {
                missing.push(format!("stage:{seq}"));
            }
        }
    }
    for target in runs {
        if declared_absence(db, project_id, pack, &target)? {
            continue;
        }
        if target.id != run.id && target.state != "done" {
            missing.push(format!("stage:{}", target.stage_name));
        }
        if let StageEval::Incomplete { missing: gaps } =
            evaluate_run(db, project_id, pack, &target)?
        {
            missing.extend(gaps);
        }
    }
    Ok(if missing.is_empty() {
        StageEval::Ready
    } else {
        StageEval::Incomplete { missing }
    })
}

fn is_final_acceptance(pack: &PackDef, run: &StageRun) -> bool {
    let flags: Vec<_> = pack.stages.iter().map(|s| s.stamp_point).collect();
    crate::stampgate::is_final_stamp(&flags, run.seq as usize)
}

// D08 / reliability-18: an empty final stage cannot hide earlier requirements.
// Select the latest attempt, including rejected attempts; never borrow an older
// successful run. Historical completion remains a fact, not current evidence.
fn evidence_runs(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<Vec<StageRun>, OrchError> {
    if !is_final_acceptance(pack, run) {
        return Ok(vec![run.clone()]);
    }
    let mut query = db.conn().prepare(
        "SELECT id,seq,stage_name,state FROM stage_runs s
        WHERE project_id=?1 AND seq<=?2 AND rowid=(SELECT MAX(rowid) FROM stage_runs latest
        WHERE latest.project_id=s.project_id AND latest.seq=s.seq) ORDER BY seq",
    )?;
    let runs = query
        .query_map(rusqlite::params![project, run.seq], |r| {
            Ok(StageRun {
                id: r.get(0)?,
                seq: r.get(1)?,
                stage_name: r.get(2)?,
                state: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(runs)
}

// D08 / reliability-18: legacy owner skips share the SQL state with absent
// rosters. Missing proof costs a manual repair; treating a skip as proof would
// silently accept absent deliverables. Only the original roster fact exempts.
fn declared_absence(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<bool, OrchError> {
    if run.state != "skipped" {
        return Ok(false);
    }
    use rusqlite::OptionalExtension;
    let payload: Option<String> = db.conn().query_row(
        "SELECT payload FROM events WHERE project_id=?1 AND stage_run_id=?2 AND kind='stage_skipped' ORDER BY id DESC LIMIT 1",
        rusqlite::params![project,run.id], |r| r.get(0)).optional()?;
    let stage = pack
        .stages
        .get(run.seq as usize)
        .ok_or(OrchError::BadSeq(run.seq))?;
    Ok(payload
        .and_then(|p| serde_json::from_str::<Value>(&p).ok())
        .is_some_and(|p| absent_roster_fact(&p, &stage.roles)))
}

fn absent_roster_fact(payload: &Value, roles: &[String]) -> bool {
    payload["reason"] == "no listed role in team"
        && payload["decision_kind"] == "roster"
        && payload["decision"]["kind"] == "roster"
        && payload["decision"]["chosen"] == json!([])
        && payload["decision"]["eligible"] == json!(roles)
}

fn evaluate_run(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<StageEval, OrchError> {
    let started = std::time::Instant::now();
    let accepted = exception::accepted_for_run(db, project_id, pack, &run.id)?;
    let stage = pack
        .stages
        .get(run.seq as usize)
        .ok_or(OrchError::BadSeq(run.seq))?;
    let mut missing = materialization_gaps(db, project_id, &run.id, &stage.due)?;

    // 应交产物齐（本 run 内 valid/stamped）
    for kind in &stage.due {
        if missing.contains(&format!("artifact:{kind}")) {
            continue;
        }
        let n: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM artifacts
             WHERE project_id=?1 AND stage_run_id=?2 AND kind=?3 AND status IN ('valid','stamped')",
            rusqlite::params![project_id, run.id, kind],
            |r| r.get(0),
        )?;
        if n == 0 {
            missing.push(format!("artifact:{kind}"));
            continue;
        }
        // 2026-09-24 论坛活测：index.html 只被 artifact_write 写进 .hexagon/，
        // 阶段只数种类，仓库根没有可打开的页面也算齐。代码产物要在仓库根
        // 有同路径文件（fs_write 那份）；规格仍只留在 .hexagon/。
        let files = missing_artifact_files(db, project_id, &run.id, kind)?;
        if !files.is_empty() {
            missing.push(format!("artifact:{kind}"));
            missing.extend(files);
        }
    }

    // D08: historical exit=0 requires stable, current delivery evidence.
    for cmd in failing_checks(db, project_id, &run.id, stage)? {
        if !accepted.contains(&ExceptionRequirement::Check {
            run_id: run.id.clone(),
            cmd: cmd.clone(),
        }) {
            missing.push(format!("check:{cmd}"));
        }
    }

    // D08: each required artifact needs its own current evidence. A kind-level
    // latest event (including legacy skips) cannot attest unrelated deliveries.
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
                r.get(0)
            })?;
    for rev in &stage.reviews {
        let mut query=db.conn().prepare("SELECT id,path FROM artifacts WHERE project_id=?1 AND stage_run_id=?2 AND kind=?3 AND status IN ('valid','stamped')")?;
        let targets = query
            .query_map(
                rusqlite::params![project_id, run.id, rev.artifact_kind],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if targets.is_empty() {
            missing.push(format!("review:{}", rev.artifact_kind));
        }
        for (id, path) in targets {
            // Reliability 19: an owner review exception cannot attest an
            // unfinished materialization, even while old bytes still match.
            // A false negative needs another owner decision; a false positive
            // would accept delivery that has never finished registering.
            let exception_applies =
                crate::artifacts::evidence::capture(db, Path::new(&root), project_id, &id)?
                    .is_some()
                    && accepted.contains(&ExceptionRequirement::Review {
                        run_id: run.id.clone(),
                        artifact_id: id.clone(),
                        reviewer: rev.reviewer.clone(),
                    });
            if !crate::artifacts::evidence::review_satisfied(
                db,
                Path::new(&root),
                project_id,
                &run.id,
                &id,
                &rev.reviewer,
            )? && !exception_applies
            {
                missing.push(format!("review:{}:{path}", rev.artifact_kind));
            }
        }
    }
    crate::diag::note(
        if missing.is_empty() {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !missing.is_empty(),
        Some(project_id),
        None,
        Some(&run.id),
        None,
        "delivery_evidence",
        if missing.is_empty() {
            "current"
        } else {
            "missing_or_stale"
        },
        started,
    );

    Ok(if missing.is_empty() {
        StageEval::Ready
    } else {
        StageEval::Incomplete { missing }
    })
}

/// Current check evidence. Original exit codes remain visible even when stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum CheckState {
    Missing,
    Passed,
    Failed,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct CheckEvidence {
    pub run_id: String,
    pub stage: String,
    pub cmd: String,
    #[ts(type = "number | null")]
    pub event_id: Option<i64>,
    pub exit_code: Option<i32>,
    pub state: CheckState,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct StageEvidence {
    pub run_id: String,
    pub fingerprint: Option<String>,
    pub checks: Vec<CheckEvidence>,
    pub exceptions: Vec<ExceptionCandidate>,
    pub missing: Vec<String>,
}

// D08: uncertainty is visible and fails closed. A false negative costs another
// check; a false positive would accept an unchecked delivery. Old exit=0 alone
// is historical execution evidence, never current version proof.
fn check_state(
    present: bool,
    exit: Option<i32>,
    recorded: Option<&str>,
    stable: bool,
    current: Option<&str>,
) -> CheckState {
    if !present {
        return CheckState::Missing;
    }
    if current.is_none() {
        return CheckState::Unavailable;
    }
    if !crate::artifacts::fingerprint::current(recorded, stable, current) {
        return CheckState::Stale;
    }
    if exit == Some(0) {
        CheckState::Passed
    } else {
        CheckState::Failed
    }
}

fn required_kinds(stage: &StageDef) -> Vec<String> {
    stage
        .due
        .iter()
        .chain(stage.reviews.iter().map(|r| &r.artifact_kind))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn check_evidence(
    db: &Db,
    project: &str,
    run: &str,
    stage: &StageDef,
) -> Result<Vec<CheckEvidence>, OrchError> {
    let started = std::time::Instant::now();
    use rusqlite::OptionalExtension;
    if stage.checks.is_empty() {
        return Ok(Vec::new());
    }
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let fingerprint = crate::artifacts::fingerprint::capture(
        db,
        Path::new(&root),
        project,
        run,
        &required_kinds(stage),
    )
    .ok();
    let mut rows = Vec::new();
    for cmd in &stage.checks {
        let latest: Option<(i64,Option<i32>,Option<String>,bool)> = db.conn().query_row(
            "SELECT id,json_extract(payload,'$.exit_code'),json_extract(payload,'$.fingerprint'),COALESCE(json_extract(payload,'$.stable'),0)
             FROM events WHERE project_id=?1 AND stage_run_id=?2 AND kind='test_ran' AND json_extract(payload,'$.cmd')=?3 ORDER BY id DESC LIMIT 1",
            rusqlite::params![project,run,cmd], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (event_id, exit_code, recorded, stable) = latest
            .map(|(id, exit, fp, stable)| (Some(id), exit, fp, stable))
            .unwrap_or_default();
        let state = check_state(
            event_id.is_some(),
            exit_code,
            recorded.as_deref(),
            stable,
            fingerprint.as_deref(),
        );
        let reason = match state {
            CheckState::Missing => "missing",
            CheckState::Passed => "current_pass",
            CheckState::Failed => "current_fail",
            CheckState::Stale => "stale",
            CheckState::Unavailable => "unavailable",
        };
        crate::diag::note(
            if state == CheckState::Passed {
                crate::diag::CLASS_JUDGE
            } else {
                crate::diag::CLASS_REJECT
            },
            state != CheckState::Passed,
            Some(project),
            None,
            Some(run),
            event_id.map(|id| id.to_string()).as_deref(),
            "check_evidence",
            reason,
            started,
        );
        rows.push(CheckEvidence {
            run_id: run.into(),
            stage: stage.name.clone(),
            cmd: cmd.clone(),
            event_id,
            exit_code,
            state,
        });
    }
    Ok(rows)
}

fn failing_checks(
    db: &Db,
    project: &str,
    run: &str,
    stage: &StageDef,
) -> Result<Vec<String>, OrchError> {
    Ok(check_evidence(db, project, run, stage)?
        .into_iter()
        .filter(|c| c.state != CheckState::Passed)
        .map(|c| c.cmd)
        .collect())
}

fn delivery_fingerprint(
    db: &Db,
    project: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<Option<String>, OrchError> {
    use sha2::{Digest, Sha256};
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let mut inputs = Vec::new();
    for target in evidence_runs(db, project, pack, run)? {
        let stage = pack
            .stages
            .get(target.seq as usize)
            .ok_or(OrchError::BadSeq(target.seq))?;
        let Ok(fingerprint) = crate::artifacts::fingerprint::capture(
            db,
            Path::new(&root),
            project,
            &target.id,
            &required_kinds(stage),
        ) else {
            return Ok(None);
        };
        inputs.push((target.id, fingerprint));
    }
    Ok(Some(format!(
        "v1:{:x}",
        Sha256::digest(serde_json::to_vec(&(pack, inputs))?)
    )))
}

/// Read-only seam: checks and gate use the same classifier, not a UI inference.
pub fn stage_evidence(
    db: &Db,
    project: &str,
    pack: &PackDef,
) -> Result<Option<StageEvidence>, OrchError> {
    let Some(run) = db.active_stage_run(project)? else {
        return Ok(None);
    };
    let before = delivery_fingerprint(db, project, pack, &run)?;
    let mut checks = Vec::new();
    for target in evidence_runs(db, project, pack, &run)? {
        if declared_absence(db, project, pack, &target)? {
            continue;
        }
        let stage = pack
            .stages
            .get(target.seq as usize)
            .ok_or(OrchError::BadSeq(target.seq))?;
        checks.extend(check_evidence(db, project, &target.id, stage)?);
    }
    let mut missing = match evaluate(db, project, pack)? {
        StageEval::Ready => Vec::new(),
        StageEval::Incomplete { missing } => missing,
    };
    let exceptions = exception::candidates(db, project, pack, &run)?;
    let after = delivery_fingerprint(db, project, pack, &run)?;
    let fingerprint =
        if crate::artifacts::fingerprint::current(before.as_deref(), true, after.as_deref()) {
            after
        } else {
            for check in &mut checks {
                if check.state == CheckState::Passed {
                    check.state = CheckState::Stale;
                }
            }
            missing.push("delivery:changed_or_unavailable".into());
            None
        };
    Ok(Some(StageEvidence {
        run_id: run.id,
        fingerprint,
        checks,
        exceptions,
        missing,
    }))
}

/// 负责人显式覆盖检验失败（票 40）：落 check_overridden 留痕（谁/哪些命令/理由），
/// 覆盖检验回执（ADR 0054）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct OverrideOutcome {
    pub overridden: Vec<String>,
    pub stage: String,
}

/// Reliability 19: require an explicit, version-bound owner decision instead.
pub fn override_checks(
    _db: &Db,
    _project_id: &str,
    _pack: &PackDef,
    _reason: &str,
) -> Result<OverrideOutcome, OrchError> {
    Err(OrchError::InvalidAcceptanceException)
}

/// 检验结果行（ADR 0054）：cmd + 退出码。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct CheckResult {
    pub cmd: String,
    pub exit_code: i32,
}

/// run_checks 回执（ADR 0054）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct CheckOutcome {
    pub results: Vec<CheckResult>,
}

/// 跑本阶段的检验命令（包内声明 = 预授权，直跑不过权限管线）。
// D08: the same OS lease used by structured tools spans evidence capture,
// execution and persistence, and final revalidation through acceptance writes.
fn write_boundary(db: &Db, project: &str) -> Result<std::fs::File, OrchError> {
    let dir: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    Ok(crate::tools::writeguard::repository_lock(
        &crate::tools::ToolContext {
            project_id: project.into(),
            agent_id: "owner".into(),
            repo_root: dir.into(),
            ..Default::default()
        },
    )?)
}

pub fn run_checks(
    db: &Db,
    project_id: &str,
    repo_root: &Path,
    pack: &PackDef,
) -> Result<Vec<CheckResult>, OrchError> {
    let lease = std::sync::Arc::new(write_boundary(db, project_id)?);
    let sessions = crate::sessions::SessionTable::default();
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    let mut out = Vec::new();
    for run in evidence_runs(db, project_id, pack, &run)? {
        if declared_absence(db, project_id, pack, &run)? {
            continue;
        }
        let stage = pack
            .stages
            .get(run.seq as usize)
            .ok_or(OrchError::BadSeq(run.seq))?;
        for cmd in &stage.checks {
            let before = crate::artifacts::fingerprint::capture(
                db,
                repo_root,
                project_id,
                &run.id,
                &required_kinds(stage),
            )
            .ok();
            let ctx = crate::tools::ToolContext {
                project_id: project_id.into(),
                agent_id: "owner".into(),
                stage_run_id: Some(run.id.clone()),
                repo_root: repo_root.into(),
                write_lease: Some(lease.clone()),
                ..Default::default()
            };
            // D08: raw sh.output() returned while redirected background children
            // were still writing. Use the existing owned, confined process tree.
            let res = sessions
                .run_oneshot(db, &ctx, cmd, std::time::Duration::from_secs(300), true)
                .map_err(std::io::Error::other)?;
            let code = if res["timed_out"] == true {
                -1
            } else {
                res["exit_code"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .unwrap_or(-1)
            };
            let after = crate::artifacts::fingerprint::capture(
                db,
                repo_root,
                project_id,
                &run.id,
                &required_kinds(stage),
            )
            .ok();
            let stable =
                crate::artifacts::fingerprint::current(before.as_deref(), true, after.as_deref());
            db.append_event(
            project_id,
            EventKind::TestRan,
            json!({"cmd": cmd, "exit_code": code, "fingerprint": before, "stable": stable,
                   "stdout": res["stdout"].as_str().unwrap_or("").chars().take(2000).collect::<String>()}),
            None,
            Some(&run.id),
        )?;
            out.push(CheckResult {
                cmd: cmd.clone(),
                exit_code: code,
            });
        }
    }
    Ok(out)
}

/// 阶段动作回执（ADR 0054）：serde(tag="action") 标号联合——
/// wire 形状与旧 json!({"action":...}) 完全一致。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[derive(ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum StageAction {
    /// 就绪但卡在盖章点（已发 stamp 卡）。
    AwaitingStamp { stage: String, question_id: String },
    /// 已 waiting_stamp——重复推进的幂等回执。
    WaitingStamp { stage: String },
    /// 就绪条件不齐：缺产物/检验/复审。
    Incomplete { stage: String, missing: Vec<String> },
    /// 开了下一阶段。
    StageOpened {
        run_id: String,
        #[ts(type = "number")]
        seq: usize,
    },
    /// 包全部跑完。
    PackFinished,
    /// 退回到指定 seq 并开新 run。
    Rewound {
        #[ts(type = "number")]
        to_seq: usize,
        run_id: String,
    },
    /// 盖章点驳回：退上一阶段（review::reject_stamp 同回执）。
    StampRejected {
        #[ts(type = "number")]
        reopened_seq: usize,
        run_id: String,
    },
}

/// 推进：评估当前阶段 → ready 则盖章点停 or done+开下一阶段。
/// 返回发生了什么的描述。
pub fn advance(db: &Db, project_id: &str, pack: &PackDef) -> Result<StageAction, OrchError> {
    let _lease = write_boundary(db, project_id)?;
    // D08: the file lease and an immediate SQLite transaction cover the whole
    // verdict. A failed event/card write previously left a completed stage.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let result = (|| {
        if is_paused(db, project_id)? {
            return Err(OrchError::Paused);
        }
        let run = db
            .active_stage_run(project_id)?
            .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
        if run.state == "waiting_stamp" {
            return Ok(StageAction::WaitingStamp {
                stage: run.stage_name,
            });
        }
        match evaluate(db, project_id, pack)? {
            StageEval::Incomplete { missing } => Ok(StageAction::Incomplete {
                stage: run.stage_name,
                missing,
            }),
            StageEval::Ready => {
                let stage = &pack.stages[run.seq as usize];
                if stage.stamp_point {
                    let flags: Vec<bool> = pack.stages.iter().map(|s| s.stamp_point).collect();
                    let seq = run.seq as usize;
                    // 存储档，不是 execution_rank。提案/授权/安装走 harnessgate（票 04）。
                    let stored = crate::autonomy::rank(db, project_id)?;
                    if crate::stampgate::classify_stamp(stored, &flags, seq)
                        == crate::stampgate::StampDisposition::AutoPass
                    {
                        return auto_pass_stamp(db, project_id, pack, &run);
                    }
                    let final_acceptance = crate::stampgate::is_final_stamp(&flags, seq);
                    db.conn().execute(
                        "UPDATE stage_runs SET state='waiting_stamp' WHERE id=?1",
                        [&run.id],
                    )?;
                    // 卡表写口归 cards.rs（arch-review 票 04）；阶段盖章卡无 agent
                    let qid = crate::cards::enqueue(
                        db,
                        project_id,
                        None,
                        crate::cards::CardKind::Stamp,
                        json!({
                            "stage": stage.name,
                            "run_id": run.id,
                            "final_acceptance": final_acceptance,
                        }),
                        None,
                    )?;
                    db.append_event(
                        project_id,
                        EventKind::PermissionAsked,
                        json!({"kind": "stamp", "stage": stage.name, "question_id": qid}),
                        None,
                        Some(&run.id),
                    )?;
                    return Ok(StageAction::AwaitingStamp {
                        stage: stage.name.clone(),
                        question_id: qid,
                    });
                }
                finish_stage(db, project_id, pack, &run)
            }
        }
    })()?;
    tx.commit()?;
    Ok(result)
}

fn finish_stage(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<StageAction, OrchError> {
    db.conn().execute(
        "UPDATE stage_runs SET state='done', finished_at=datetime('now') WHERE id=?1",
        [&run.id],
    )?;
    db.append_event(
        project_id,
        EventKind::StageFinished,
        json!({"stage": run.stage_name, "seq": run.seq}),
        None,
        Some(&run.id),
    )?;
    open_next(db, project_id, pack, run.seq as usize + 1)
}

/// 开下一个阶段；跳过是即时的——连续穿到第一个非跳过阶段或跑完。
pub fn open_next(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    from_seq: usize,
) -> Result<StageAction, OrchError> {
    let mut seq = from_seq;
    loop {
        if seq >= pack.stages.len() {
            // 全程跑完：全员休眠
            write_team_sleeping(db, project_id)?;
            db.append_event(
                project_id,
                EventKind::TeamSlept,
                json!({"reason": "pack finished"}),
                None,
                None,
            )?;
            return Ok(StageAction::PackFinished);
        }
        let (rid, skipped) = open_stage(db, project_id, pack, seq)?;
        if !skipped {
            return Ok(StageAction::StageOpened { run_id: rid, seq });
        }
        seq += 1;
    }
}

/// 盖章确认：waiting_stamp → done → 推进。
pub fn stamp(db: &Db, project_id: &str, pack: &PackDef) -> Result<StageAction, OrchError> {
    let _lease = write_boundary(db, project_id)?;
    // D08: the file lease and an immediate SQLite transaction cover the whole
    // verdict. A failed event/card write previously left a completed stage.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let result = (|| {
        let run = db
            .active_stage_run(project_id)?
            .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
        if run.state != "waiting_stamp" {
            return Err(OrchError::BadSeq(run.seq));
        }
        // D08: waiting for the owner does not freeze files or review evidence.
        let evidence = stage_evidence(db, project_id, pack)?
            .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
        if !evidence.missing.is_empty() {
            return Ok(StageAction::Incomplete {
                stage: run.stage_name,
                missing: evidence.missing,
            });
        }
        db.conn().execute(
            "UPDATE stage_runs SET state='done', finished_at=datetime('now') WHERE id=?1",
            [&run.id],
        )?;
        // 销掉本 run 的盖章卡（e2e_live 实证：卡曾永不销——queued 僵尸卡
        // 在已推进阶段后仍挂待决区。提案型 stamp 卡 payload 无 run_id，
        // 按 run_id 匹配天然不碰提案面；提案卡归 proposals::activate 销）。
        crate::cards::answer_queued_where(
            db,
            project_id,
            crate::cards::CardKind::Stamp,
            "run_id",
            &run.id,
            "owner",
        )?;
        db.append_event(
            project_id,
            EventKind::Stamped,
            // 票 02：by=owner 与自动通过的 by=autonomy 区分。缺 by 的旧事件当人工。
            json!({"stage": run.stage_name, "seq": run.seq, "by": "owner", "delivery_fingerprint": evidence.fingerprint}),
            None,
            Some(&run.id),
        )?;
        open_next(db, project_id, pack, run.seq as usize + 1)
    })()?;
    tx.commit()?;
    Ok(result)
}

/// L3/L4 非最终盖章点：不入待决卡，直接通过并留下和人工盖章不同的轨迹。
fn auto_pass_stamp(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<StageAction, OrchError> {
    db.conn().execute(
        "UPDATE stage_runs SET state='done', finished_at=datetime('now') WHERE id=?1",
        [&run.id],
    )?;
    db.append_event(
        project_id,
        EventKind::Stamped,
        json!({"stage": run.stage_name, "seq": run.seq, "by": "autonomy"}),
        None,
        Some(&run.id),
    )?;
    open_next(db, project_id, pack, run.seq as usize + 1)
}

/// 退回：当前 run 标 rejected，目标 seq 开新 run。
pub fn rewind(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    to_seq: usize,
) -> Result<StageAction, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    if to_seq >= pack.stages.len() || to_seq as i64 >= run.seq {
        return Err(OrchError::BadSeq(to_seq as i64));
    }
    db.conn().execute(
        "UPDATE stage_runs SET state='rejected', finished_at=datetime('now') WHERE id=?1",
        [&run.id],
    )?;
    db.append_event(
        project_id,
        EventKind::StageRewound,
        json!({"from_stage": run.stage_name, "from_seq": run.seq, "to_seq": to_seq}),
        None,
        Some(&run.id),
    )?;
    let (rid, _) = open_stage(db, project_id, pack, to_seq)?;
    Ok(StageAction::Rewound {
        to_seq,
        run_id: rid,
    })
}

/// Reliability 19: general stage skipping cannot stand in for delivery evidence.
pub fn skip(_db: &Db, _project_id: &str, _pack: &PackDef) -> Result<StageAction, OrchError> {
    Err(OrchError::InvalidAcceptanceException)
}

pub fn pause(db: &Db, project_id: &str) -> Result<(), OrchError> {
    db.append_event(project_id, EventKind::Paused, json!({}), None, None)?;
    Ok(())
}
pub fn resume(db: &Db, project_id: &str) -> Result<(), OrchError> {
    db.append_event(project_id, EventKind::Resumed, json!({}), None, None)?;
    Ok(())
}

// ---------- 名册/项目读模型与休眠控制（ADR 0052 控制通道） ----------
// 这组函数只吃 &Db：壳层读组/控制组命令经第二条连接直调，不占 wb 锁。

/// 团队名册（agents 表读模型）。
/// 团队花名册行（ADR 0054）：agents 读模型，IPC 直出。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct TeamRow {
    pub id: String,
    pub role: String,
    pub model_slot: Option<String>,
    #[ts(type = "'active' | 'sleeping'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub status: String,
    /// 头像内容哈希（票 07）：随行下发，UI 哈希变了才拉 data URL。
    pub avatar_hash: Option<String>,
}

pub fn team(
    db: &Db,
    project_id: &str,
    repo_root: &std::path::Path,
) -> Result<Vec<TeamRow>, OrchError> {
    let mut st = db
        .conn()
        .prepare("SELECT id, role, model_slot, status FROM agents WHERE project_id=?1")?;
    let mut rows = st
        .query_map([project_id], |r| {
            Ok(TeamRow {
                id: r.get(0)?,
                role: r.get(1)?,
                model_slot: r.get(2)?,
                status: r.get(3)?,
                avatar_hash: None,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for row in &mut rows {
        // 头像 IO 失败不炸团队接口——哈希缺席=UI 当无头像处理
        // （沿用 avatar 读路 `.catch(() => null)` 的静默降级语义）。
        row.avatar_hash = crate::roles::avatar_hash(repo_root, &row.id).ok().flatten();
    }
    Ok(rows)
}

/// 阶段运行状态行（stage_runs 读模型）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct StageRow {
    pub run_id: String,
    pub stage: String,
    #[ts(type = "number")] // JS number 域（wire 上是 JSON number，ts-rs 默认 bigint 不符 wire）
    pub seq: i64,
    #[ts(
        type = "'pending' | 'active' | 'done' | 'skipped' | 'waiting_stamp' | 'rejected' | 'interrupted'"
    )] // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub state: String,
}

/// 阶段运行状态表（stage_runs 读模型 + pending 投影）。
///
/// 活测实证（桌面真窗口跑向导）：pack 项目建档后 stage_runs 为空，
/// 阶段条拿不到任何 pending 行 → 「开阶段」按钮永远不渲染，owner
/// 没有途径开首阶段——`pending` 词表在写侧从没人写过（dead vocab）。
/// 修法选型：pending 不落库，只做读模型投影——stage_runs 保持
/// 「真实发生过的 run」的事件溯源语义；包声明了但尚无 run 的 seq
/// 合成一行 pending（run_id 用占位串，UI 只拿它当列表 key）。
/// 曾考虑建档时预写 pending 行——否决：open_stage 的 INSERT 会与之
/// 撞 seq，且 rewind 依赖同 seq 多行历史。
pub fn stage_status(db: &Db, project_id: &str) -> Result<Vec<StageRow>, OrchError> {
    let mut st = db.conn().prepare(
        "SELECT id, stage_name, seq, state FROM stage_runs WHERE project_id=?1 ORDER BY seq, id",
    )?;
    let mut rows = st
        .query_map([project_id], |r| {
            Ok(StageRow {
                run_id: r.get(0)?,
                stage: r.get(1)?,
                seq: r.get(2)?,
                state: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // pending 投影：仅 pack 模式有意义；钉盘副本缺失/非 pack 时静默跳过
    // （读模型投影失败不该拖死整个状态查询——阶段条少几个待开 chip
    // 总比整面读不出强，fail-open 偏向可用性）。
    let dir_mode = db.conn().query_row(
        "SELECT dir, mode FROM projects WHERE id=?1",
        [project_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    if let Ok((dir, mode)) = dir_mode {
        if mode == "pack" {
            if let Ok(pack) = PackDef::pinned(Path::new(&dir)) {
                let have: std::collections::HashSet<i64> = rows.iter().map(|r| r.seq).collect();
                for (i, s) in pack.stages.iter().enumerate() {
                    let seq = i as i64;
                    if !have.contains(&seq) {
                        rows.push(StageRow {
                            run_id: format!("pending:{seq}"),
                            stage: s.name.clone(),
                            seq,
                            state: "pending".into(),
                        });
                    }
                }
                // stable sort：同 seq 的历史 run（rewind 遗留）保持 id 序
                rows.sort_by_key(|r| r.seq);
            }
        }
    }
    Ok(rows)
}

/// 项目元信息（projects 行 + fastpath 角色名解析）。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ProjectInfo {
    pub name: String,
    #[ts(type = "'pack' | 'fastpath'")]
    // schema CHECK 词表钉死（migrations/*.sql / CardKind::as_str）
    pub mode: String,
    pub pack_name: Option<String>,
    pub fastpath_agent_id: Option<String>,
    pub fastpath_role: Option<String>,
}

pub fn project_info(db: &Db, project_id: &str) -> Result<ProjectInfo, OrchError> {
    let (name, mode, pack_name, fast_aid): (String, String, Option<String>, Option<String>) =
        db.conn().query_row(
            "SELECT name, mode, pack_name, fastpath_agent_id FROM projects WHERE id=?1",
            [project_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    let fast_role: Option<String> = fast_aid.as_ref().and_then(|a| {
        db.conn()
            .query_row("SELECT role FROM agents WHERE id=?1", [a], |r| r.get(0))
            .ok()
    });
    Ok(ProjectInfo {
        name,
        mode,
        pack_name,
        fastpath_agent_id: fast_aid,
        fastpath_role: fast_role,
    })
}

/// agents.status 的唯一写口（arch-review 票 04）：裸状态迁移，**不附事件**——
/// 事件语义归各调用场景（owner 干预 TeamSlept/AgentSlept、usage cap 的
/// UsageCapHit、会诊唤醒 ConsultWakeup、点名唤醒 AgentActivated{by:dispatch}）。
/// 薄包装只转 SQL 错误，故返回 rusqlite::Error 而非 OrchError。
pub fn write_agent_status(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    sleeping: bool,
) -> Result<(), rusqlite::Error> {
    db.conn().execute(
        "UPDATE agents SET status=?1 WHERE id=?2 AND project_id=?3",
        rusqlite::params![
            if sleeping { "sleeping" } else { "active" },
            agent_id,
            project_id
        ],
    )?;
    Ok(())
}

/// 全项目休眠裸写（usage cap 与 sleep_all 共用）。
pub fn write_team_sleeping(db: &Db, project_id: &str) -> Result<(), rusqlite::Error> {
    db.conn().execute(
        "UPDATE agents SET status='sleeping' WHERE project_id=?1",
        [project_id],
    )?;
    Ok(())
}

/// 全员休眠（干预指令：回合进行中也要能落，故走控制通道）。
pub fn sleep_all(db: &Db, project_id: &str) -> Result<(), OrchError> {
    write_team_sleeping(db, project_id)?;
    db.append_event(
        project_id,
        EventKind::TeamSlept,
        json!({"by": "owner"}),
        None,
        None,
    )?;
    Ok(())
}

/// 单个 Agent 休眠/唤醒（干预指令，同上走控制通道）。
pub fn set_agent_sleeping(
    db: &Db,
    project_id: &str,
    agent_id: &str,
    sleeping: bool,
) -> Result<(), OrchError> {
    write_agent_status(db, project_id, agent_id, sleeping)?;
    db.append_event(
        project_id,
        if sleeping {
            EventKind::AgentSlept
        } else {
            EventKind::AgentActivated
        },
        json!({"by": "owner"}),
        Some(agent_id),
        None,
    )?;
    Ok(())
}

// ---------- 崩溃恢复（票 37） ----------

/// 重开检出中断回合：每个 (run, agent) 看最后一条回合边界事件——
/// 若是 turn_started 即进程被杀时的盘上痕迹。闭环轨迹（补 turn_failed）、
/// run 标 interrupted、恢复卡入队。幂等：边界已闭，再开零检出。
pub fn detect_interrupted(db: &Db, project_id: &str) -> Result<u32, OrchError> {
    let mut st = db.conn().prepare(
        "SELECT stage_run_id, agent_id FROM events e
         WHERE e.project_id = ?1 AND e.stage_run_id IS NOT NULL
           AND e.kind = 'turn_started'
           AND e.id = (
             SELECT MAX(id) FROM events
             WHERE project_id = e.project_id
               AND stage_run_id IS e.stage_run_id
               AND agent_id IS e.agent_id
               AND kind IN ('turn_started','turn_finished','turn_failed'))",
    )?;
    let dangling: Vec<(String, Option<String>)> = st
        .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut n = 0;
    for (run_id, agent_id) in dangling {
        // 只收编仍 active 的 run；waiting_stamp/done 等已收束态不动
        let state: Option<String> = db
            .conn()
            .query_row(
                "SELECT state FROM stage_runs WHERE id=?1 AND project_id=?2",
                rusqlite::params![run_id, project_id],
                |r| r.get(0),
            )
            .ok();
        if state.as_deref() != Some("active") {
            continue;
        }
        let stage: String = db.conn().query_row(
            "SELECT stage_name FROM stage_runs WHERE id=?1",
            [&run_id],
            |r| r.get(0),
        )?;
        db.append_event(
            project_id,
            EventKind::TurnFailed,
            json!({"reason": "interrupted_shutdown"}),
            agent_id.as_deref(),
            Some(&run_id),
        )?;
        db.conn().execute(
            "UPDATE stage_runs SET state='interrupted' WHERE id=?1",
            [&run_id],
        )?;
        // 票 NR-03：卡载荷带 reason——「进程被杀」与「断网超时」的恢复卡
        // 在 UI/重触发路径上要能区分。
        let _qid = crate::cards::enqueue(
            db,
            project_id,
            agent_id.as_deref(),
            crate::cards::CardKind::Recovery,
            json!({"run_id": run_id, "stage": stage, "reason": "interrupted_shutdown"}),
            None,
        )?;
        n += 1;
    }
    Ok(n)
}

/// 负责人按「继续」：interrupted run 回 active、恢复卡销、落 Resumed。
/// 不重放模型调用——回合上下文已随进程死，下一步由人发起。
pub fn recover_run(db: &Db, project_id: &str, run_id: &str) -> Result<(), OrchError> {
    let state: String = db.conn().query_row(
        "SELECT state FROM stage_runs WHERE id=?1 AND project_id=?2",
        rusqlite::params![run_id, project_id],
        |r| r.get(0),
    )?;
    if state != "interrupted" {
        return Err(OrchError::NotInterrupted(run_id.into()));
    }
    db.conn()
        .execute("UPDATE stage_runs SET state='active' WHERE id=?1", [run_id])?;
    // 销掉该 run 的全部 queued 恢复卡（卡表写口归 cards.rs，票 04）
    crate::cards::answer_queued_where(
        db,
        project_id,
        crate::cards::CardKind::Recovery,
        "run_id",
        run_id,
        "owner",
    )?;
    db.append_event(
        project_id,
        EventKind::Resumed,
        json!({"run_id": run_id, "reason": "crash_recovery"}),
        None,
        Some(run_id),
    )?;
    Ok(())
}

/// 断网等网超时挂起（network-resilience 票 01）：run → interrupted +
/// 恢复卡（reason=network_timeout）+ 系统事件。与 detect_interrupted
/// 共用 interrupted 终态和卡型——挂起不是第三种 run 状态词：恢复语义
/// 与进程被杀相同（解锁→重触发，票 NR-03），分开造词只会让恢复面分叉。
/// 幂等：run 已非 active 时不动状态也不补卡。
pub fn suspend_run(
    db: &Db,
    project_id: &str,
    run_id: &str,
    agent_id: &str,
) -> Result<(), OrchError> {
    let state: String = db.conn().query_row(
        "SELECT state FROM stage_runs WHERE id=?1 AND project_id=?2",
        rusqlite::params![run_id, project_id],
        |r| r.get(0),
    )?;
    if state != "active" {
        return Ok(());
    }
    let stage: String = db.conn().query_row(
        "SELECT stage_name FROM stage_runs WHERE id=?1",
        [run_id],
        |r| r.get(0),
    )?;
    db.conn().execute(
        "UPDATE stage_runs SET state='interrupted' WHERE id=?1",
        [run_id],
    )?;
    let _qid = crate::cards::enqueue(
        db,
        project_id,
        Some(agent_id),
        crate::cards::CardKind::Recovery,
        json!({"run_id": run_id, "stage": stage, "reason": "network_timeout"}),
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::System,
        json!({"kind": "run_suspended", "run_id": run_id, "reason": "network_timeout"}),
        Some(agent_id),
        Some(run_id),
    )?;
    Ok(())
}

/// 恢复卡携带的归属 agent（票 NR-03 重触发的读口）：detect_interrupted
/// 与 suspend_run 都把中断时的 agent 写进卡 agent_id。None = 悬空卡
/// （解锁但不重触发）。
pub fn recovery_agent(
    db: &Db,
    project_id: &str,
    run_id: &str,
) -> Result<Option<String>, OrchError> {
    use rusqlite::OptionalExtension;
    Ok(db
        .conn()
        .query_row(
            "SELECT agent_id FROM pending_questions
             WHERE project_id=?1 AND kind='recovery' AND state='queued'
               AND json_extract(payload,'$.run_id')=?2
             ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![project_id, run_id],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> PackDef {
        serde_json::from_value(json!({
            "name": "规格驱动", "version": 3,
            "stages": [
                {"name": "规格", "roles": ["产品策划"], "due": ["规格"], "stamp_point": true},
                {"name": "界面", "roles": ["UI"], "due": ["界面稿"]},
                {"name": "接口", "roles": ["后端","架构师"], "due": ["接口说明"],
                 "reviews": [{"artifact_kind": "接口说明", "reviewer": "架构师"}],
                 "backfill_edges": [["前端","UI"]], "consult_wake": ["UX"]},
                {"name": "实现", "roles": ["前端","后端"], "due": ["代码"],
                 "checks": ["cargo test --quiet"]},
                {"name": "合入", "roles": ["运维"], "due": [], "stamp_point": true}
            ]
        }))
        .unwrap()
    }

    fn setup(roles: &[&str]) -> (Db, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1',?1,'x','pack')",
                [dir.path().to_string_lossy().as_ref()],
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
        (db, dir)
    }

    #[test]
    fn missing_role_stage_skipped() {
        let (db, _d) = setup(&["产品策划", "后端", "架构师"]); // 没有 UI
        let p = pack();
        let (_rid, skipped) = open_stage(&db, "p1", &p, 1).unwrap();
        assert!(skipped);
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StageSkipped]))
            .unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn open_stage_activates_listed_sleeps_rest() {
        let (db, _d) = setup(&["产品策划", "后端", "架构师"]);
        let p = pack();
        open_stage(&db, "p1", &p, 0).unwrap();
        let on: Vec<String> = {
            let mut st = db
                .conn()
                .prepare("SELECT role FROM agents WHERE status='active'")
                .unwrap();
            st.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(on, vec!["产品策划"]);
    }

    #[test]
    fn eval_needs_artifacts_reviews_checks() {
        let (db, dir) = setup(&["产品策划", "后端", "架构师"]);
        let p = pack();
        // 跳到「接口」阶段（直接开 seq2）
        let (rid, _) = open_stage(&db, "p1", &p, 2).unwrap();
        match evaluate(&db, "p1", &p).unwrap() {
            StageEval::Incomplete { missing } => {
                assert!(missing.iter().any(|m| m.starts_with("artifact:")));
                assert!(missing.iter().any(|m| m.starts_with("review:")));
            }
            _ => panic!("should be incomplete"),
        }
        // D08: actual bodies and target-bound reviews replace the old fake
        // kind-only event fixture; a row alone no longer attests delivery.
        let ctx = crate::tools::ToolContext {
            project_id: "p1".into(),
            agent_id: "a1".into(),
            repo_root: dir.path().into(),
            stage_run_id: Some(rid.clone()),
            ..Default::default()
        };
        let aid = crate::artifacts::deliver(
            &db,
            &ctx,
            &Default::default(),
            "specs/api.md",
            "---\nkind: 接口说明\nauthor: a1\n---\n## 资源\nx\n## 端点\nx\n## 错误码\nx",
            None,
        )
        .unwrap();
        let reviewer = crate::tools::ToolContext {
            agent_id: "a2".into(),
            ..ctx
        };
        crate::review::submit_review(
            &db,
            &reviewer,
            &aid,
            crate::review::Verdict::Pass,
            "reviewed",
        )
        .unwrap();
        assert_eq!(evaluate(&db, "p1", &p).unwrap(), StageEval::Ready);
        // advance → done + 开实现阶段
        let r = serde_json::to_value(advance(&db, "p1", &p).unwrap()).unwrap();
        assert_eq!(r["action"], "stage_opened");
        assert_eq!(r["seq"], 3);
        let _ = dir;
    }

    #[test]
    fn stamp_point_stops_until_stamped() {
        let (db, dir) = setup(&["产品策划", "后端", "架构师"]);
        // 规格不是最后一道盖章点。离开时按原先 L4 自动通过，界面无 UI 被跳过，开到接口。
        let p = pack();
        let (rid, _) = open_stage(&db, "p1", &p, 0).unwrap();
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
             VALUES ('x','p1','specs/prd.md','规格','skeleton',?1,1,'valid')",
                [&rid],
            )
            .unwrap();
        // D08: registration must have a readable current body.
        std::fs::create_dir_all(dir.path().join(".hexagon/specs")).unwrap();
        std::fs::write(dir.path().join(".hexagon/specs/prd.md"), "spec").unwrap();
        let r = serde_json::to_value(advance(&db, "p1", &p).unwrap()).unwrap();
        assert_eq!(r["action"], "stage_opened");
        assert_eq!(r["seq"], 2);
        let st: String = db
            .conn()
            .query_row("SELECT state FROM stage_runs WHERE seq=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "skipped");
        // 回归（e2e_live 实证）：stamp 必须销掉本 run 的盖章卡——此前卡
        // 永挂 queued，已推进阶段后待决区仍见僵尸「阶段盖章」卡。
        let zombie: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM pending_questions
                 WHERE project_id='p1' AND kind='stamp' AND state='queued'
                   AND json_extract(payload,'$.run_id')=?1",
                [&rid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(zombie, 0, "stamp 后盖章卡必须销（曾永挂 queued）");
    }

    /// 回归（桌面真窗口活测）：pack 项目建档后 stage_runs 为空 →
    /// stage_status 无行 → 阶段条渲染不出「开阶段」按钮，owner 无路
    /// 开首阶段（pending 词表在写侧从没人写过）。修法：pending 合成
    /// 为读模型投影，不落库；开跑后投影被真实 run 行替换。
    #[test]
    fn stage_status_projects_pending_for_unrun_stages() {
        let (db, d) = setup(&["产品策划", "后端", "架构师", "运维"]);
        let p = pack();
        // 投影数据源是钉盘副本——把 projects.dir 指到真目录并 pin
        let dir = d.path().join("proj");
        p.pin(&dir).unwrap();
        db.conn()
            .execute(
                "UPDATE projects SET dir=?1 WHERE id='p1'",
                [dir.to_string_lossy().as_ref()],
            )
            .unwrap();
        // 无任何 run：全部 5 阶段投影为 pending（首个即 UI 的开阶段按钮目标）
        let rows = stage_status(&db, "p1").unwrap();
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|r| r.state == "pending"));
        assert_eq!(rows[0].stage, "规格");
        assert_eq!(rows[0].seq, 0);
        // 开 seq0：投影让位真实行，未跑 seq 仍 pending，按 seq 排序
        open_stage(&db, "p1", &p, 0).unwrap();
        let rows = stage_status(&db, "p1").unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].state, "active");
        assert_eq!(rows[0].stage, "规格");
        assert!(rows[1..].iter().all(|r| r.state == "pending"));
        assert_eq!(rows[4].stage, "合入");
        // 快通道/无钉盘：不投影（mode 闸 + pinned 失败静默跳过）
        db.conn()
            .execute("UPDATE projects SET mode='fastpath' WHERE id='p1'", [])
            .unwrap();
        let rows = stage_status(&db, "p1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "active");
    }

    /// 2026-09-24 论坛活测：index.html 只在 .hexagon/ 里，阶段仍算缺这份代码。
    /// 规格类 markdown 不要求仓库根另有一份。
    #[test]
    fn code_artifact_without_repo_file_stays_missing() {
        let (db, dir) = setup(&["前端"]);
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".hexagon")).unwrap();
        db.conn()
            .execute(
                "UPDATE projects SET dir=?1 WHERE id='p1'",
                [root.to_string_lossy().as_ref()],
            )
            .unwrap();
        let p: PackDef = serde_json::from_value(json!({
            "name": "论坛", "version": 1,
            "stages": [{"name": "实现", "roles": ["前端"], "due": ["HTML/CSS/JS源码"]}]
        }))
        .unwrap();
        let (rid, _) = open_stage(&db, "p1", &p, 0).unwrap();
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
                 VALUES ('h','p1','index.html','HTML/CSS/JS源码','freeform',?1,1,'valid')",
                [&rid],
            )
            .unwrap();
        std::fs::write(root.join(".hexagon/index.html"), "<html></html>").unwrap();
        match evaluate(&db, "p1", &p).unwrap() {
            StageEval::Incomplete { missing } => {
                assert!(
                    missing.iter().any(|m| m == "artifact:HTML/CSS/JS源码"),
                    "{missing:?}"
                );
            }
            other => panic!("only .hexagon/index.html must stay missing, got {other:?}"),
        }
        std::fs::write(root.join("index.html"), "<html></html>").unwrap();
        assert_eq!(evaluate(&db, "p1", &p).unwrap(), StageEval::Ready);
    }

    #[test]
    fn rewind_opens_new_run_at_target() {
        let (db, _d) = setup(&["产品策划", "后端", "架构师"]);
        let p = pack();
        open_stage(&db, "p1", &p, 2).unwrap();
        let r = serde_json::to_value(rewind(&db, "p1", &p, 0).unwrap()).unwrap();
        assert_eq!(r["action"], "rewound");
        let st: String = db
            .conn()
            .query_row("SELECT state FROM stage_runs WHERE seq=2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "rejected");
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StageRewound]))
            .unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn pause_blocks_advance() {
        let (db, _d) = setup(&["产品策划"]);
        let p = pack();
        open_stage(&db, "p1", &p, 0).unwrap();
        pause(&db, "p1").unwrap();
        assert!(matches!(advance(&db, "p1", &p), Err(OrchError::Paused)));
        resume(&db, "p1").unwrap();
        assert!(advance(&db, "p1", &p).is_ok());
    }

    #[test]
    fn checks_run_and_gate_eval() {
        let (db, dir) = setup(&["前端", "后端"]);
        let p = pack();
        let (rid, _) = open_stage(&db, "p1", &p, 3).unwrap();
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
             VALUES ('x','p1','src/a.rs','代码','freeform',?1,1,'valid')",
                [&rid],
            )
            .unwrap();
        // 检验命令会失败（仓里没 cargo 项目）→ incomplete
        run_checks(&db, "p1", dir.path(), &p).unwrap();
        match evaluate(&db, "p1", &p).unwrap() {
            StageEval::Incomplete { missing } => {
                assert!(missing.iter().any(|m| m == "check:cargo test --quiet"))
            }
            _ => panic!(),
        }
        // 换一条必过的命令验证通过路径
        let p2 = serde_json::from_value::<PackDef>(json!({
            "name":"t","version":1,"stages":[{"name":"实现","roles":["前端"],"due":["代码"],
             "checks":["true"]}]}))
        .unwrap();
        // 不删行（events/artifacts 外键引用），直接关掉当前 run 再开新场景
        db.conn()
            .execute("UPDATE stage_runs SET state='done'", [])
            .unwrap();
        let (rid2, _) = open_stage(&db, "p1", &p2, 0).unwrap();
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
             VALUES ('y','p1','src/b.rs','代码','freeform',?1,1,'valid')",
                [&rid2],
            )
            .unwrap();
        // 2026-09-24：kind 为代码时，仓库根上要有同路径文件，阶段才算齐。
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/b.rs"), "fn b() {}").unwrap();
        // D08: both the registered body and its executable counterpart exist.
        std::fs::create_dir_all(root.join(".hexagon/src")).unwrap();
        std::fs::write(root.join(".hexagon/src/b.rs"), "fn b() {}").unwrap();
        db.conn()
            .execute(
                "UPDATE projects SET dir=?1 WHERE id='p1'",
                [root.to_string_lossy().as_ref()],
            )
            .unwrap();
        run_checks(&db, "p1", &root, &p2).unwrap();
        assert_eq!(evaluate(&db, "p1", &p2).unwrap(), StageEval::Ready);
    }

    /// 端到端：假供应商驱动「规格→接口→合入」整包，推进/盖章全按声明。
    #[test]
    fn full_pack_run_with_scripted_provider() {
        use crate::provider::ScriptedProvider;
        use crate::tools::{Registry, ToolContext};
        use crate::turn::{run_turn, text_response, tool_response, TurnOutcome};

        let (db, dir) = setup(&["产品策划", "后端", "架构师", "运维"]);
        // 票 02：默认 L4 会自动通过「规格」（非最终盖章点），本走查要的是等人再 stamp。
        db.conn()
            .execute("UPDATE projects SET autonomy='L0' WHERE id='p1'", [])
            .unwrap();
        let p = serde_json::from_value::<PackDef>(json!({
            "name": "规格驱动", "version": 1,
            "stages": [
                {"name": "规格", "roles": ["产品策划"], "due": ["规格"], "stamp_point": true},
                {"name": "接口", "roles": ["后端","架构师"], "due": ["接口说明"],
                 "reviews": [{"artifact_kind": "接口说明", "reviewer": "架构师"}]},
                {"name": "合入", "roles": ["运维"], "due": [], "stamp_point": true}
            ]
        }))
        .unwrap();
        p.pin(dir.path()).unwrap();

        let spec_art = "---\nkind: 规格\nauthor: a0\nhandoff: 范围收敛\n---\n## 目标\nx\n## 范围\nx\n## 验收\nx\n";
        let api_art =
            "---\nkind: 接口说明\nauthor: a1\n---\n## 资源\nx\n## 端点\nx\n## 错误码\nx\n";
        let provider = ScriptedProvider::new(vec![
            // 产品策划回合：交付规格 → 结束
            tool_response(vec![(
                "t1",
                "artifact_write",
                json!({"path":"specs/prd.md","content":spec_art}),
            )]),
            text_response("规格已交付"),
            // 后端回合：交付接口说明 → 结束
            tool_response(vec![(
                "t2",
                "artifact_write",
                json!({"path":"specs/api.md","content":api_art}),
            )]),
            text_response("接口说明已交付"),
        ]);
        let reg = Registry::builtin();
        let ctx_for = |aid: &str, rid: &str| ToolContext {
            project_id: "p1".into(),
            agent_id: aid.into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: Some(rid.into()),
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        };

        // 阶段 0：规格
        let (r0, _) = open_stage(&db, "p1", &p, 0).unwrap();
        assert_eq!(
            run_turn(&db, &provider, &reg, &ctx_for("a0", &r0), vec![], "写规格").unwrap(),
            TurnOutcome::Finished
        );
        // 规格不是最后一道盖章点，离开时自动通过并打开接口。
        assert_eq!(
            serde_json::to_value(advance(&db, "p1", &p).unwrap()).unwrap()["action"],
            "stage_opened"
        );

        // 阶段 1：后端交付 → 架构师复审通过 → 推进
        let r1 = db.active_stage_run("p1").unwrap().unwrap().id;
        run_turn(&db, &provider, &reg, &ctx_for("a1", &r1), vec![], "写接口").unwrap();
        // 复审仍缺 → incomplete
        match evaluate(&db, "p1", &p).unwrap() {
            StageEval::Incomplete { missing } => assert!(!missing.is_empty()),
            _ => panic!(),
        }
        // D08: a kind-only event no longer passes the review gate.
        let target: String = db
            .conn()
            .query_row(
                "SELECT id FROM artifacts WHERE path='specs/api.md' ORDER BY version DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        crate::review::submit_review(
            &db,
            &ctx_for("a2", &r1),
            &target,
            crate::review::Verdict::Pass,
            "reviewed",
        )
        .unwrap();
        // 推进 → 合入（盖章点）
        assert_eq!(
            serde_json::to_value(advance(&db, "p1", &p).unwrap()).unwrap()["action"],
            "stage_opened"
        );
        assert_eq!(
            serde_json::to_value(advance(&db, "p1", &p).unwrap()).unwrap()["action"],
            "awaiting_stamp"
        );
        assert_eq!(
            serde_json::to_value(stamp(&db, "p1", &p).unwrap()).unwrap()["action"],
            "pack_finished"
        );

        // 回放：事件面完整
        let items = db.timeline("p1", None, 200, None).unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
        for k in [
            EventKind::StageStarted,
            EventKind::AgentActivated,
            EventKind::AgentSlept,
            EventKind::ArtifactDelivered,
            EventKind::Stamped,
            EventKind::ReviewPassed,
            EventKind::StageFinished,
        ] {
            assert!(kinds.contains(&k), "missing {k:?}");
        }
        // 产物都落了 .hexagon/
        assert!(dir.path().join(".hexagon/specs/prd.md").exists());
        assert!(dir.path().join(".hexagon/specs/api.md").exists());
    }

    #[test]
    fn pinned_pack_survives_source_edit() {
        let dir = tempfile::tempdir().unwrap();
        let p = pack();
        p.pin(dir.path()).unwrap();
        // 改掉「源」文件不影响 active
        std::fs::write(
            dir.path().join(".hexagon/pack.json"),
            serde_json::to_string(&pack())
                .unwrap()
                .replace("规格驱动", "改过的包"),
        )
        .unwrap();
        let pinned = PackDef::pinned(dir.path()).unwrap();
        assert_eq!(pinned.name, "规格驱动");
        assert_eq!(pinned.version, 3);
    }

    // ---- rsi-research 票 01/04：名单决策点 + 策略面 diff ----

    #[test]
    fn open_stage_records_roster_decision() {
        // 团队含产品策划+前端：seq3「实现」声明 [前端,后端],后端缺席。
        let (db, _dir) = setup(&["产品策划", "前端"]);
        let pk = pack();
        open_stage(&db, "p1", &pk, 0).unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StageStarted]))
            .unwrap();
        let d = &items[0].event.payload["decision"];
        assert_eq!(d["kind"], "roster");
        assert_eq!(d["eligible"], json!(["产品策划"]));
        assert_eq!(d["chosen"], json!(["产品策划"]));
        open_stage(&db, "p1", &pk, 3).unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StageStarted]))
            .unwrap();
        let d2 = items.iter().find(|e| e.event.payload["seq"] == 3).unwrap();
        let dec = &d2.event.payload["decision"];
        assert_eq!(dec["eligible"], json!(["前端", "后端"]));
        assert_eq!(dec["chosen"], json!(["前端"]));
        let alt = dec["alternatives"].as_array().unwrap();
        assert!(alt
            .iter()
            .any(|a| a["option"] == "后端" && a["reason"] == "absent_from_team"));
    }

    #[test]
    fn stage_skipped_also_records_roster_decision() {
        let (db, _dir) = setup(&["产品策划"]);
        let pk = pack();
        open_stage(&db, "p1", &pk, 0).unwrap();
        db.conn()
            .execute("UPDATE stage_runs SET state='done'", [])
            .unwrap();
        // seq1「界面」要 UI——缺席 → skipped,decision 仍落盘
        open_next(&db, "p1", &pk, 1).unwrap();
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::StageSkipped]))
            .unwrap();
        let d = &items[0].event.payload["decision"];
        assert_eq!(d["kind"], "roster");
        assert_eq!(d["chosen"], json!([]));
        assert_eq!(d["alternatives"][0]["option"], "UI");
    }

    #[test]
    fn policy_diff_separates_knobs_from_process() {
        let mut a = pack();
        let mut b = a.clone();
        // 旋钮改动：knobs + 阶段级策略字段
        b.knobs.flag_patience = Some(5);
        b.stages[0].stamp_point = false;
        b.stages[2].backfill_edges = vec![];
        // 流程改动：角色名单
        b.stages[0].roles = vec!["产品策划".into(), "架构师".into()];
        let diff = policy_diff(&a, &b);
        let paths: Vec<&str> = diff.iter().filter_map(|d| d["path"].as_str()).collect();
        assert!(paths.contains(&"knobs.flag_patience"));
        assert!(paths.contains(&"stages.规格.stamp_point"));
        assert!(paths.contains(&"stages.接口.backfill_edges"));
        // 流程改动不进策略 diff
        assert!(!paths.iter().any(|p| p.contains("roles")));
        // 另一面:non_policy_changes 只报流程差异
        let npc = non_policy_changes(&a, &b);
        assert_eq!(npc, vec![".stages[0].roles[1]".to_string()]);
        // 纯旋钮改动 → non_policy_changes 为空
        a.knobs.judge = Some("llm".into());
        assert!(non_policy_changes(&a, &b)
            .iter()
            .all(|p| p.contains("roles")));
    }

    // ---- rsi-research 票 05：延续机制默认 disarmed ----

    #[test]
    fn crash_recovery_does_not_rearm_turns() {
        // 崩溃恢复只把 run 交还负责人：不重放回合、不自动激活——
        // 「继续」的下一步永远由人发起（ralph 式续跑默认关闭）。
        let (db, _dir) = setup(&["产品策划"]);
        let pk = pack();
        let (r0, _) = open_stage(&db, "p1", &pk, 0).unwrap();
        // 造 dangling turn：turn_started 无收尾
        db.append_event(
            "p1",
            EventKind::TurnStarted,
            json!({"agent": "a0"}),
            Some("a0"),
            Some(&r0),
        )
        .unwrap();
        assert_eq!(detect_interrupted(&db, "p1").unwrap(), 1);
        let pre_turns: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='turn_started'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let status_pre: String = db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        recover_run(&db, "p1", &r0).unwrap();
        // 恢复后无新回合、agent 状态未被恢复动作触碰（激活与否由
        // 阶段名单决定,不由恢复闸决定——恢复不重放、不点名）。
        let post_turns: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='turn_started'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pre_turns, post_turns);
        let status_post: String = db
            .conn()
            .query_row("SELECT status FROM agents WHERE id='a0'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status_pre, status_post);
        let st: String = db
            .conn()
            .query_row("SELECT state FROM stage_runs WHERE id=?1", [&r0], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(st, "active");
    }

    /// B2 核验（arch-review 附录 B2）：流式循环每 delta 调一次
    /// `is_paused`（turn.rs 唯一暂停缝）。10 万事件库上实测每次调用
    /// 必须走 `idx_events_kind(project_id,kind,id)` 索引 O(log n)——
    /// 若未来 schema/查询漂移成全表扫，此测试会咬人。
    #[test]
    fn is_paused_scales_on_large_event_log() {
        let (db, _d) = setup(&["后端"]);
        let mut batch = String::from("BEGIN;");
        for i in 0..100_000u32 {
            batch.push_str(
                "INSERT INTO events (project_id,kind,payload) VALUES ('p1','message','{}');",
            );
            if i % 10_000 == 9_999 {
                batch.push_str("COMMIT;BEGIN;");
            }
        }
        batch.push_str("COMMIT;");
        db.conn().execute_batch(&batch).unwrap();

        let t0 = std::time::Instant::now();
        for _ in 0..1000 {
            assert!(!is_paused(&db, "p1").unwrap());
        }
        let per = t0.elapsed() / 1000;
        eprintln!("is_paused ×1000 @100k events: {per:?}/call");
        // 宽限阈值：索引路径实测在微秒级；掉到毫秒级即说明索引失效。
        assert!(
            per < std::time::Duration::from_millis(5),
            "is_paused 单次 {per:?}——索引路径疑似失效"
        );
    }
}

#[cfg(test)]
mod evidence_properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn only_current_success_is_passed(value in ".{0,60}", exit in any::<i32>(), stable in any::<bool>()) {
            let before = format!("old:{value}");
            let after = format!("new:{value}");
            prop_assert_ne!(check_state(true, Some(exit), Some(&before), stable, Some(&after)), CheckState::Passed);
            prop_assert_ne!(check_state(true, Some(exit), None, stable, Some(&after)), CheckState::Passed);
            prop_assert_ne!(check_state(true, Some(exit), Some(&before), stable, None), CheckState::Passed);
            prop_assert_eq!(check_state(true, Some(exit), Some(&before), stable, Some(&before)) == CheckState::Passed, exit == 0 && stable);
        }
        #[test]
        fn owner_skip_is_never_roster_evidence(role in ".{0,40}", reason in ".{0,60}") {
            let owner = json!({"by":"owner","reason":reason});
            prop_assert!(!absent_roster_fact(&owner, std::slice::from_ref(&role)));
            let fact = json!({"reason":"no listed role in team","decision_kind":"roster", "decision":{"kind":"roster","chosen":[],"eligible":[role.clone()]}});
            prop_assert!(absent_roster_fact(&fact, &[role]));
        }
    }
}
