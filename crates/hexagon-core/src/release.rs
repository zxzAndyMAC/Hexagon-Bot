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

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cards::{CardKind, CardState};
use crate::credentials::CredentialStore;
use crate::db::Db;
use crate::release_targets::{self as targets, ReleaseError};
use crate::trace::EventKind;

/// 发布命令的墙钟上限：包发布 / 部署可能要几分钟，半小时没结束视为卡死，杀掉整个进程组。
pub const RELEASE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 轨迹与回执里留的输出末尾（字符）。
pub const OUTPUT_TAIL_CHARS: usize = 4000;
/// 每路输出在内存里最多留的字节（只留最新的）；远大于末尾窗口，截断边界落不进末尾。
const CAPTURE_BYTES: usize = 1 << 20;
/// 凭据值在输出里的替换文。
pub const REDACTED: &str = "[redacted]";

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
    confirm_by(db, project_id, qid, store, "owner", RELEASE_TIMEOUT)
}

/// 确认发布（负责人或托管裁决人）：前置闸 → 原子认领 → 执行 → 落轨迹。
pub fn confirm_by(
    db: &Db,
    project_id: &str,
    qid: &str,
    store: &dyn CredentialStore,
    by: &str,
    timeout: Duration,
) -> Result<ReleaseOutcome, ReleaseRunError> {
    let started = Instant::now();
    let card = crate::cards::get(db, qid)
        .ok()
        .filter(|c| c.kind == CardKind::Release.as_str() && c.project_id == project_id)
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
    let run = run_command(&target.command, &dir, &secrets, timeout);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let result: Result<String, RunFailure> = match run {
        Err(e) => Err(e.into()),
        Ok(r) => {
            let output = tail(&redact(&r.output, &secrets), OUTPUT_TAIL_CHARS);
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
    };
    drop(secrets);
    match result {
        Ok(output) => {
            db.append_event(
                project_id,
                EventKind::ReleaseConfirmed,
                json!({
                    "target_id": target.id, "name": target.name, "question_id": qid, "by": by,
                    "exit_code": 0, "duration_ms": duration_ms, "output": output,
                }),
                None,
                None,
            )?;
            note("release_run", false, qid, "released", started);
            log::info!("release done: target={}", target.id);
            Ok(ReleaseOutcome {
                target_id: target.id,
                name: target.name,
                output,
                duration_ms,
            })
        }
        Err(RunFailure { e, code, output }) => {
            let error = crate::errcode::ErrorCode::code(&e);
            failed(
                db,
                project_id,
                qid,
                &payload,
                by,
                json!({"error": error, "executed": true, "exit_code": code,
                       "duration_ms": duration_ms, "output": output}),
            )?;
            note("release_run", true, qid, &error, started);
            log::warn!("release failed: target={} {e}", target.id);
            Err(e)
        }
    }
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
    reject_by(db, project_id, qid, "owner")
}

pub fn reject_by(db: &Db, project_id: &str, qid: &str, by: &str) -> Result<(), ReleaseRunError> {
    let card = crate::cards::get(db, qid)
        .ok()
        .filter(|c| c.kind == CardKind::Release.as_str() && c.project_id == project_id)
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

/// 把每个凭据值换成 [`REDACTED`]。长的先换，免得一个值是另一个值的子串时留下半截。
pub(crate) fn redact(output: &str, secrets: &[(String, String)]) -> String {
    let mut values: Vec<&str> = secrets
        .iter()
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values
        .into_iter()
        .fold(output.to_string(), |acc, v| acc.replace(v, REDACTED))
}

fn tail(s: &str, chars: usize) -> String {
    let n = s.chars().count();
    if n <= chars {
        return s.to_string();
    }
    s.chars().skip(n - chars).collect()
}

enum RunStatus {
    /// 退出码；被信号终止为 None。
    Exited(Option<i32>),
    TimedOut,
}

struct Run {
    status: RunStatus,
    output: String,
}

/// 跑一条登记的命令行：unix `sh -c`（自成进程组，超时整组杀），Windows `cmd /C`。
/// stdin 关闭（发布命令不能停下来等输入，等就是卡死）；stdout / stderr 合流只留最新的
/// [`CAPTURE_BYTES`]。凭据作为环境变量交给子进程。
fn run_command(
    command: &str,
    dir: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Run, ReleaseRunError> {
    #[cfg(unix)]
    let mut c = {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command).process_group(0);
        c
    };
    #[cfg(not(unix))]
    let mut c = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    c.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in env {
        c.env(name, value);
    }
    // 只记错误种类：spawn 错误文案可能带命令片段。
    let mut child = c
        .spawn()
        .map_err(|e| ReleaseRunError::ReleaseSpawn(format!("{:?}", e.kind())))?;
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let pipes: Vec<Box<dyn Read + Send>> = [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    let readers = pipes.len();
    for mut pipe in pipes {
        let buf = buf.clone();
        let done = done_tx.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = buf.lock().unwrap_or_else(|p| p.into_inner());
                b.extend_from_slice(&chunk[..n]);
                if b.len() > CAPTURE_BYTES {
                    let cut = b.len() - CAPTURE_BYTES;
                    b.drain(..cut);
                }
            }
            let _ = done.send(());
        });
    }
    drop(done_tx);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| ReleaseRunError::ReleaseSpawn(format!("{:?}", e.kind())))?
        {
            break RunStatus::Exited(status.code());
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            let _ = child.wait();
            break RunStatus::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // 命令退出后，留在后台的孙进程可能还握着管道：最多再等 2 秒收尾，不无限等。
    let grace = Instant::now() + Duration::from_secs(2);
    for _ in 0..readers {
        let left = grace.saturating_duration_since(Instant::now());
        if done_rx.recv_timeout(left).is_err() {
            break;
        }
    }
    let output =
        String::from_utf8_lossy(&buf.lock().unwrap_or_else(|p| p.into_inner())).to_string();
    Ok(Run { status, output })
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // process_group(0)：组号 = 子进程 pid；负号 = 整组（含 sh 派生的孙进程）。
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: kill(2) 只发信号，不触碰本进程内存。
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn redact_replaces_longest_first() {
        let secrets = vec![
            ("A".to_string(), "tok".to_string()),
            ("B".to_string(), "token-123".to_string()),
        ];
        assert_eq!(
            redact("x token-123 y tok", &secrets),
            format!("x {REDACTED} y {REDACTED}")
        );
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
    }

    proptest! {
        /// 脱敏不变量：任何凭据值都不会留在脱敏后的输出里，不管它出现几次、夹在什么文字中间。
        #[test]
        fn redacted_output_never_contains_a_secret(
            secret in "[A-Za-z0-9_-]{4,24}",
            parts in proptest::collection::vec("[ -~]{0,16}", 0..6),
        ) {
            let output = parts.join(&secret);
            let cleaned = redact(&output, &[("TOKEN".into(), secret.clone())]);
            prop_assert!(!cleaned.contains(&secret) || REDACTED.contains(&secret));
        }
    }
}
