//! 发布（owner-followups-1015 #15，ADR 0093 / CONTEXT.md「远程发布」「发布目标」）：对已登记的发布目标
//! 发起发布 = 入队一张发布确认卡（kind='release'）；批准后用登记的命令与钥匙串凭据执行，结果落轨迹。
//!
//! - **必问、永不记忆**：发布通常不可逆。卡上没有「允许本项目」，本模块不写 `permission_rules`、
//!   不读记忆层；每次发布都是一张新卡、一次亲手批准（或托管在宿主事实全部成立时代批，见
//!   `api/managed/routes.rs` 的 `release_forced`）。聊天发起（票 16，`release_chat::RequestRelease`）也只落这张卡。
//! - **卡面即授权范围**：卡载荷抄下请求时的命令、工作目录、凭据名。确认时与登记表逐项比对，
//!   登记在卡发出后被改过就拒绝（`release_target_changed`），负责人批的必须是他看到的那条命令。
//! - **前置闸 fail-closed**：目标还在、未改动、工作目录在项目内且存在、每个所需凭据都已存入——
//!   任一不成立就明确失败、什么都不跑，卡留在待决里（补好凭据可再点一次），失败落 `release_failed`
//!   （`executed: false`）。代价模型：误拦只花负责人一次补录 / 再点；误放是一次带着缺失或过期配置的
//!   不可逆发布。偏向拦。
//! - **认领一次**：前置闸过后用 `cards::claim` 原子认领卡，认领失败（连点 / 负责人与托管同时裁决）
//!   不执行——重复跑一次发布命令的代价远高于一次报错。认领后才执行，所以执行失败不会被静默重发。
//! - **凭据只走环境变量**：凭据名即环境变量名（`release_targets::normalize_credential_name`），值从钥匙串
//!   读出后只进子进程环境；不拼命令行、不进卡、不进轨迹、不进日志与诊断。命令输出入轨迹前把每个
//!   凭据值替换成 `[redacted]`（发布工具常回显令牌），只留末尾 [`OUTPUT_TAIL_CHARS`] 字。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cards::{CardKind, CardState};
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::release_targets::{self as targets, ReleaseError};
use crate::trace::EventKind;

mod runner;
use runner::{assemble, run_command, tail, RunStatus};

/// 发布命令的墙钟上限：包发布 / 部署可能要几分钟，半小时没结束视为卡死，杀掉整个进程组。
pub const RELEASE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 轨迹与回执里留的输出末尾（字符）。
pub const OUTPUT_TAIL_CHARS: usize = 4000;
/// 凭据值在输出里的替换文。
pub const REDACTED: &str = "[redacted]";

/// 裁决人（发布 / 登记拟稿的确认与驳回入口共用）。ticket 33（2026-10-25 批次审查 S6，原票 #16「托管不得新登记」）：
/// 「只有负责人」原先靠 `by != "owner"` 字符串比较——任何把字面量 `owner` 当裁决人标签传进来的路径（托管 actor 名、
/// 手拼的 `managed:*`）都能冒充负责人。改成类型：负责人只有壳层的负责人按钮与工作台无托管作用域时构造，
/// 托管作用域下一律是 [`Decider::Delegate`]，标签只用于认领与轨迹，不参与判定。
/// 代价模型：误拒 = 非负责人的调用报 `release_owner_only`；漏放 = 托管登记一条随后可代批执行的命令。偏向拒。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decider {
    /// 负责人亲手裁决。
    Owner,
    /// 代理裁决人（托管 `managed:<grant>:<gen>` 等），携带轨迹标签。
    Delegate(String),
}

