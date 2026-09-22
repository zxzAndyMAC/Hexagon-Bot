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
    exit_code: Option<i32>,
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
fn sh_command(dir: &Path, spec: &crate::sandbox::SandboxSpec) -> Command {
    let mut c = Command::new("sh");
    c.current_dir(dir).env("TERM", "dumb");
    // wrap 重建 Command——stdio 配置不可经 get_* 读出，管道在包裹后设置。
    let mut w = crate::sandbox::wrap_command(&mut c, spec);
    w.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    w
}

/// 杀整棵进程树：unix 先打进程组（pgid=child pid），再杀领头兜底。
/// 进程组 kill 借 `sh -c kill`（POSIX 内建），不引 libc 依赖。
/// 非 unix 只能杀领头——孙进程可能残留，这是平台边界不是 bug。
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pgid = child.id();
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("kill -KILL -- -{pgid} 2>/dev/null"))
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// `eof_done`：stdout EOF 是否即判收尾——会话成立（sh 死则命令边界消失，
/// 哨兵永远等不到），任务不成立（退出码权威在 monitor 的 wait()；
/// 若由 EOF 置 done，oneshot 会在 wait 写回 exit_code 前抢跑——
/// 回归：bash_asks_then_executes_on_allow 曾因此拿到 -1）。
fn reader_to_ring<R: Read>(mut r: R, shared: Arc<Shared>, is_out: bool, eof_done: bool) {
    let mut hold: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
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
            Err(_) => break,
        }
    }
    let mut st = shared.st.lock().unwrap();
    if is_out && eof_done {
        st.exited = true;
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
                    st.done = true;
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
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    shared: Arc<Shared>,
    /// 同会话命令串行——一条跑完才写下一条（哨兵协议的前提）。
    cmd_lock: Mutex<()>,
    /// spawn 时的沙箱规格（票 06）：后续命令 spec 不同即拒——
    /// 换围笼=换会话名，不在活会话上偷换边界。
    spec: crate::sandbox::SandboxSpec,
}

