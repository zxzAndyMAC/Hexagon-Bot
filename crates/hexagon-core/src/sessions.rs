//! 会话化终端（agent-senses 票 05 / ADR 0058-3）：持久 sh 会话 + 后台任务。
//!
//! 出处与边界：
//! - **哨兵切边界**（ADR 0058-3）：会话是一条 piped sh，命令边界靠写入
//!   `echo "__HX_DONE_<nonce>_<$?>__"` 后由 stdout 读线程识别剥除。
//!   nonce 每命令随机——模型输出里出现 `__HX_DONE_` 字样不会误触发
//!   （测试钉死）。局限诚实声明：无 PTY，交互式程序（要 tty 的
//!   top/vim/ssh 密码提示）在会话里行为不可预期；需要 PTY 时换
//!   portable-pty，接缝就在 spawn 一处。
//! - **进程组**（unix `process_group(0)`）：每条命令/任务自成进程组，
//!   超时与 kill 打整个组——`sleep & disown` 这类子进程不留孤儿。
//!   非 unix 平台退化为只杀领头进程（显式 degrade，不假装隔离）。
//! - **会话表挂 Workbench 生命周期**：`Workbench` Drop 时 `kill_all`，
//!   项目关闭/切换不留 sh 孤儿。上下文经 `ToolContext.sessions` 注入，
//!   临时构造的 ctx 拿到的是独立空表（一次性路径无共享需求）。
//! - 权限轴不变：session/background/timeout_ms 不进形状记忆——记忆
//!   只认 `cmd`（票 05-④）。

use crate::db::Db;
use crate::tools::{spill_trim, ToolContext, ToolError, BASH_OUTPUT_CAP};
use crate::trace::EventKind;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// 每会话/任务的环形输出缓冲：溢出丢最旧并置 `lost` 标记。
const RING_CAP: usize = 256 * 1024;

static NONCE_SEQ: AtomicU64 = AtomicU64::new(0);

// ---------- 环形缓冲 ----------

/// 绝对游标语义的环形字节缓冲：`base` = buf[0] 的全流偏移；
/// 游标越过已丢弃区段时 clamp 到现存最旧字节并报 `lost`。
#[derive(Default)]
struct Ring {
    buf: VecDeque<u8>,
    base: u64,
}

impl Ring {
    fn push(&mut self, data: &[u8]) {
        if self.buf.len() + data.len() > RING_CAP {
            let drop = (self.buf.len() + data.len() - RING_CAP).min(self.buf.len());
            for _ in 0..drop {
                self.buf.pop_front();
                self.base += 1;
            }
        }
        self.buf.extend(data);
    }

    /// 流末尾的绝对偏移（下一字节游标）。
    fn end(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    /// 从绝对游标读增量；游标落在已丢弃区 → 返回现存全部 + lost=true。
    fn read_from(&self, cursor: u64) -> (Vec<u8>, u64, bool) {
        let lost = cursor < self.base;
        let start = if lost {
            0
        } else {
            (cursor - self.base) as usize
        };
        let out: Vec<u8> = self.buf.iter().skip(start).copied().collect();
        (out, self.end(), lost)
    }
}

// ---------- 输出瞬时口子（exec-cards 票 04）----------

/// bash 输出瞬时增量，壳层 emit 到 webview。与 TurnDelta 同纪律：
/// **不落库、只增不重放**——持久层照旧是 tool_result 事件全量。
/// `seq` 对 tool_called 载荷的调用序（"r{round}:i{index}"），UI 据此
/// 把流挂到在途调用卡；None=非回合路径（owner resolve 等不经重放的
/// 调用），UI 回退按该 agent 最新在途 bash 匹配。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ToolOutputDelta {
    pub agent_id: String,
    pub seq: Option<String>,
    /// "stdout" | "stderr"
    pub stream: String,
    pub text: String,
}

/// 口子回调：reader 线程跨线程调用 → Send 要求落在此层（Workbench
/// hook 存的是 Box<dyn FnMut + Send>，包 Arc<Mutex> 供多 reader 共享）。
pub type OutputTapCb = Box<dyn FnMut(&ToolOutputDelta) + Send>;
pub type OutputTap = Arc<Mutex<OutputTapCb>>;

/// 绑定到某次调用的口子：归属元数据 + 回调。后台任务的 Shared 活得
/// 比调用久——其输出终身记到 spawn 时那个 seq 名下（票 04 裁决：
/// 后台进程就是那 call 的子嗣，归属随进程不随时间窗）。
struct BoundTap {
    agent_id: String,
    seq: Option<String>,
    cb: OutputTap,
}

// ---------- 共享状态 ----------

struct Shared {
    st: Mutex<SharedState>,
    cond: Condvar,
    /// 输出口子（票 04）：reader 线程每段字节顺手推 UI。
    tap: Mutex<Option<BoundTap>>,
}

#[derive(Default)]
struct SharedState {
    out: Ring,
    err: Ring,
    /// 会话当前命令的哨兵 nonce；None = 命令间隙（输出原样入环）。
    /// 后台任务恒 None（任务边界=进程退出，无需哨兵）。
    nonce: Option<String>,
    /// 前台命令/任务已收尾（哨兵到 / 进程退出）。
    done: bool,
    stdout_eof: bool,
    stderr_eof: bool,
    /// I3: a failed pipe read is not EOF or proof of command completion.
    read_error: Option<&'static str>,
    exit_code: Option<i32>,
    // Ticket04 (2026-10-01): timing the entire tool call misclassified sandbox
    // setup/cleanup jitter as an application regression. Measure the owned
    // benchmark leader with a host monotonic clock; still await full cleanup.
    command_started: Option<Instant>,
    command_elapsed_ms: Option<f64>,
    /// 子进程已退出（stdout EOF 或 wait 观察到）。
    exited: bool,
    killed: bool,
}

impl Shared {
    fn new(tap: Option<BoundTap>) -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(SharedState::default()),
            cond: Condvar::new(),
            tap: Mutex::new(tap),
        })
    }
}

// ---------- 子进程 ----------

