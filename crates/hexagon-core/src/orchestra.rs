//! 编排内核：流程包驱动阶段推进。
//!
//! - 版本钉住：开跑时把包副本快照到 `.hexagon/pack.active.json`，运行中实例
//!   只读这份；提案改 `.hexagon/pack.json` 下次开跑生效。
//! - 阶段推进：按包声明名单激活/休眠 Agent；阶段名单与团队交集为空 → 跳过；
//!   阶段内并行默认。
//! - 成功判定（工作台裁决）：应交产物齐且登记 + 检验命令全过 + 声明复审全过。
//! - 声明式回路：回填边与会诊唤醒名单是包内声明数据（票 10 消费裁决语义）；
//!   盖章点到点停为待决问题。
//! - 阶段动作：退回/跳过/暂停/恢复，全落事件。

use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OrchError {
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDecl {
    /// 复审对象：产物 kind
    pub artifact_kind: String,
    /// 复审者角色
    pub reviewer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    fn active_stage_run(&self, project_id: &str) -> Result<Option<StageRun>, OrchError> {
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
        Ok(rows.next().transpose()?)
    }
}

/// 暂停看事件流：最后一个 Paused/Resumed 决定。
pub fn is_paused(db: &Db, project_id: &str) -> Result<bool, OrchError> {
    let k: Option<String> = db
        .conn()
        .query_row(
            "SELECT kind FROM events WHERE project_id=?1
             AND kind IN ('paused','resumed') ORDER BY id DESC LIMIT 1",
            [project_id],
            |r| r.get(0),
        )
        .ok();
    Ok(k.as_deref() == Some("paused"))
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
        db.conn().execute(
            "UPDATE agents SET status=?1 WHERE id=?2",
            rusqlite::params![if on { "active" } else { "sleeping" }, aid],
        )?;
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

/// 阶段成功判定（工作台裁决，非 Agent）。
pub fn evaluate(db: &Db, project_id: &str, pack: &PackDef) -> Result<StageEval, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    let stage = &pack.stages[run.seq as usize];
    let mut missing = Vec::new();

    // 应交产物齐（本 run 内 valid/stamped）
    for kind in &stage.due {
        let n: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM artifacts
             WHERE project_id=?1 AND stage_run_id=?2 AND kind=?3 AND status IN ('valid','stamped')",
            rusqlite::params![project_id, run.id, kind],
            |r| r.get(0),
        )?;
        if n == 0 {
            missing.push(format!("artifact:{kind}"));
        }
    }

    // 检验命令全过：最近一次 TestRan exit=0，或负责人已显式覆盖（票 40）
    for cmd in failing_checks(db, project_id, &run.id, stage)? {
        missing.push(format!("check:{cmd}"));
    }

    // 声明复审全过：每条声明最新复审事件为 passed
    for rev in &stage.reviews {
        let k: Option<String> = db
            .conn()
            .query_row(
                "SELECT kind FROM events
                 WHERE project_id=?1 AND stage_run_id=?2
                 AND kind IN ('review_passed','review_rejected','review_skipped')
                 AND json_extract(payload,'$.artifact_kind')=?3
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![project_id, run.id, rev.artifact_kind],
                |r| r.get(0),
            )
            .ok();
        // passed 或负责人显式跳过都算满足；rejected 仍是缺口
        if !matches!(k.as_deref(), Some("review_passed") | Some("review_skipped")) {
            missing.push(format!("review:{}", rev.artifact_kind));
        }
    }

    Ok(if missing.is_empty() {
        StageEval::Ready
    } else {
        StageEval::Incomplete { missing }
    })
}

/// 未满足的检验命令：最近一次 TestRan 非 0（含未跑），且没有
/// 覆盖到该命令的 check_overridden 事件（票 40：覆盖是留痕事实，评估认账）。
fn failing_checks(
    db: &Db,
    project_id: &str,
    run_id: &str,
    stage: &StageDef,
) -> Result<Vec<String>, OrchError> {
    let mut failing = Vec::new();
    for cmd in &stage.checks {
        let passed: Option<i64> = db
            .conn()
            .query_row(
                "SELECT CAST(json_extract(payload,'$.exit_code') AS INTEGER) FROM events
                 WHERE project_id=?1 AND stage_run_id=?2 AND kind='test_ran'
                 AND json_extract(payload,'$.cmd')=?3
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![project_id, run_id, cmd],
                |r| r.get(0),
            )
            .ok();
        if passed == Some(0) {
            continue;
        }
        let covered: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM events e, json_each(json_extract(e.payload,'$.cmds')) j
             WHERE e.project_id=?1 AND e.stage_run_id=?2
               AND e.kind='check_overridden' AND j.value=?3",
            rusqlite::params![project_id, run_id, cmd],
            |r| r.get(0),
        )?;
        if covered == 0 {
            failing.push(cmd.clone());
        }
    }
    Ok(failing)
}