impl Decider {
    /// 认领与轨迹里的 `by` 字样。
    pub fn label(&self) -> &str {
        match self {
            Decider::Owner => "owner",
            Decider::Delegate(actor) => actor,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReleaseRunError {
    #[error("unknown release question: {0}")]
    ReleaseQuestionUnknown(String),
    #[error("release question {0} was already answered")]
    ReleaseAlreadyAnswered(String),
    #[error("release target is no longer registered: {0}")]
    ReleaseTargetGone(String),
    #[error("release target {0} changed after this card was asked; request the release again")]
    ReleaseTargetChanged(String),
    /// 只点名凭据名，值永不出现在错误里。
    #[error("missing credential {credential} for release target {target}")]
    ReleaseCredentialMissing { target: String, credential: String },
    #[error("release working directory is unavailable: {0}")]
    ReleaseCwdUnavailable(String),
    #[error("release command could not start: {0}")]
    ReleaseSpawn(String),
    /// 退出码（信号终止为 None）；输出只在轨迹里（已脱敏），不进错误文案。
    #[error("release command failed (exit {})", .0.map_or("signal".to_string(), |c| c.to_string()))]
    ReleaseCommandFailed(Option<i32>),
    #[error("release command timed out after {0}s")]
    ReleaseTimedOut(u64),
    /// 聊天拟登记（票 16）：拟稿疑似夹着凭据值。文案不回显任何字段。
    #[error("this looks like it contains a credential value; list the credential by name (like NPM_TOKEN) and have the owner enter the value in Settings · Release")]
    ReleaseSecretSuspected,
    #[error("unknown release target proposal: {0}")]
    ReleaseProposalUnknown(String),
    #[error("release target proposal {0} was already answered")]
    ReleaseProposalAnswered(String),
    #[error(transparent)]
    Target(#[from] ReleaseError),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("trace: {0}")]
    Trace(#[from] crate::trace::TraceError),
    #[error("credential: {0}")]
    Cred(#[from] crate::credentials::CredError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// 发布回执（ADR 0054）：只有成功才有回执；失败走错误 + `release_failed` 事件。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ReleaseOutcome {
    pub target_id: String,
    pub name: String,
    /// 命令输出末尾（stdout 与 stderr 合流，凭据值已替换为 `[redacted]`）。
    pub output: String,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

fn project_dir(db: &Db, project_id: &str) -> Result<PathBuf, ReleaseRunError> {
    Ok(db
        .conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
            r.get::<_, String>(0)
        })?
        .into())
}

fn text(p: &Value, key: &str) -> String {
    p[key].as_str().unwrap_or_default().to_string()
}

/// 发起发布：只入队确认卡，不执行任何东西。同一目标已有一张待决的发布卡时返回那张卡
/// （⌘K 连按不叠卡；这不是记忆——那张卡同样要亲手批准）。
pub fn request(
    db: &Db,
    project_id: &str,
    target_id: &str,
    by: &str,
) -> Result<String, ReleaseRunError> {
    let target = targets::row(db, project_id, target_id)?;
    if let Some(open) = crate::cards::queued(db, project_id)?
        .into_iter()
        .find(|c| c.kind == CardKind::Release.as_str() && c.payload["target_id"] == target.id)
    {
        return Ok(open.id);
    }
    let qid = crate::cards::enqueue(
        db,
        project_id,
        None,
        CardKind::Release,
        json!({
            "target_id": target.id,
            "name": target.name,
            "command": target.command,
            "cwd": target.cwd,
            "credentials": target.credentials,
            "requested_by": by,
        }),
        None,
    )?;
    db.append_event(
        project_id,
        EventKind::ReleaseRequested,
        json!({"target_id": target.id, "name": target.name, "question_id": qid, "by": by}),
        None,
        None,
    )?;
    log::info!("release requested: target={} qid={qid}", target.id);
    Ok(qid)
}

/// 前置闸通过后的执行材料。`secrets` 只活在这次确认里。
struct Prepared {
    target: targets::Row,
    dir: PathBuf,
    secrets: Vec<(String, String)>,
}

/// 卡面与登记逐项比对 + 工作目录 + 凭据。任何一项不成立都不执行。
fn preflight(
    db: &Db,
    project_id: &str,
    payload: &Value,
    store: &dyn CredentialStore,
) -> Result<Prepared, ReleaseRunError> {
    let name = text(payload, "name");
    let target = match targets::row(db, project_id, &text(payload, "target_id")) {
        Ok(t) => t,
        Err(ReleaseError::ReleaseTargetUnknown(_)) => {
            return Err(ReleaseRunError::ReleaseTargetGone(name))
        }
        Err(e) => return Err(e.into()),
    };
    let asked: Vec<String> = payload["credentials"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if target.command != text(payload, "command")
        || target.cwd != text(payload, "cwd")
        || target.credentials != asked
        || target.name != name
    {
        return Err(ReleaseRunError::ReleaseTargetChanged(target.name));
    }
    let dir = working_dir(&project_dir(db, project_id)?, &target.cwd)?;
    let mut secrets = Vec::new();
    for credential in &target.credentials {
        match store.get(&targets::credential_key(&target.id, credential))? {
            Some(value) if !value.is_empty() => secrets.push((credential.clone(), value)),
            _ => {
                return Err(ReleaseRunError::ReleaseCredentialMissing {
                    target: target.name.clone(),
                    credential: credential.clone(),
                })
            }
        }
    }
    Ok(Prepared {
        target,
        dir,
        secrets,
    })
}

/// 工作目录：登记时已按字面挡掉 `..` / 绝对路径（`normalize_cwd`）；这里再按真实路径挡符号链接
/// 走出项目——登记之后有人在项目里放一个指向外面的链接，字面判定看不出来。
fn working_dir(root: &Path, cwd: &str) -> Result<PathBuf, ReleaseRunError> {
    let unavailable = || ReleaseRunError::ReleaseCwdUnavailable(cwd.to_string());
    let root = root.canonicalize().map_err(|_| unavailable())?;
    let dir = root.join(cwd).canonicalize().map_err(|_| unavailable())?;
    if !dir.starts_with(&root) {
        return Err(ReleaseError::ReleaseCwdOutside(cwd.to_string()).into());
    }
    if !dir.is_dir() {
        return Err(unavailable());
    }
    Ok(dir)
}

fn note(branch: &str, warn: bool, qid: &str, code: &str, started: Instant) {
    crate::diag::note(
        if warn {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_HOST
        },
        warn,
        Some(crate::PROJECT_ID),
        None,
        None,
        Some(qid),
        branch,
        code,
        started,
    );
}

/// 失败落轨迹：`detail` 带错误码、是否执行过，以及执行过时的退出码 / 耗时 / 脱敏输出。
fn failed(
    db: &Db,
    project_id: &str,
    qid: &str,
    payload: &Value,
    by: &str,
    detail: Value,
) -> Result<(), ReleaseRunError> {
    let mut event = json!({
        "target_id": text(payload, "target_id"),
        "name": text(payload, "name"),
        "question_id": qid,
        "by": by,
    });
    if let (Some(event), Some(detail)) = (event.as_object_mut(), detail.as_object()) {
        event.extend(detail.clone());
    }
    db.append_event(project_id, EventKind::ReleaseFailed, event, None, None)?;
    Ok(())
}

/// 负责人确认发布。
pub fn confirm(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
) -> Result<ReleaseOutcome, ReleaseRunError> {
    confirm_by(db, project_id, qid, store, &Decider::Owner, RELEASE_TIMEOUT)
}

/// 确认发布（负责人或托管裁决人）：前置闸 → 原子认领 → 执行 → 落轨迹。
pub fn confirm_by(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
    decider: &Decider,
    timeout: Duration,
) -> Result<ReleaseOutcome, ReleaseRunError> {
    let by = decider.label();
    let started = Instant::now();
    // ticket 33 S2：读失败如实上抛（`Cards`），只有真没有这张发布卡才是「未知」。
    let card = crate::cards::get_kind(db, project_id, qid, CardKind::Release)?
        .ok_or_else(|| ReleaseRunError::ReleaseQuestionUnknown(qid.into()))?;
    if card.state != CardState::Queued {
        return Err(ReleaseRunError::ReleaseAlreadyAnswered(qid.into()));
    }
    let payload = card.payload;
    let prepared = match preflight(db, project_id, &payload, store) {
        Ok(p) => p,
        Err(e) => {
            // 什么都没跑：卡留在待决里，补齐后可再确认。
            let code = crate::errcode::ErrorCode::code(&e);
            note("release_preflight", true, qid, &code, started);
            failed(
                db,
                project_id,
                qid,
                &payload,
                by,
                json!({"error": code, "executed": false}),
            )?;
            return Err(e);
        }
    };
    if !crate::cards::claim(db, project_id, qid, CardKind::Release, by)? {
        note(
            "release_claim",
            true,
            qid,
            "release_already_answered",
            started,
        );
        return Err(ReleaseRunError::ReleaseAlreadyAnswered(qid.into()));
    }
    let Prepared {
        target,
        dir,
        secrets,
    } = prepared;
    let result = execute(&target.command, &dir, &secrets, timeout);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    drop(secrets);
    let run = Ran {
        qid,
        by,
        started,
        duration_ms,
    };
    match result {
        Ok(output) => record_released(db, project_id, &run, target, output),
        Err(failure) => record_run_failed(db, project_id, &run, &target, &payload, failure),
    }
}

/// 认领后的执行：跑命令、取脱敏后的输出末尾、按退出状态分成功 / 失败。
fn execute(
    command: &str,
    dir: &Path,
    secrets: &[(String, String)],
    timeout: Duration,
) -> Result<String, RunFailure> {
    let r = run_command(command, dir, secrets, timeout)?;
    let output = tail(&assemble(&r, secrets), OUTPUT_TAIL_CHARS);
    match r.status {
        RunStatus::Exited(Some(0)) => Ok(output),
        RunStatus::Exited(code) => Err(RunFailure {
            e: ReleaseRunError::ReleaseCommandFailed(code),
            code,
            output,
        }),
        RunStatus::TimedOut => Err(RunFailure {
            e: ReleaseRunError::ReleaseTimedOut(timeout.as_secs()),
            code: None,
            output,
        }),
    }
}

/// 一次已执行发布的轨迹上下文。
struct Ran<'a> {
    qid: &'a str,
    by: &'a str,
    started: Instant,
    duration_ms: u64,
}

fn record_released(
    db: &Db,
    project_id: &str,
    run: &Ran,
    target: targets::Row,
    output: String,
) -> Result<ReleaseOutcome, ReleaseRunError> {
    db.append_event(
        project_id,
        EventKind::ReleaseConfirmed,
        json!({
            "target_id": target.id, "name": target.name, "question_id": run.qid, "by": run.by,
            "exit_code": 0, "duration_ms": run.duration_ms, "output": output,
        }),
        None,
        None,
    )?;
    note("release_run", false, run.qid, "released", run.started);
    log::info!("release done: target={}", target.id);
    Ok(ReleaseOutcome {
        target_id: target.id,
        name: target.name,
        output,
        duration_ms: run.duration_ms,
    })
}

fn record_run_failed(
    db: &Db,
    project_id: &str,
    run: &Ran,
    target: &targets::Row,
    payload: &Value,
    RunFailure { e, code, output }: RunFailure,
) -> Result<ReleaseOutcome, ReleaseRunError> {
    let error = crate::errcode::ErrorCode::code(&e);
    failed(
        db,
        project_id,
        run.qid,
        payload,
        run.by,
        json!({"error": error, "executed": true, "exit_code": code,
               "duration_ms": run.duration_ms, "output": output}),
    )?;
    note("release_run", true, run.qid, &error, run.started);
    log::warn!("release failed: target={} {e}", target.id);
    Err(e)
}

struct RunFailure {
    e: ReleaseRunError,
    code: Option<i32>,
    output: String,
}

impl From<ReleaseRunError> for RunFailure {
    fn from(e: ReleaseRunError) -> Self {
        RunFailure {
            e,
            code: None,
            output: String::new(),
        }
    }
}

/// 驳回发布：认领 + 轨迹。
pub fn reject(db: &Db, project_id: &str, qid: &str) -> Result<(), ReleaseRunError> {
    reject_by(db, project_id, qid, &Decider::Owner)
}

pub fn reject_by(
    db: &Db,
    project_id: &str,
    qid: &str,
    decider: &Decider,
) -> Result<(), ReleaseRunError> {
    let by = decider.label();
    // ticket 33 S2：读失败如实上抛（`Cards`），只有真没有这张发布卡才是「未知」。
    let card = crate::cards::get_kind(db, project_id, qid, CardKind::Release)?
        .ok_or_else(|| ReleaseRunError::ReleaseQuestionUnknown(qid.into()))?;
    if !crate::cards::claim(db, project_id, qid, CardKind::Release, by)? {
        return Err(ReleaseRunError::ReleaseAlreadyAnswered(qid.into()));
    }
    db.append_event(
        project_id,
        EventKind::ReleaseRejected,
        json!({
            "target_id": text(&card.payload, "target_id"),
            "name": text(&card.payload, "name"),
            "question_id": qid,
            "by": by,
        }),
        None,
        None,
    )?;
    Ok(())
}