/// agent 命令的唯一 Command 出口（票 06 漏斗证明点：grep `Command::new`
/// 在本文件只命中这里与 kill 辅助——kill 是宿主侧工具进程不该进沙箱）。
/// wrap_command 会重建 Command 并自带 process_group，故裸 Command 不设组。
fn sh_command(dir: &Path, spec: &crate::sandbox::SandboxSpec) -> std::io::Result<Command> {
    let mut c = Command::new("sh");
    c.current_dir(dir).env("TERM", "dumb");
    // wrap 重建 Command——stdio 配置不可经 get_* 读出，管道在包裹后设置。
    // Reliability 15: inherited process-group confinement also blocks
    // setsid/setpgid and posix_spawn escapes; group kill alone is insufficient.
    #[cfg(target_os = "macos")]
    let fenced = match spec {
        crate::sandbox::SandboxSpec::Seatbelt(profile) => crate::sandbox::SandboxSpec::Seatbelt(
            format!("{profile}\n{}", crate::sandbox::PROCESS_GROUP_RULES),
        ),
        other => other.clone(),
    };
    #[cfg(target_os = "macos")]
    let spec = &fenced;
    let mut w = crate::sandbox::wrap_command(&mut c, spec)?;
    w.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(w)
}

// Reliability 15: observe without reaping so the owned leader pins the PID
// until the group has been signalled. A cached PID after wait() can be reused.
#[cfg(unix)]
fn child_exited(child: &Child) -> std::io::Result<bool> {
    // SAFETY: zeroed siginfo is writable; P_PID refers to our unreaped child.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } != 0)
}
#[cfg(not(unix))]
fn child_exited(_child: &Child) -> std::io::Result<bool> {
    Ok(false)
}