/// 负责人显式覆盖检验失败（票 40）：落 check_overridden 留痕（谁/哪些命令/理由），
/// 之后 evaluate 把这些命令记为满足——stamp/合入随既有闸门自然放行。
/// 只覆盖检验项；缺产物/未过复审不在覆盖范围。
pub fn override_checks(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    reason: &str,
) -> Result<Value, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    let stage = &pack.stages[run.seq as usize];
    let failing = failing_checks(db, project_id, &run.id, stage)?;
    if failing.is_empty() {
        return Err(OrchError::NothingToOverride(run.id));
    }
    db.append_event(
        project_id,
        EventKind::CheckOverridden,
        json!({"cmds": failing, "reason": reason, "by": "owner"}),
        None,
        Some(&run.id),
    )?;
    Ok(json!({"overridden": failing, "stage": run.stage_name}))
}

/// 跑本阶段的检验命令（包内声明 = 预授权，直跑不过权限管线）。
pub fn run_checks(
    db: &Db,
    project_id: &str,
    repo_root: &Path,
    pack: &PackDef,
) -> Result<Vec<(String, i32)>, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    let stage = &pack.stages[run.seq as usize];
    let mut out = Vec::new();
    for cmd in &stage.checks {
        let res = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(repo_root)
            .output()?;
        let code = res.status.code().unwrap_or(-1);
        db.append_event(
            project_id,
            EventKind::TestRan,
            json!({"cmd": cmd, "exit_code": code,
                   "stdout": String::from_utf8_lossy(&res.stdout).chars().take(2000).collect::<String>()}),
            None,
            Some(&run.id),
        )?;
        out.push((cmd.clone(), code));
    }
    Ok(out)
}

/// 推进：评估当前阶段 → ready 则盖章点停 or done+开下一阶段。
/// 返回发生了什么的描述。
pub fn advance(db: &Db, project_id: &str, pack: &PackDef) -> Result<Value, OrchError> {
    if is_paused(db, project_id)? {
        return Err(OrchError::Paused);
    }
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    if run.state == "waiting_stamp" {
        return Ok(json!({"action": "waiting_stamp", "stage": run.stage_name}));
    }
    match evaluate(db, project_id, pack)? {
        StageEval::Incomplete { missing } => {
            Ok(json!({"action": "incomplete", "stage": run.stage_name, "missing": missing}))
        }
        StageEval::Ready => {
            let stage = &pack.stages[run.seq as usize];
            if stage.stamp_point {
                db.conn().execute(
                    "UPDATE stage_runs SET state='waiting_stamp' WHERE id=?1",
                    [&run.id],
                )?;
                let qid = format!("q{}", db.next_id("q")?);
                db.conn().execute(
                    "INSERT INTO pending_questions (id, project_id, kind, payload)
                     VALUES (?1,?2,'stamp',?3)",
                    rusqlite::params![
                        qid,
                        project_id,
                        json!({"stage": stage.name, "run_id": run.id}).to_string()
                    ],
                )?;
                db.append_event(
                    project_id,
                    EventKind::PermissionAsked,
                    json!({"kind": "stamp", "stage": stage.name, "question_id": qid}),
                    None,
                    Some(&run.id),
                )?;
                return Ok(
                    json!({"action": "awaiting_stamp", "stage": stage.name, "question_id": qid}),
                );
            }
            finish_stage(db, project_id, pack, &run)
        }
    }
}

fn finish_stage(
    db: &Db,
    project_id: &str,
    pack: &PackDef,
    run: &StageRun,
) -> Result<Value, OrchError> {
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
) -> Result<Value, OrchError> {
    let mut seq = from_seq;
    loop {
        if seq >= pack.stages.len() {
            // 全程跑完：全员休眠
            db.conn().execute(
                "UPDATE agents SET status='sleeping' WHERE project_id=?1",
                [project_id],
            )?;
            db.append_event(
                project_id,
                EventKind::TeamSlept,
                json!({"reason": "pack finished"}),
                None,
                None,
            )?;
            return Ok(json!({"action": "pack_finished"}));
        }
        let (rid, skipped) = open_stage(db, project_id, pack, seq)?;
        if !skipped {
            return Ok(json!({"action": "stage_opened", "run_id": rid, "seq": seq}));
        }
        seq += 1;
    }
}