impl Session {
    fn spawn(dir: &Path, spec: crate::sandbox::SandboxSpec) -> std::io::Result<Self> {
        let mut child = sh_command(dir, &spec).spawn()?;
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
        Ok(Self {
            child: Mutex::new(child),
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
        matches!(self.child.lock().unwrap().try_wait(), Ok(Some(_)))
    }

    fn kill(&self) {
        kill_tree(&mut self.child.lock().unwrap());
        let mut st = self.shared.st.lock().unwrap();
        st.exited = true;
        st.done = true;
        st.killed = true;
        drop(st);
        self.shared.cond.notify_all();
    }
}

/// 进程表：Workbench 持一份（Arc 共享进每个 ToolContext）。
/// Clone = 同一张表；Default = 独立空表（一次性 ctx 用）。
#[derive(Clone, Default)]
pub struct SessionTable {
    inner: Arc<Mutex<TableState>>,
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
    /// 票 04：表级口子回调（Workbench 开库后挂一次）+ 当次调用归属。
    /// exec 前 set_call_meta、exec 后 clear；spawn/exec_on 起 Shared 时
    /// 把当前 (tap, meta) 绑进去。
    tap: Option<OutputTap>,
    meta: Option<(String, Option<String>)>,
}

/// 任务句柄：monitor 线程独占 Child 阻塞等退出码，句柄只留 pid + shared。
/// kill 走 pgid 信号不打锁（wait 随信号返回由 monitor 收割）。
pub struct TaskHandle {
    pid: u32,
    shared: Arc<Shared>,
}

impl SessionTable {
    /// 票 04：Workbench 开库后挂一次输出口子（壳层 emit 到 webview）。
    pub fn set_output_tap(&self, cb: Option<OutputTapCb>) {
        self.inner.lock().unwrap().tap = cb.map(|c| Arc::new(Mutex::new(c)) as OutputTap);
    }

    /// exec_and_log 包边：调用前登记归属，spawn/run_in 据此把
    /// (tap, meta) 绑进新 Shared；调用结束 clear 防陈旧 meta 串线。
    pub fn set_call_meta(&self, agent_id: &str, seq: Option<&str>) {
        self.inner.lock().unwrap().meta = Some((agent_id.into(), seq.map(Into::into)));
    }
    pub fn clear_call_meta(&self) {
        self.inner.lock().unwrap().meta = None;
    }
    fn bound_tap(&self) -> Option<BoundTap> {
        let t = self.inner.lock().unwrap();
        match (&t.tap, &t.meta) {
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

    /// 出网裁决 → 沙箱规格；不可用即裸跑并留事件（ADR 0058-4：
    /// 不可用不误放——Exec 必问卡仍是真边界，沙箱只是围笼）。
    fn spec_for(&self, db: &Db, ctx: &ToolContext, net: bool) -> crate::sandbox::SandboxSpec {
        let spec = crate::sandbox::spec_for(&ctx.repo_root, &ctx.owned_globs, net);
        if spec == crate::sandbox::SandboxSpec::Unavailable {
            emit(
                db,
                ctx,
                "sandbox_unavailable",
                json!({"note": crate::sandbox::status().note}),
            );
        }
        spec
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
        let spec = self.spec_for(db, ctx, net);
        let task = spawn_task_handle(&ctx.repo_root, cmd, &spec, self.bound_tap())
            .map_err(|e| ToolError::Exec(format!("spawn sh: {e}")))?;
        let deadline = Instant::now() + timeout;
        let timed_out = wait_done(&task.shared, deadline);
        if timed_out {
            kill_pid_group(task.pid);
            wait_done(&task.shared, Instant::now() + Duration::from_secs(2));
            emit(
                db,
                ctx,
                "exec_timeout",
                json!({"cmd": head(cmd), "timeout_ms": timeout.as_millis() as u64}),
            );
        }
        Ok(task_result(&task, timed_out, ctx))
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
            emit(db, ctx, "session_exited", json!({"session": name}));
            let fresh = Session::spawn(&ctx.repo_root, spec.clone())
                .map_err(|e| ToolError::Exec(format!("respawn sh: {e}")))?;
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
            drop(t);
            emit(db, ctx, "session_exited", json!({"session": name}));
            t = self.inner.lock().unwrap();
            t.sessions.remove(name);
        }
        let s = Session::spawn(&ctx.repo_root, spec.clone())
            .map_err(|e| ToolError::Exec(format!("spawn sh: {e}")))?;
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
        if let Err(e) = sess.stdin.lock().unwrap().write_all(script.as_bytes()) {
            let mut st = sess.shared.st.lock().unwrap();
            st.nonce = None;
            return Err(ToolError::Exec(format!("write to session {name}: {e}")));
        }
        // 票 04：会话 Shared 跨命令复用——本条命令期间口子绑当前
        // 调用归属，收尾即摘，防下一条命令吃到上条的 seq。
        *sess.shared.tap.lock().unwrap() = self.bound_tap();
        let deadline = Instant::now() + timeout;
        let timed_out = wait_done(&sess.shared, deadline);
        *sess.shared.tap.lock().unwrap() = None;
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
        let spec = self.spec_for(db, ctx, net);
        let task = spawn_task_handle(&ctx.repo_root, cmd, &spec, self.bound_tap())
            .map_err(|e| ToolError::Exec(format!("spawn task: {e}")))?;
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
        Ok(json!({"task_id": id, "background": true}))
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
            "done": st.done,
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
                kill_pid_group(h.pid);
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
            if !already {
                s.kill();
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
            kill_pid_group(h.pid);
        }
    }
}

/// 任务 spawn：monitor 线程独占 Child 阻塞等退出码；
/// kill 走 pgid 信号（不碰 child 锁，wait 随信号返回）。
fn spawn_task_handle(
    dir: &Path,
    cmd: &str,
    spec: &crate::sandbox::SandboxSpec,
    tap: Option<BoundTap>,
) -> std::io::Result<TaskHandle> {
    let mut c = sh_command(dir, spec);
    c.arg("-c").arg(cmd);
    let mut child = c.spawn()?;
    let pid = child.id();
    // 任务的 Shared 就是这条进程的——口子随 spawn 绑死，进程终身
    // 输出都记在这个 seq 名下（含后台任务活得比调用久的情形）。
    let shared = Shared::new(tap);
    let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    std::thread::spawn({
        let s = shared.clone();
        move || reader_to_ring(out, s, true, false)
    });
    std::thread::spawn({
        let s = shared.clone();
        move || reader_to_ring(err, s, false, false)
    });
    std::thread::spawn({
        let s = shared.clone();
        move || {
            let status = child.wait();
            let mut st = s.st.lock().unwrap();
            st.done = true;
            st.exited = true;
            st.exit_code = status.ok().and_then(|x| x.code());
            drop(st);
            s.cond.notify_all();
        }
    });
    Ok(TaskHandle { pid, shared })
}

/// unix 下 pgid==pid（spawn 时 process_group(0)）；非 unix 退化为杀单进程。
fn kill_pid_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "kill -KILL -- -{pid} 2>/dev/null; kill -KILL {pid} 2>/dev/null"
            ))
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

fn wait_done(shared: &Shared, deadline: Instant) -> bool {
    let mut st = shared.st.lock().unwrap();
    loop {
        if st.done {
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