fn process_group_gone(pid: u32) -> bool {
    #[cfg(unix)]
    {
        (unsafe { libc::kill(-(pid as i32), 0) }) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn finish_child(
    mut child: Child,
    lease: Option<std::fs::File>,
) -> std::io::Result<std::process::ExitStatus> {
    let pid = child.id();
    kill_pid_group(pid);
    let _ = child.kill();
    let status = child.wait();
    // After reaping never signal this numeric PID again. Group disappearance
    // proves that even redirected/double-fork children cannot write later.
    // False busy costs a retry; false free costs an unreviewed side effect.
    if let Some(lease) = lease {
        #[cfg(unix)]
        {
            let gone = move || process_group_gone(pid);
            if !gone() {
                std::thread::spawn(move || {
                    while !gone() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    drop(lease);
                });
                return status;
            }
        }
        drop(lease);
    }
    status
}

/// Named commands use a nonce sentinel; shell EOF still awaits monitor status
/// and both drains, exactly as a oneshot does. `eof_done` also flushes a held
/// partial sentinel suffix when the persistent shell's output pipe closes.
fn reader_to_ring<R: Read>(mut r: R, shared: Arc<Shared>, is_out: bool, eof_done: bool) {
    let mut hold: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut reached_eof = false;
    loop {
        match r.read(&mut buf) {
            Ok(0) => {
                reached_eof = true;
                break;
            }
            Ok(n) => {
                let mut st = shared.st.lock().unwrap();
                if is_out && st.nonce.is_some() {
                    hold.extend_from_slice(&buf[..n]);
                    feed_out(&mut hold, &mut st);
                } else if is_out {
                    st.out.push(&buf[..n]);
                } else {
                    st.err.push(&buf[..n]);
                }
                drop(st);
                shared.cond.notify_all();
                // 票 04：瞬时口子——字节原样推 UI 展示层，utf8 有损可接受。
                // 锁序：st 已放，tap 锁独立；回调只做 emit 不回探会话表。
                if let Some(t) = shared.tap.lock().unwrap().as_ref() {
                    let d = ToolOutputDelta {
                        agent_id: t.agent_id.clone(),
                        seq: t.seq.clone(),
                        stream: if is_out { "stdout" } else { "stderr" }.into(),
                        text: String::from_utf8_lossy(&buf[..n]).into_owned(),
                    };
                    (t.cb.lock().unwrap())(&d);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                let mut st = shared.st.lock().unwrap();
                st.read_error = Some(if is_out { "stdout" } else { "stderr" });
                st.done = false;
                shared.cond.notify_all();
                break;
            }
        }
    }
    let mut st = shared.st.lock().unwrap();
    if is_out {
        st.stdout_eof = reached_eof;
    } else {
        st.stderr_eof = reached_eof;
    }
    // I3: named stdout EOF used to invent completion before the monitor
    // supplied the shell status. Preserve exit/respawn, await both pipe drains
    // and authoritative wait status; a normal command still uses its sentinel.
    if is_out && eof_done && !hold.is_empty() {
        st.out.push(&hold);
    }
    if st.read_error.is_none() && st.exited && st.stdout_eof && st.stderr_eof {
        st.done = true;
    }
    shared.cond.notify_all();
}

/// 哨兵剥离：hold 里找 `__HX_DONE_<nonce>_<code>__`；码后到 `__` 终止符。
/// 针尾可能跨 chunk——找不到整针时按 needle 前缀长度扣留尾字节。
fn feed_out(hold: &mut Vec<u8>, st: &mut SharedState) {
    let Some(nonce) = st.nonce.clone() else {
        return;
    };
    let needle = format!("__HX_DONE_{nonce}_").into_bytes();
    match find_subslice(hold, &needle) {
        Some(pos) => {
            st.out.push(&hold[..pos]);
            let after = pos + needle.len();
            match find_subslice(&hold[after..], b"__") {
                Some(end) => {
                    let code = String::from_utf8_lossy(&hold[after..after + end]).into_owned();
                    st.exit_code = code.trim().parse().ok();
                    // I3: a stdout sentinel cannot erase a stderr read failure.
                    st.done = st.read_error.is_none();
                    st.nonce = None;
                    let mut rest = after + end + 2;
                    if hold.get(rest) == Some(&b'\n') {
                        rest += 1;
                    }
                    hold.drain(..rest);
                    if !hold.is_empty() {
                        st.out.push(hold);
                        hold.clear();
                    }
                }
                None => {
                    hold.drain(..pos); // 终止符未到：只放针前字节
                }
            }
        }
        None => {
            let keep = partial_suffix_len(hold, &needle);
            let emit = hold.len() - keep;
            st.out.push(&hold[..emit]);
            hold.drain(..emit);
        }
    }
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// hold 末尾与 needle 前缀重叠的最长长度（针跨 chunk 的扣留量）。
fn partial_suffix_len(hold: &[u8], needle: &[u8]) -> usize {
    let max = needle.len().saturating_sub(1).min(hold.len());
    (1..=max)
        .rev()
        .find(|&k| hold.ends_with(&needle[..k]))
        .unwrap_or(0)
}

// ---------- 会话与任务 ----------

struct Session {
    /// Reliability 15: a persistent shell can still have background children
    /// after a command sentinel; retain its write lease until the shell closes.
    write_lease: Arc<Mutex<Option<std::fs::File>>>,
    child: Arc<Mutex<Option<Child>>>,
    stdin: Mutex<ChildStdin>,
    shared: Arc<Shared>,
    /// 同会话命令串行——一条跑完才写下一条（哨兵协议的前提）。
    cmd_lock: Mutex<()>,
    /// spawn 时的沙箱规格（票 06）：后续命令 spec 不同即拒——
    /// 换围笼=换会话名，不在活会话上偷换边界。
    spec: crate::sandbox::SandboxSpec,
}

impl Session {
    fn spawn(ctx: &ToolContext, spec: crate::sandbox::SandboxSpec) -> std::io::Result<Self> {
        let lease = crate::tools::writeguard::repository_lock(ctx)?;
        crate::evaluation::control::checkpoint(&ctx.repo_root)?;
        let mut child = sh_command(&ctx.repo_root, &spec)?.spawn()?;
        let stdin = child.stdin.take().unwrap();
        // 会话 Shared 跨命令复用——口子不随 spawn 绑死，exec_on
        // 每条命令换绑（票 04）。
        let shared = Shared::new(None);
        let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        std::thread::spawn({
            let s = shared.clone();
            move || reader_to_ring(out, s, true, true)
        });
        std::thread::spawn({
            let s = shared.clone();
            move || reader_to_ring(err, s, false, false)
        });
        let child = Arc::new(Mutex::new(Some(child)));
        let write_lease = Arc::new(Mutex::new(Some(lease)));
        monitor_child(child.clone(), write_lease.clone(), shared.clone());
        Ok(Self {
            write_lease,
            child,
            stdin: Mutex::new(stdin),
            shared,
            cmd_lock: Mutex::new(()),
            spec,
        })
    }

    fn dead(&self) -> bool {
        let st = self.shared.st.lock().unwrap();
        if st.exited {
            return true;
        }
        drop(st);
        self.child
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|child| child_exited(child).unwrap_or(false))
    }

    fn kill(&self) {
        if let Some(child) = self.child.lock().unwrap().take() {
            let _ = finish_child(child, self.write_lease.lock().unwrap().take());
        }
        let mut st = self.shared.st.lock().unwrap();
        st.exited = true;
        st.done = true;
        st.killed = true;
        drop(st);
        self.shared.cond.notify_all();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 进程表：Workbench 持一份（Arc 共享进每个 ToolContext）。
/// Clone = 同一张表；Default = 独立空表（一次性 ctx 用）。
#[derive(Clone, Default)]
pub struct SessionTable {
    inner: Arc<Mutex<TableState>>,
    call_meta: Option<(String, Option<String>)>,
}

impl std::fmt::Debug for SessionTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let t = self.inner.lock().unwrap();
        f.debug_struct("SessionTable")
            .field("sessions", &t.sessions.len())
            .field("tasks", &t.tasks.len())
            .finish()
    }
}

#[derive(Default)]
struct TableState {
    sessions: HashMap<String, Arc<Session>>,
    tasks: HashMap<String, Arc<TaskHandle>>,
    seq: u64,
    /// Shared output callback; call attribution lives on the immutable adapter.
    tap: Option<OutputTap>,
}

/// Monitor 与取消共享未回收的 Child；拿走句柄后取消不能再信号旧 PID。
pub struct TaskHandle {
    pid: u32,
    child: Arc<Mutex<Option<Child>>>,
    shared: Arc<Shared>,
}

impl TaskHandle {
    fn kill(&self) {
        // Same lock as the monitor: never signal after it has taken/reaped Child.
        if let Some(child) = self.child.lock().unwrap().as_ref() {
            kill_pid_group(child.id());
        }
    }
}

impl SessionTable {
    fn read_result(shared: &Shared, ctx: &ToolContext) -> Result<(), ToolError> {
        let stream = shared.st.lock().unwrap().read_error;
        if let Some(stream) = stream {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "terminal_result",
                if stream == "stdout" {
                    "stdout_read_failed_unknown"
                } else {
                    "stderr_read_failed_unknown"
                },
                Instant::now(),
            );
            return Err(ToolError::Exec(format!(
                "terminal {stream} read failed after execution; outcome unknown"
            )));
        }
        Ok(())
    }
    /// 票 04：Workbench 开库后挂一次输出口子（壳层 emit 到 webview）。
    pub fn set_output_tap(&self, cb: Option<OutputTapCb>) {
        self.inner.lock().unwrap().tap = cb.map(|c| Arc::new(Mutex::new(c)) as OutputTap);
    }

    /// I3 / 2026-10-06: shared set/clear let concurrent calls steal each
    /// other's output. Share process ownership, freeze attribution per call.
    pub fn for_call(&self, agent_id: &str, seq: Option<&str>) -> Self {
        Self {
            inner: self.inner.clone(),
            call_meta: Some((agent_id.into(), seq.map(Into::into))),
        }
    }
    fn bound_tap(&self) -> Option<BoundTap> {
        let t = self.inner.lock().unwrap();
        match (&t.tap, self.call_meta.as_ref()) {
            (Some(cb), Some((a, s))) => Some(BoundTap {
                agent_id: a.clone(),
                seq: s.clone(),
                cb: cb.clone(),
            }),
            _ => None,
        }
    }

    fn next_nonce(&self) -> String {
        let n = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{n:x}{nanos:x}")
    }

    /// reliability 02 / Q3: missing isolation records a refusal; never naked execution.
    fn spec_for(&self, db: &Db, ctx: &ToolContext, net: bool) -> crate::sandbox::SandboxSpec {
        // D05/D09: only host model transport may use the network in a worker.
        let net = net && !ctx.repo_root.join(".hexagon/evaluation-worker").exists();
        let started = Instant::now();
        let read_only =
            crate::permissions::agent_role(db, ctx).as_deref() == Some(crate::pm_route::PM_ROLE);
        let spec = if read_only {
            crate::sandbox::read_only_spec(&ctx.repo_root, net)
        } else {
            crate::sandbox::spec_for(&ctx.repo_root, &ctx.owned_globs, net)
        };
        let spec = crate::design::guard_shell(db, ctx, spec);
        Self::note_spec(
            db,
            ctx,
            &spec,
            if read_only {
                "pm_read_only"
            } else {
                "owned_scope"
            },
            started,
        );
        spec
    }

    fn note_spec(
        db: &Db,
        ctx: &ToolContext,
        spec: &crate::sandbox::SandboxSpec,
        branch: &str,
        started: Instant,
    ) {
        if *spec == crate::sandbox::SandboxSpec::Unavailable {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "sandbox",
                "isolation_unavailable",
                started,
            );
            emit(
                db,
                ctx,
                "sandbox_unavailable",
                json!({
                    "note": "terminal execution blocked: required isolation unavailable or requested scope cannot be enforced",
                    "backend": crate::sandbox::status().mode
                }),
            );
        } else {
            crate::diag::note(
                crate::diag::CLASS_JUDGE,
                false,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "sandbox",
                branch,
                started,
            );
        }
    }

    /// 前台一次性执行（无会话名）：起任务形制进程 + 同步等收尾。
    /// 与 background 共用 spawn/超时/杀组路径——单一执行漏斗。
    pub fn run_oneshot(
        &self,
        db: &Db,
        ctx: &ToolContext,
        cmd: &str,
        timeout: Duration,
        net: bool,
    ) -> Result<Value, ToolError> {
        self.run_oneshot_with_spec(db, ctx, cmd, timeout, (net, None, None))
    }

    /// Host-only adapter for a separately authorized external cwd. The caller
    /// supplies an equally restrictive OS profile; models cannot set this field.
    pub(crate) fn run_oneshot_with_spec(
        &self,
        db: &Db,
        ctx: &ToolContext,
        cmd: &str,
        timeout: Duration,
        policy: (
            bool,
            Option<crate::sandbox::SandboxSpec>,
            Option<std::path::PathBuf>,
        ),
    ) -> Result<Value, ToolError> {
        let (net, host_spec, output_root) = policy;
        let observed_paths = if ctx.native_effect.is_some() {
            let mut paths = crate::evaluation::control::task_write_paths(&ctx.repo_root)?;
            paths.retain(|p| {
                ctx.owned_globs.is_empty()
                    || ctx
                        .owned_globs
                        .iter()
                        .any(|g| crate::tools::glob_match(g, p))
            });
            if crate::permissions::agent_role(db, ctx).as_deref() == Some(crate::pm_route::PM_ROLE)
            {
                paths.clear();
            }
            Some(paths)
        } else {
            None
        };
        let spec = match &observed_paths {
            Some(paths) => {
                let spec = crate::sandbox::evaluation_spec(&ctx.repo_root, paths);
                Self::note_spec(db, ctx, &spec, "evaluation_task_scope", Instant::now());
                spec
            }
            None => host_spec.unwrap_or_else(|| self.spec_for(db, ctx, net)),
        };
        let task = spawn_task_handle(ctx, cmd, &spec, self.bound_tap(), None)
            .map_err(|e| ToolError::NotExecuted(format!("spawn sh: {e}")))?;
        // D09: foreground checks used to escape kill_all because their handle
        // lived only on this stack. Keep them in the same owned-process table.
        let task = Arc::new(task);
        let id = format!("oneshot-{}", self.next_nonce());
        self.inner
            .lock()
            .unwrap()
            .tasks
            .insert(id.clone(), task.clone());
        let deadline = Instant::now() + timeout;
        let waiting = wait_controlled(&task.shared, deadline, &ctx.repo_root);
        if let Err(e) = waiting {
            task.kill();
            wait_done(&task.shared, Instant::now() + Duration::from_secs(2));
            self.inner.lock().unwrap().tasks.remove(&id);
            return Err(ToolError::Exec(format!("evaluation interrupted: {e}")));
        }
        let timed_out = waiting.unwrap();
        if let Err(error) = Self::read_result(&task.shared, ctx) {
            task.kill();
            self.inner.lock().unwrap().tasks.remove(&id);
            return Err(error);
        }
        if timed_out {
            task.kill();
            wait_done(&task.shared, Instant::now() + Duration::from_secs(2));
            emit(
                db,
                ctx,
                "exec_timeout",
                json!({"cmd": head(cmd), "timeout_ms": timeout.as_millis() as u64}),
            );
        }
        self.inner.lock().unwrap().tasks.remove(&id);
        let mut output_ctx = ctx.clone();
        if let Some(root) = output_root {
            output_ctx.repo_root = root;
        }
        let output = task_result(&task, timed_out, &output_ctx);
        if let Some(write_paths) = observed_paths.filter(|_| !timed_out) {
            // Ticket 25: the leader can be reaped before orphaned descendants.
            // Wait within the original deadline; no proof is preferable to a
            // false completion that lets a child write after the action result.
            let cleanup_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
            while !process_group_gone(task.pid) && Instant::now() < cleanup_deadline {
                crate::evaluation::control::checkpoint(&ctx.repo_root)?;
                std::thread::sleep(Duration::from_millis(10));
            }
            let st = task.shared.st.lock().unwrap();
            if process_group_gone(task.pid) && st.exit_code.is_some() {
                ctx.observe_native_effect(crate::tools::effects::NativeEffect::ShellCompleted {
                    write_paths,
                    profile_digest: crate::evaluation::config::digest(&format!("{spec:?}"))?,
                    process_id: task.pid,
                });
            }
        }
        Ok(output)
    }

    /// 命名会话执行：会话不存在/已死则（重）起；命令经哨兵切边界。
    pub fn run_in(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        cmd: &str,
        timeout: Duration,
        net: bool,
    ) -> Result<Value, ToolError> {
        let spec = self.spec_for(db, ctx, net);
        let sess = self.ensure_session(db, ctx, name, &spec)?;
        let _ser = sess.cmd_lock.lock().unwrap();
        // 会话可能在等锁期间死掉——重查一次
        if sess.dead() {
            sess.kill();
            emit(db, ctx, "session_exited", json!({"session": name}));
            let fresh = Session::spawn(ctx, spec.clone())
                .map_err(|e| ToolError::NotExecuted(format!("respawn sh: {e}")))?;
            let fresh = Arc::new(fresh);
            self.inner
                .lock()
                .unwrap()
                .sessions
                .insert(name.into(), fresh.clone());
            emit(
                db,
                ctx,
                "session_started",
                json!({"session": name, "respawn": true}),
            );
            return self.exec_on(db, ctx, name, &fresh, cmd, timeout);
        }
        // 沙箱规格不合即拒（ADR 0058-4）：不在活会话上偷换围笼边界——
        // net:false 的会话跑 net:true 命令是静默扩权。换一个会话名即可。
        if sess.spec != spec {
            return Err(ToolError::BadInput(format!(
                "session {name} runs under a different sandbox spec (net flag changed?) — use another session name"
            )));
        }
        self.exec_on(db, ctx, name, &sess, cmd, timeout)
    }

    fn ensure_session(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        spec: &crate::sandbox::SandboxSpec,
    ) -> Result<Arc<Session>, ToolError> {
        let mut t = self.inner.lock().unwrap();
        if let Some(s) = t.sessions.get(name) {
            if !s.dead() {
                return Ok(s.clone());
            }
            s.kill();
            drop(t);
            emit(db, ctx, "session_exited", json!({"session": name}));
            t = self.inner.lock().unwrap();
            t.sessions.remove(name);
        }
        let s = Session::spawn(ctx, spec.clone())
            .map_err(|e| ToolError::NotExecuted(format!("spawn sh: {e}")))?;
        let s = Arc::new(s);
        t.sessions.insert(name.into(), s.clone());
        drop(t);
        emit(db, ctx, "session_started", json!({"session": name}));
        Ok(s)
    }

    fn exec_on(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        sess: &Arc<Session>,
        cmd: &str,
        timeout: Duration,
    ) -> Result<Value, ToolError> {
        crate::evaluation::control::checkpoint(&ctx.repo_root)
            .map_err(|e| ToolError::NotExecuted(e.to_string()))?;
        let nonce = self.next_nonce();
        let start_out;
        let start_err;
        {
            let mut st = sess.shared.st.lock().unwrap();
            st.done = false;
            st.exit_code = None;
            st.nonce = Some(nonce.clone());
            start_out = st.out.end();
            start_err = st.err.end();
        }
        // nonce 先于命令落 shared——读线程从命令第一个字节起就在剥哨兵
        let script = format!("{cmd}\necho \"__HX_DONE_{nonce}_$?__\"\n");
        // Bind before writing: a short command can emit before write_all returns.
        *sess.shared.tap.lock().unwrap() = self.bound_tap();
        if let Err(e) = sess.stdin.lock().unwrap().write_all(script.as_bytes()) {
            *sess.shared.tap.lock().unwrap() = None;
            let mut st = sess.shared.st.lock().unwrap();
            st.nonce = None;
            return Err(ToolError::Exec(format!("write to session {name}: {e}")));
        }
        let deadline = Instant::now() + timeout;
        let waiting = wait_controlled(&sess.shared, deadline, &ctx.repo_root);
        if let Err(e) = waiting {
            *sess.shared.tap.lock().unwrap() = None;
            sess.kill();
            self.inner.lock().unwrap().sessions.remove(name);
            return Err(ToolError::Exec(format!("evaluation interrupted: {e}")));
        }
        let timed_out = waiting.unwrap();
        *sess.shared.tap.lock().unwrap() = None;
        if let Err(error) = Self::read_result(&sess.shared, ctx) {
            sess.kill();
            self.inner.lock().unwrap().sessions.remove(name);
            return Err(error);
        }
        // I3: shell EOF without a sentinel has no authoritative command status.
        if sess.shared.st.lock().unwrap().exit_code.is_none() && !timed_out {
            sess.kill();
            self.inner.lock().unwrap().sessions.remove(name);
            return Err(ToolError::Exec(
                "session exited without a command result; outcome unknown".into(),
            ));
        }
        if timed_out {
            sess.kill();
            emit(
                db,
                ctx,
                "exec_timeout",
                json!({"session": name, "cmd": head(cmd), "timeout_ms": timeout.as_millis() as u64}),
            );
            emit(
                db,
                ctx,
                "session_exited",
                json!({"session": name, "reason": "timeout"}),
            );
            self.inner.lock().unwrap().sessions.remove(name);
        }
        let st = sess.shared.st.lock().unwrap();
        let (out, _, lost_o) = st.out.read_from(start_out);
        let (err, _, lost_e) = st.err.read_from(start_err);
        let mut stdout = String::from_utf8_lossy(&out).into_owned();
        let mut stderr = String::from_utf8_lossy(&err).into_owned();
        if lost_o {
            stdout.insert_str(0, "[ring overflow: oldest output dropped]\n");
        }
        if lost_e {
            stderr.insert_str(0, "[ring overflow: oldest output dropped]\n");
        }
        if stdout.len() > BASH_OUTPUT_CAP {
            stdout = spill_trim(ctx, &stdout, BASH_OUTPUT_CAP);
        }
        if stderr.len() > BASH_OUTPUT_CAP {
            stderr = spill_trim(ctx, &stderr, BASH_OUTPUT_CAP);
        }
        Ok(json!({
            "session": name,
            "exit_code": st.exit_code.unwrap_or(-1),
            "timed_out": timed_out,
            "session_exited": st.exited,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }

    /// 后台任务：spawn 即回 task_id，不等收尾。
    pub fn spawn_task(
        &self,
        db: &Db,
        ctx: &ToolContext,
        cmd: &str,
        net: bool,
    ) -> Result<Value, ToolError> {
        let started = Instant::now();
        // reliability 04 / D02: tests used to inherit the parent's source-write
        // scope. Only a fresh host-created directory is writable now; callers
        // cannot designate source or a pre-existing alias as an output directory.
        let output = if ctx.subagent.is_some() {
            let root = ctx.repo_root.canonicalize()?;
            let dir = root.join(format!(
                ".hexagon-test-{}-{}",
                std::process::id(),
                self.next_nonce()
            ));
            std::fs::create_dir(&dir)?;
            Some(dir)
        } else {
            None
        };
        let spec = if let Some(output) = &output {
            let name = output.file_name().unwrap().to_string_lossy();
            let spec = crate::sandbox::spec_for(&ctx.repo_root, &[format!("{name}/**")], false);
            Self::note_spec(db, ctx, &spec, "subagent_test_outputs", started);
            spec
        } else {
            self.spec_for(db, ctx, net)
        };
        let task = spawn_task_handle(ctx, cmd, &spec, self.bound_tap(), output.as_deref())
            .map_err(|e| ToolError::NotExecuted(format!("spawn task: {e}")))?;
        let task = Arc::new(task);
        let id = {
            let mut t = self.inner.lock().unwrap();
            t.seq += 1;
            let id = format!("task-{}-{}", std::process::id(), t.seq);
            t.tasks.insert(id.clone(), task);
            id
        };
        emit(
            db,
            ctx,
            "task_started",
            json!({"task_id": id, "cmd": head(cmd)}),
        );
        Ok(json!({"task_id": id, "background": true, "output_dir": output}))
    }

    /// 增量读：`cursor_out/cursor_err` 为上次返回的游标（首读 0）。
    pub fn read_output(
        &self,
        _db: &Db,
        _ctx: &ToolContext,
        task_id: Option<&str>,
        session: Option<&str>,
        cursor_out: u64,
        cursor_err: u64,
    ) -> Result<Value, ToolError> {
        let shared = {
            let t = self.inner.lock().unwrap();
            if let Some(id) = task_id {
                t.tasks
                    .get(id)
                    .map(|h| h.shared.clone())
                    .ok_or_else(|| ToolError::BadInput(format!("unknown task: {id}")))?
            } else if let Some(name) = session {
                t.sessions
                    .get(name)
                    .map(|s| s.shared.clone())
                    .ok_or_else(|| ToolError::BadInput(format!("unknown session: {name}")))?
            } else {
                return Err(ToolError::BadInput("task_id or session required".into()));
            }
        };
        let st = shared.st.lock().unwrap();
        let (o, co, lo) = st.out.read_from(cursor_out);
        let (e, ce, le) = st.err.read_from(cursor_err);
        Ok(json!({
            "stdout": String::from_utf8_lossy(&o),
            "stderr": String::from_utf8_lossy(&e),
            "cursor_out": co,
            "cursor_err": ce,
            "lost": lo || le,
            "done": st.done && st.read_error.is_none(),
            "read_error": st.read_error,
            "outcome_unknown": st.read_error.is_some(),
            "exited": st.exited,
            "exit_code": st.exit_code,
        }))
    }

    /// 终止任务或会话；已死者幂等回报（不重杀、不落重复事件由调用方看 killed）。
    pub fn kill(
        &self,
        db: &Db,
        ctx: &ToolContext,
        task_id: Option<&str>,
        session: Option<&str>,
    ) -> Result<Value, ToolError> {
        if let Some(id) = task_id {
            let h = self
                .inner
                .lock()
                .unwrap()
                .tasks
                .get(id)
                .cloned()
                .ok_or_else(|| ToolError::BadInput(format!("unknown task: {id}")))?;
            // 幂等看 exited∥killed：kill 信号到 monitor 收割有窗口，
            // 只看 exited 会让紧跟的二次 kill 误报 killed:true。
            let already = {
                let st = h.shared.st.lock().unwrap();
                st.exited || st.killed
            };
            if !already {
                h.kill();
                h.shared.st.lock().unwrap().killed = true;
                emit(db, ctx, "task_killed", json!({"task_id": id}));
            }
            return Ok(json!({"task_id": id, "killed": !already}));
        }
        if let Some(name) = session {
            let s = self
                .inner
                .lock()
                .unwrap()
                .sessions
                .get(name)
                .cloned()
                .ok_or_else(|| ToolError::BadInput(format!("unknown session: {name}")))?;
            let already = s.dead();
            s.kill();
            if !already {
                emit(
                    db,
                    ctx,
                    "session_exited",
                    json!({"session": name, "reason": "killed"}),
                );
                self.inner.lock().unwrap().sessions.remove(name);
            }
            return Ok(json!({"session": name, "killed": !already}));
        }
        Err(ToolError::BadInput("task_id or session required".into()))
    }

    /// Workbench Drop / 项目关闭：全灭。
    pub fn kill_all(&self) {
        let mut t = self.inner.lock().unwrap();
        for (_, s) in t.sessions.drain() {
            s.kill();
        }
        for (_, h) in t.tasks.drain() {
            h.kill();
        }
    }
}

/// A real OS read fault on an owned test shell, using the production reader
/// and result classifier. No global FD manipulation or unsafe double-close.
#[cfg(test)]
pub(crate) fn terminal_os_read_failure_for_test(
    db: &Db,
    ctx: &ToolContext,
    cmd: &str,
) -> Result<Value, ToolError> {
    struct FaultAfterOutput {
        pipe: std::process::ChildStdout,
        invalid_reader: std::fs::File,
        read_once: bool,
    }
    impl Read for FaultAfterOutput {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.read_once {
                self.read_once = true;
                self.pipe.read(buf)
            } else {
                self.invalid_reader.read(buf)
            }
        }
    }
    let table = ctx.sessions.clone();
    let spec = table.spec_for(db, ctx, false);
    let mut command = sh_command(&ctx.repo_root, &spec)?;
    command.arg("-c").arg(cmd);
    let mut child = command.spawn()?;
    let pipe = child.stdout.take().unwrap();
    let shared = Shared::new(table.bound_tap());
    let invalid_reader = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(ctx.repo_root.join("read-fault-fixture"))?;
    reader_to_ring(
        FaultAfterOutput {
            pipe,
            invalid_reader,
            read_once: false,
        },
        shared.clone(),
        true,
        false,
    );
    let status = child.wait()?;
    record_process_exit(&mut shared.st.lock().unwrap(), status.code(), None);
    assert!(
        shared.st.lock().unwrap().read_error.is_some(),
        "OS fault fixture must reach the failed-read branch, not a fallback Exec"
    );
    SessionTable::read_result(&shared, ctx)?;
    Err(ToolError::Exec(
        "OS fault fixture did not fail its read".into(),
    ))
}

/// 任务 spawn：monitor 先观察退出再收割，进程组消失后才释放写租约。
fn spawn_task_handle(
    ctx: &ToolContext,
    cmd: &str,
    spec: &crate::sandbox::SandboxSpec,
    tap: Option<BoundTap>,
    output: Option<&Path>,
) -> std::io::Result<TaskHandle> {
    // D08 / reliability-18: checks already hold the host boundary across before/
    // after fingerprints. Clone that lease so descendants retain it until dead.
    let lease = match &ctx.write_lease {
        Some(lease) => lease.try_clone()?,
        None => crate::tools::writeguard::repository_lock(ctx)?,
    };
    let mut c = sh_command(&ctx.repo_root, spec)?;
    c.arg("-c").arg(cmd);
    if let Some(output) = output {
        // Host-generated paths after environment sanitization; test input
        // cannot inject arbitrary environment or filesystem capabilities.
        c.env("HEXAGON_TEST_OUTPUT", output)
            .env("TMPDIR", output)
            .env("CARGO_TARGET_DIR", output.join("target"))
            .env("XDG_CACHE_HOME", output.join("cache"))
            .env("npm_config_cache", output.join("npm-cache"))
            .env("COVERAGE_FILE", output.join(".coverage"));
    }
    // D09: acquiring the repository lease can wait across a stop.
    crate::evaluation::control::checkpoint(&ctx.repo_root)?;
    let command_started = Instant::now();
    let mut child = c.spawn()?;
    let pid = child.id();
    // 任务的 Shared 就是这条进程的——口子随 spawn 绑死，进程终身
    // 输出都记在这个 seq 名下（含后台任务活得比调用久的情形）。
    let shared = Shared::new(tap);
    shared.st.lock().unwrap().command_started = Some(command_started);
    let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    std::thread::spawn({
        let s = shared.clone();
        move || reader_to_ring(out, s, true, false)
    });
    std::thread::spawn({
        let s = shared.clone();
        move || reader_to_ring(err, s, false, false)
    });
    let child = Arc::new(Mutex::new(Some(child)));
    monitor_child(
        child.clone(),
        Arc::new(Mutex::new(Some(lease))),
        shared.clone(),
    );
    Ok(TaskHandle { pid, child, shared })
}

// Reliability 15: a named shell may exit after its command sentinel. Monitor
// lifetime independently of tool calls so an exited table entry holds no lease.
fn monitor_child(
    owned: Arc<Mutex<Option<Child>>>,
    lease: Arc<Mutex<Option<std::fs::File>>>,
    shared: Arc<Shared>,
) {
    std::thread::spawn(move || {
        let child = loop {
            let mut guard = owned.lock().unwrap();
            let Some(child) = guard.as_ref() else {
                return;
            };
            if child_exited(child).unwrap_or(true) {
                break guard.take().unwrap();
            }
            drop(guard);
            std::thread::sleep(Duration::from_millis(5));
        };
        let command_elapsed_ms = shared
            .st
            .lock()
            .unwrap()
            .command_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0);
        let status = finish_child(child, lease.lock().unwrap().take());
        let mut st = shared.st.lock().unwrap();
        record_process_exit(
            &mut st,
            status.ok().and_then(|x| x.code()),
            command_elapsed_ms,
        );
        drop(st);
        shared.cond.notify_all();
    });
}

fn record_process_exit(st: &mut SharedState, exit_code: Option<i32>, elapsed: Option<f64>) {
    // QA 13 / 2026-10-05: the monitor used to publish successful completion
    // before the independent pipe readers appended the final bytes. Short
    // --version commands then froze None and falsely drifted on the next read.
    // Keep leader timing authoritative, but await both drains within the
    // caller's existing deadline; a stalled pipe must never become success.
    st.exited = true;
    st.exit_code = exit_code;
    st.command_elapsed_ms = elapsed;
    if st.read_error.is_none() && st.stdout_eof && st.stderr_eof {
        st.done = true;
    }
}

/// unix 下 pgid==pid（spawn 时 process_group(0)）；非 unix 退化为杀单进程。
pub(crate) fn kill_pid_group(pid: u32) {
    #[cfg(unix)]
    {
        // All callers hold an unreaped Child. Direct signals avoid launching
        // another shell during cancellation (Reliability 10/15 deadlines).
        // SAFETY: kill takes numeric process IDs only; negative ID selects our
        // confined process group and the second signal covers its leader.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

// D09: a silent child cannot keep a stopped run blocked until its tool timeout.
fn wait_controlled(shared: &Shared, deadline: Instant, root: &Path) -> std::io::Result<bool> {
    if !root.join(".hexagon/evaluation-worker").exists() {
        return Ok(wait_done(shared, deadline));
    }
    loop {
        crate::evaluation::control::checkpoint(root)?;
        if !wait_done(
            shared,
            deadline.min(Instant::now() + Duration::from_millis(100)),
        ) {
            return Ok(false);
        }
        if Instant::now() >= deadline {
            return Ok(true);
        }
    }
}

fn wait_done(shared: &Shared, deadline: Instant) -> bool {
    let mut st = shared.st.lock().unwrap();
    loop {
        if st.done || st.read_error.is_some() {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let step = (deadline - now).min(Duration::from_millis(50));
        let (g, _) = shared.cond.wait_timeout(st, step).unwrap();
        st = g;
    }
}

fn task_result(t: &TaskHandle, timed_out: bool, ctx: &ToolContext) -> Value {
    let st = t.shared.st.lock().unwrap();
    let (o, _, lo) = st.out.read_from(0);
    let (e, _, le) = st.err.read_from(0);
    let mut stdout = String::from_utf8_lossy(&o).into_owned();
    let mut stderr = String::from_utf8_lossy(&e).into_owned();
    if lo {
        stdout.insert_str(0, "[ring overflow: oldest output dropped]\n");
    }
    if le {
        stderr.insert_str(0, "[ring overflow: oldest output dropped]\n");
    }
    if stdout.len() > BASH_OUTPUT_CAP {
        stdout = spill_trim(ctx, &stdout, BASH_OUTPUT_CAP);
    }
    if stderr.len() > BASH_OUTPUT_CAP {
        stderr = spill_trim(ctx, &stderr, BASH_OUTPUT_CAP);
    }
    json!({
        "exit_code": st.exit_code.unwrap_or(-1),
        "timed_out": timed_out,
        "command_elapsed_ms": st.command_elapsed_ms,
        "stdout": stdout,
        "stderr": stderr,
    })
}

fn head(cmd: &str) -> String {
    cmd.chars().take(200).collect()
}

fn emit(db: &Db, ctx: &ToolContext, kind: &str, payload: Value) {
    let mut p = payload;
    p["kind"] = json!(kind);
    if let Err(e) = db.append_event(
        &ctx.project_id,
        EventKind::System,
        p,
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
    ) {
        log::warn!("session event {kind} not traced: {e}");
    }
}

#[cfg(test)]
mod output_completion_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn oneshot_reader_error_cannot_publish_successful_completion() {
        struct FailedRead;
        impl Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("controlled pipe failure"))
            }
        }
        let shared = Shared::new(None);
        record_process_exit(&mut shared.st.lock().unwrap(), Some(0), Some(1.0));
        reader_to_ring(
            std::io::Cursor::new(b"stderr"),
            shared.clone(),
            false,
            false,
        );
        reader_to_ring(FailedRead, shared.clone(), true, false);
        let state = shared.st.lock().unwrap();
        assert!(state.exited);
        assert!(!state.done);
        assert!(!state.stdout_eof);
    }

    #[test]
    fn separate_command_contexts_never_steal_output_attribution() {
        let table = SessionTable::default();
        let deltas = Arc::new(Mutex::new(Vec::new()));
        let observed = deltas.clone();
        table.set_output_tap(Some(Box::new(move |delta| {
            observed.lock().unwrap().push((
                delta.agent_id.clone(),
                delta.seq.clone(),
                delta.text.clone(),
            ));
        })));
        for index in 0..100 {
            let aseq = format!("a{index}");
            let bseq = format!("b{index}");
            let a = table.for_call("parent-a", Some(&aseq));
            let b = table.for_call("parent-b", Some(&bseq));
            let ashared = Shared::new(a.bound_tap());
            let bshared = Shared::new(b.bound_tap());
            reader_to_ring(std::io::Cursor::new(b"A"), ashared, true, false);
            reader_to_ring(std::io::Cursor::new(b"B"), bshared, true, false);
            let actual = deltas.lock().unwrap();
            assert_eq!(
                actual[index * 2],
                ("parent-a".into(), Some(aseq), "A".into())
            );
            assert_eq!(
                actual[index * 2 + 1],
                ("parent-b".into(), Some(bseq), "B".into())
            );
        }
    }

    #[test]
    fn named_reader_error_must_reach_the_tool_as_uncertain() {
        struct FailedRead;
        impl Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("controlled pipe failure"))
            }
        }
        let shared = Shared::new(None);
        reader_to_ring(FailedRead, shared.clone(), true, true);
        let state = shared.st.lock().unwrap();
        assert!(
            !state.done,
            "reader error cannot masquerade as named command completion"
        );
        assert!(!state.stdout_eof);
    }

    proptest! {
        #[test]
        fn named_sentinel_cannot_override_pipe_error_in_either_order(
            error_first in any::<bool>(), code in any::<i32>(), text in "[a-zA-Z0-9]{0,80}"
        ) {
            struct FailedRead;
            impl Read for FailedRead {
                fn read(&mut self,_:&mut[u8])->std::io::Result<usize> { Err(std::io::Error::other("fixture")) }
            }
            let shared = Shared::new(None);
            shared.st.lock().unwrap().nonce = Some("fixture-nonce".into());
            let output = format!("{text}__HX_DONE_fixture-nonce_{code}__\n");
            if error_first { reader_to_ring(FailedRead,shared.clone(),false,false); }
            reader_to_ring(std::io::Cursor::new(output.as_bytes()),shared.clone(),true,true);
            if !error_first { reader_to_ring(FailedRead,shared.clone(),false,false); }
            let st = shared.st.lock().unwrap();
            prop_assert!(!st.done);
            prop_assert_eq!(st.read_error,Some("stderr"));
            prop_assert_eq!(st.exit_code,Some(code));
            prop_assert_eq!(st.out.read_from(0).0,text.as_bytes());
        }
    }

    proptest! {
        #[test]
        fn pipe_failure_never_becomes_success_after_monitor_or_eof(
            is_out in any::<bool>(), named in any::<bool>(), exit_first in any::<bool>(), code in any::<i32>()
        ) {
            struct FailedRead;
            impl Read for FailedRead { fn read(&mut self,_:&mut[u8])->std::io::Result<usize> { Err(std::io::Error::other("fixture")) } }
            let shared = Shared::new(None);
            if exit_first { record_process_exit(&mut shared.st.lock().unwrap(),Some(code),None); }
            reader_to_ring(FailedRead,shared.clone(),is_out,named);
            reader_to_ring(std::io::Cursor::new(b""),shared.clone(),!is_out,false);
            record_process_exit(&mut shared.st.lock().unwrap(),Some(code),None);
            prop_assert!(!shared.st.lock().unwrap().done);
            prop_assert!(shared.st.lock().unwrap().read_error.is_some());
            prop_assert!(!wait_done(&shared,Instant::now()+Duration::from_secs(1)));
        }
    }

    proptest! {
        #[test]
        fn oneshot_completion_requires_exit_and_both_drains_in_any_order(
            order in 0usize..6, code in any::<i32>(), text in "[a-zA-Z0-9]{0,80}"
        ) {
            let permutations = [[0,1,2], [0,2,1], [1,0,2], [1,2,0], [2,0,1], [2,1,0]];
            let shared = Shared::new(None);
            for (position, step) in permutations[order].iter().enumerate() {
                match step {
                    0 => record_process_exit(&mut shared.st.lock().unwrap(), Some(code), Some(1.0)),
                    1 => reader_to_ring(std::io::Cursor::new(text.as_bytes()), shared.clone(), true, false),
                    _ => reader_to_ring(std::io::Cursor::new(b"stderr"), shared.clone(), false, false),
                }
                prop_assert_eq!(shared.st.lock().unwrap().done, position == 2);
            }
            let state = shared.st.lock().unwrap();
            prop_assert_eq!(state.exit_code, Some(code));
            prop_assert_eq!(state.out.read_from(0).0, text.as_bytes());
            prop_assert_eq!(state.err.read_from(0).0, b"stderr");
        }
    }

    #[test]
    fn oneshot_completion_waits_for_delayed_output_readers() {
        // QA 13 (2026-10-05): version probes raced the pipe readers under a
        // parallel evaluation suite. Hold both actual pipes until the owned
        // leader has exited; completion must not expose empty successful output.
        let root = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let ctx = ToolContext::owner(&db, root.path());
        let table = SessionTable::default();
        let spec = table.spec_for(&db, &ctx, false);
        let mut command = sh_command(root.path(), &spec).unwrap();
        command
            .arg("-c")
            .arg("printf 'Python 3.9.6\\n'; printf 'probe warning\\n' >&2");
        let mut child = command.spawn().unwrap();
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        let shared = Shared::new(None);
        monitor_child(
            Arc::new(Mutex::new(Some(child))),
            Arc::new(Mutex::new(None)),
            shared.clone(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = shared.st.lock().unwrap();
        while !state.exited {
            assert!(Instant::now() < deadline, "owned leader did not exit");
            state = shared
                .cond
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap()
                .0;
        }
        assert_eq!(state.exit_code, Some(0));
        drop(state);
        assert!(
            wait_done(&shared, Instant::now() + Duration::from_millis(20)),
            "leader exit exposed unread output as complete"
        );
        reader_to_ring(out, shared.clone(), true, false);
        assert!(
            wait_done(&shared, Instant::now() + Duration::from_millis(20)),
            "stdout EOF exposed unread stderr as complete"
        );
        reader_to_ring(err, shared.clone(), false, false);
        assert!(!wait_done(&shared, Instant::now() + Duration::from_secs(1)));
        let state = shared.st.lock().unwrap();
        assert_eq!(state.out.read_from(0).0, b"Python 3.9.6\n");
        assert_eq!(state.err.read_from(0).0, b"probe warning\n");
    }
}