/// 盖章确认：waiting_stamp → done → 推进。
pub fn stamp(db: &Db, project_id: &str, pack: &PackDef) -> Result<Value, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    if run.state != "waiting_stamp" {
        return Err(OrchError::BadSeq(run.seq));
    }
    db.conn().execute(
        "UPDATE stage_runs SET state='done', finished_at=datetime('now') WHERE id=?1",
        [&run.id],
    )?;
    db.append_event(
        project_id,
        EventKind::Stamped,
        json!({"stage": run.stage_name, "seq": run.seq}),
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
) -> Result<Value, OrchError> {
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
    Ok(json!({"action": "rewound", "to_seq": to_seq, "run_id": rid}))
}

/// 跳过当前阶段。
pub fn skip(db: &Db, project_id: &str, pack: &PackDef) -> Result<Value, OrchError> {
    let run = db
        .active_stage_run(project_id)?
        .ok_or_else(|| OrchError::NoActiveStage(project_id.into()))?;
    db.conn().execute(
        "UPDATE stage_runs SET state='skipped', finished_at=datetime('now') WHERE id=?1",
        [&run.id],
    )?;
    db.append_event(
        project_id,
        EventKind::StageSkipped,
        json!({"stage": run.stage_name, "by": "owner"}),
        None,
        Some(&run.id),
    )?;
    open_next(db, project_id, pack, run.seq as usize + 1)
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
pub fn team(db: &Db, project_id: &str) -> Result<Vec<Value>, OrchError> {
    let mut st = db
        .conn()
        .prepare("SELECT id, role, model_slot, status FROM agents WHERE project_id=?1")?;
    let rows = st
        .query_map([project_id], |r| {
            Ok(
                json!({"id": r.get::<_,String>(0)?, "role": r.get::<_,String>(1)?,
                      "model_slot": r.get::<_,Option<String>>(2)?,
                      "status": r.get::<_,String>(3)?}),
            )
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 阶段运行状态表（stage_runs 读模型）。
pub fn stage_status(db: &Db, project_id: &str) -> Result<Vec<Value>, OrchError> {
    let mut st = db.conn().prepare(
        "SELECT id, stage_name, seq, state FROM stage_runs WHERE project_id=?1 ORDER BY seq, id",
    )?;
    let rows = st
        .query_map([project_id], |r| {
            Ok(
                json!({"run_id": r.get::<_,String>(0)?, "stage": r.get::<_,String>(1)?,
                      "seq": r.get::<_,i64>(2)?, "state": r.get::<_,String>(3)?}),
            )
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 项目元信息（projects 行 + fastpath 角色名解析）。
pub fn project_info(db: &Db, project_id: &str) -> Result<Value, OrchError> {
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
    Ok(json!({
        "name": name,
        "mode": mode,
        "pack_name": pack_name,
        "fastpath_agent_id": fast_aid,
        "fastpath_role": fast_role,
    }))
}

/// 全员休眠（干预指令：回合进行中也要能落，故走控制通道）。
/// agents.status 的归一写口收敛进 cards 阶段（arch-review 票 04）。
pub fn sleep_all(db: &Db, project_id: &str) -> Result<(), OrchError> {
    db.conn().execute(
        "UPDATE agents SET status='sleeping' WHERE project_id=?1",
        [project_id],
    )?;
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
    db.conn().execute(
        "UPDATE agents SET status=?1 WHERE id=?2 AND project_id=?3",
        rusqlite::params![
            if sleeping { "sleeping" } else { "active" },
            agent_id,
            project_id
        ],
    )?;
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
        let qid = format!("q{}", db.next_id("q")?);
        db.conn().execute(
            "INSERT INTO pending_questions (id, project_id, agent_id, kind, payload)
             VALUES (?1, ?2, ?3, 'recovery', ?4)",
            rusqlite::params![
                qid,
                project_id,
                agent_id,
                json!({"run_id": run_id, "stage": stage}).to_string()
            ],
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
    db.conn().execute(
        "UPDATE pending_questions SET state='answered', answered_at=datetime('now'), answered_by='owner'
         WHERE project_id=?1 AND kind='recovery' AND state='queued'
           AND json_extract(payload, '$.run_id')=?2",
        rusqlite::params![project_id, run_id],
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
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
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
        (db, tempfile::tempdir().unwrap())
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
        // 交付产物 + 复审通过
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
             VALUES ('x','p1','specs/api.md','接口说明','skeleton',?1,1,'valid')",
                [&rid],
            )
            .unwrap();
        db.append_event(
            "p1",
            EventKind::ReviewPassed,
            json!({"artifact_kind":"接口说明","reviewer":"架构师"}),
            None,
            Some(&rid),
        )
        .unwrap();
        assert_eq!(evaluate(&db, "p1", &p).unwrap(), StageEval::Ready);
        // advance → done + 开实现阶段
        let r = advance(&db, "p1", &p).unwrap();
        assert_eq!(r["action"], "stage_opened");
        assert_eq!(r["seq"], 3);
        let _ = dir;
    }

    #[test]
    fn stamp_point_stops_until_stamped() {
        let (db, _d) = setup(&["产品策划", "后端", "架构师"]);
        let p = pack();
        let (rid, _) = open_stage(&db, "p1", &p, 0).unwrap();
        db.conn()
            .execute(
                "INSERT INTO artifacts (id,project_id,path,kind,tier,stage_run_id,version,status)
             VALUES ('x','p1','specs/prd.md','规格','skeleton',?1,1,'valid')",
                [&rid],
            )
            .unwrap();
        let r = advance(&db, "p1", &p).unwrap();
        assert_eq!(r["action"], "awaiting_stamp");
        // 再 advance 不会动
        let r2 = advance(&db, "p1", &p).unwrap();
        assert_eq!(r2["action"], "waiting_stamp");
        // 盖章 → 「界面」无 UI 被跳过 → 穿透到「接口」
        let r3 = stamp(&db, "p1", &p).unwrap();
        assert_eq!(r3["action"], "stage_opened");
        assert_eq!(r3["seq"], 2);
        let st: String = db
            .conn()
            .query_row("SELECT state FROM stage_runs WHERE seq=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(st, "skipped");
    }

    #[test]
    fn rewind_opens_new_run_at_target() {
        let (db, _d) = setup(&["产品策划", "后端", "架构师"]);
        let p = pack();
        open_stage(&db, "p1", &p, 2).unwrap();
        let r = rewind(&db, "p1", &p, 0).unwrap();
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
        run_checks(&db, "p1", dir.path(), &p2).unwrap();
        assert_eq!(evaluate(&db, "p1", &p2).unwrap(), StageEval::Ready);
    }

    /// 端到端：假供应商驱动「规格→接口→合入」整包，推进/盖章全按声明。
    #[test]
    fn full_pack_run_with_scripted_provider() {
        use crate::provider::ScriptedProvider;
        use crate::tools::{Registry, ToolContext};
        use crate::turn::{run_turn, text_response, tool_response, TurnOutcome};

        let (db, dir) = setup(&["产品策划", "后端", "架构师", "运维"]);
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
        };

        // 阶段 0：规格
        let (r0, _) = open_stage(&db, "p1", &p, 0).unwrap();
        assert_eq!(
            run_turn(&db, &provider, &reg, &ctx_for("a0", &r0), vec![], "写规格").unwrap(),
            TurnOutcome::Finished
        );
        assert_eq!(advance(&db, "p1", &p).unwrap()["action"], "awaiting_stamp");
        // 盖章 → 接口阶段（架构师同激活，无交付义务）
        assert_eq!(stamp(&db, "p1", &p).unwrap()["action"], "stage_opened");

        // 阶段 1：后端交付 → 架构师复审通过 → 推进
        let r1 = db.active_stage_run("p1").unwrap().unwrap().id;
        run_turn(&db, &provider, &reg, &ctx_for("a1", &r1), vec![], "写接口").unwrap();
        // 复审仍缺 → incomplete
        match evaluate(&db, "p1", &p).unwrap() {
            StageEval::Incomplete { missing } => assert!(!missing.is_empty()),
            _ => panic!(),
        }
        db.append_event(
            "p1",
            EventKind::ReviewPassed,
            json!({"artifact_kind":"接口说明","reviewer":"架构师"}),
            Some("a2"),
            Some(&r1),
        )
        .unwrap();
        // 推进 → 合入（盖章点）
        assert_eq!(advance(&db, "p1", &p).unwrap()["action"], "stage_opened");
        assert_eq!(advance(&db, "p1", &p).unwrap()["action"], "awaiting_stamp");
        assert_eq!(stamp(&db, "p1", &p).unwrap()["action"], "pack_finished");

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
}
