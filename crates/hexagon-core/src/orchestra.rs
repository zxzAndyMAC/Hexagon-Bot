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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackDef {
    pub name: String,
    pub version: u32,
    pub stages: Vec<StageDef>,
}

impl PackDef {
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
            json!({"stage": stage.name, "seq": seq, "reason": "no listed role in team"}),
            None,
            Some(&rid),
        )?;
        return Ok((rid, true));
    }

    db.append_event(
        project_id,
        EventKind::StageStarted,
        json!({"stage": stage.name, "seq": seq, "due": stage.due, "stamp_point": stage.stamp_point}),
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
}
