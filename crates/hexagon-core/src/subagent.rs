//! 子代理（code-search-and-subagent 票 04–07）：父代理在当前激活里派遣
//! 有界子代理做读/搜/比/测，交回简洁结论 + 实际读过的路径。
//!
//! 前身是 US36「研究助手」（research.rs）：同一条结构性约束思路，面放宽。
//! 结构性约束（不靠提示词自觉）：
//! - 嵌套注册表 = `Registry::subagent_scope(isolated)`：fs_read/artifact_read/
//!   load_skill + fs_find/fs_grep/sem_search/web_search + run_test +
//!   本次勾选的 mcp:* —— 没有写/bash/git/web_fetch/subagent →
//!   不可写、不可跑任意命令、不可外带、不可再派生；
//! - 容量：同一激活至多 MAX_CHILDREN 个并发子代理，第 5 个派遣立即拒
//!   （不排队——排队会把「现在就要答案」变成不可控的挂起）；
//! - 嵌套 ctx 继承父 agent_id/stage_run_id/owned_globs/sessions/tasks →
//!   事件、用量、步数全部记父 Agent；可见回复不落时间线，只回填回执格；
//! - 父休眠子代理不跑（派遣前置闸 + 嵌套回合轮顶复查）；
//! - mcp:* 勾选集只在本次派遣生效：不写授权表、不沉淀、不带到下一次。
//!
//! 任务清单（票 07）挂同一模块：它只为「父代理派了什么活/收回什么」服务，
//! 同 root 续接保留，确认的手建待办由既有工具回执恢复；不碰阶段指针。

use crate::db::Db;
use crate::provider::ModelProvider;
use crate::tools::{CallOutcome, Registry, ToolContext, ToolError};
use crate::turn::{run_turn_streaming, TurnOutcome};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 同一激活里并发子代理上限（票 04）：第五个派遣立即拒，不排队。
pub const MAX_CHILDREN: usize = 4;
/// 交回父代理的回执预算（字符）：子代理的职责是提炼，不是搬运。
const ANSWER_CAP: usize = 4_000;
/// run_test 单条命令的默认/封顶超时——子代理没有负责人可问，测试挂死
/// 不能无限占住容量位。
const RUN_TEST_TIMEOUT_MS: u64 = 120_000;

// ---------- 派遣域（ctx.subagent 的非空即「本回合是子代理」）----------

#[derive(Clone)]
pub struct Scope {
    /// Owner Q11: later project widening cannot expand an already dispatched
    /// child; revocation still takes effect through the current permission gate.
    pub approval_mode: crate::approval_mode::ApprovalMode,
    pub permission_rules: Arc<HashSet<String>>,
    /// 停止旗：tasks stop / 激活清场 / Workbench Drop 置位；
    /// 嵌套回合在流 delta 缝与轮顶检查。
    pub halt: Arc<AtomicBool>,
    /// 回执格：persist_visible 在子代理域不写时间线，把最终回复写这里。
    pub answer: Arc<Mutex<Option<String>>>,
    /// 本次派遣勾选的 mcp:* 工具名集（票 06）。集合外的 mcp 在
    /// 权限层即拒；集合只活在本次派遣的 ctx 上。
    pub mcp: Arc<HashSet<String>>,
    /// 实际读过的仓内路径（fs_read/artifact_read 成功执行时自记）。
    /// 引用 = 这格的内容，不是模型自称读过什么。
    pub reads: Arc<Mutex<Vec<String>>>,
}

// ---------- 激活任务清单（票 07）----------

/// Benchmark I3 / ticket 04: a stage is shared by parents; a resume turn is
/// not a new activation. The host validates and supplies the stable root.
/// None is reserved for direct host/test calls, never substituted with a parent id.
pub fn activation_key(ctx: &ToolContext) -> String {
    json!([
        ctx.project_id,
        ctx.agent_id,
        ctx.stage_run_id,
        ctx.activation_root_turn_id
    ])
    .to_string()
}

fn same_parent(a: &str, b: &str) -> bool {
    match (
        serde_json::from_str::<Value>(a),
        serde_json::from_str::<Value>(b),
    ) {
        (Ok(Value::Array(a)), Ok(Value::Array(b))) if a.len() == 4 && b.len() == 4 => {
            a[0] == b[0] && a[1] == b[1]
        }
        _ => a == b, // Existing internal direct-call keys remain isolated.
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskKind {
    /// 父代理手建的备忘条目。
    Manual,
    /// 子代理派遣。
    Subagent,
}

struct Task {
    key: String,
    id: String,
    title: String,
    kind: TaskKind,
    /// open|running|done|failed|stopped
    status: &'static str,
    result: Option<Value>,
    error: Option<String>,
    halt: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    worker_pending: bool,
    retired: bool,
}

/// 激活内任务清单。`Clone` 共享同一内表——ctx 各层克隆看到的
/// 是同一块板（父与子代理同板；子代理看不到 tasks 工具所以无妨）。
#[derive(Clone, Default)]
pub struct TaskBoard {
    inner: Arc<Mutex<Vec<Task>>>,
    seq: Arc<Mutex<u64>>,
}

/// dispatch 落位成功的回执：任务 id + 三格共享面（halt 旗 / 回执格 /
/// 实读清单）——嵌套回合的 Scope 与板上的条目共享同一组 Arc。
pub struct Dispatched {
    pub id: String,
    pub halt: Arc<AtomicBool>,
    pub answer: Arc<Mutex<Option<String>>>,
    pub reads: Arc<Mutex<Vec<String>>>,
}

impl TaskBoard {
    /// Same-root resumes retain their board. A fresh root retires only this
    /// parent's old board; a halt request must not free a live worker's slot.
    pub fn begin_activation(&self, key: &str) {
        self.reap_finished();
        let mut tasks = self.inner.lock().unwrap();
        for t in tasks
            .iter_mut()
            .filter(|t| t.key != key && same_parent(&t.key, key))
        {
            t.halt.store(true, Ordering::Relaxed);
            t.retired = true;
        }
        tasks.retain(|t| !t.retired || Self::in_flight(t));
    }

    fn in_flight(t: &Task) -> bool {
        t.kind == TaskKind::Subagent
            && (t.status == "running"
                || t.worker_pending && t.handle.as_ref().is_none_or(|h| !h.is_finished()))
    }

    /// Only join proven finished workers, outside the board lock. Publishing a
    /// result precedes thread destructors, so result status alone is insufficient.
    fn reap_finished(&self) {
        let handles = {
            let mut tasks = self.inner.lock().unwrap();
            tasks
                .iter_mut()
                .filter_map(|t| {
                    if t.handle.as_ref().is_some_and(|h| h.is_finished()) {
                        t.worker_pending = false;
                        Some((t.key.clone(), t.id.clone(), t.handle.take().unwrap()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for (key, id, handle) in handles {
            if handle.join().is_err() {
                self.finish(
                    &key,
                    &id,
                    "failed",
                    None,
                    Some("child worker panicked".into()),
                );
            }
        }
        self.inner
            .lock()
            .unwrap()
            .retain(|t| !t.retired || Self::in_flight(t));
    }

    /// Confirmed receipts only; validation and application share this parser.
    /// I3 fail-closed: refusing an incomplete chain costs one investigation;
    /// inventing a completed child hides a lost task. Never replay unknown work.
    fn restored_tasks(operations: &[(Value, Value)]) -> Result<Vec<Task>, ToolError> {
        let mut restored: Vec<Task> = Vec::new();
        for (input, output) in operations {
            if input["host_tool"] == "subagent" {
                // I2/I3 Spec sibling 2026-10-06: native refusals succeed as
                // model feedback but do not allocate a child. Requiring their
                // task_id blocked resume; ignoring dispatched:false instead
                // accepted contradictory fake done receipts. Accept only the
                // three producer contracts, retaining the input task gate.
                // Fail closed: one investigation is cheaper than invented
                // completion hiding an unfinished task. Historical feedback
                // stays in tool_actions/history; this only projects the board.
                if output["dispatched"] == false {
                    let expected = match output["reason"].as_str() {
                        Some("capacity full") => json!({
                            "dispatched":false,"reason":"capacity full","running":MAX_CHILDREN
                        }),
                        Some("parent agent sleeping") => {
                            json!({"dispatched":false,"reason":"parent agent sleeping"})
                        }
                        Some("subagents cannot dispatch") => {
                            json!({"dispatched":false,"reason":"subagents cannot dispatch"})
                        }
                        _ => {
                            return Err(ToolError::Exec(
                                "unknown confirmed dispatch refusal".into(),
                            ))
                        }
                    };
                    if output != &expected
                        || !input["task"]
                            .as_str()
                            .is_some_and(|task| !task.trim().is_empty())
                    {
                        return Err(ToolError::Exec(
                            "confirmed undispatched child receipt invalid".into(),
                        ));
                    }
                    continue;
                }
                let id = crate::tools::str_arg(output, "task_id")?;
                let title = input["title"]
                    .as_str()
                    .or(input["task"].as_str())
                    .ok_or_else(|| ToolError::Exec("confirmed child missing task".into()))?;
                let status = match output["status"].as_str() {
                    Some("done") => "done",
                    Some("failed") => "failed",
                    Some("stopped") => "stopped",
                    // Dispatch success proves acceptance, not final child completion.
                    Some("running" | "interrupted") => "interrupted",
                    _ => return Err(ToolError::Exec("confirmed child status invalid".into())),
                };
                if !restored.iter().any(|t| t.id == id) {
                    restored.push(Task {
                        key: String::new(),
                        id: id.into(),
                        title: title.into(),
                        kind: TaskKind::Subagent,
                        status,
                        result: output.get("result").cloned(),
                        error: if status == "interrupted" {
                            Some("child outcome unknown after restart; not redispatched".into())
                        } else {
                            output["error"].as_str().map(Into::into)
                        },
                        halt: Arc::new(AtomicBool::new(false)),
                        handle: None,
                        worker_pending: false,
                        retired: false,
                    });
                }
                continue;
            }
            if input.get("host_tool").is_some_and(|v| v != "tasks") {
                return Err(ToolError::Exec("invalid confirmed task tool".into()));
            }
            match input["action"].as_str() {
                Some("create") => {
                    let id = crate::tools::str_arg(output, "task_id")?;
                    let title = crate::tools::str_arg(input, "title")?;
                    if !restored.iter().any(|t| t.id == id) {
                        restored.push(Task {
                            key: String::new(),
                            id: id.into(),
                            title: title.into(),
                            kind: TaskKind::Manual,
                            status: "open",
                            result: None,
                            error: None,
                            halt: Arc::new(AtomicBool::new(false)),
                            handle: None,
                            worker_pending: false,
                            retired: false,
                        });
                    }
                }
                Some(op @ ("close" | "stop"))
                    if output[if op == "close" { "closed" } else { "stopped" }] == true =>
                {
                    let id = crate::tools::str_arg(input, "task_id")?;
                    let Some(task) = restored.iter_mut().find(|t| t.id == id) else {
                        return Err(ToolError::Exec(
                            "confirmed task operation refers to an unrestored child".into(),
                        ));
                    };
                    if op == "close" && task.kind != TaskKind::Manual {
                        return Err(ToolError::Exec(
                            "confirmed close cannot complete a child".into(),
                        ));
                    }
                    if task.status == "open"
                        || op == "stop" && matches!(task.status, "interrupted" | "failed")
                    {
                        task.status = if op == "close" { "done" } else { "stopped" };
                    }
                }
                Some("list") => {
                    // I3 Spec review 2026-10-06: a parent's successful list is a
                    // durable receipt, not merely UI output. Ignoring it lost
                    // done/result/citations after restart. Only a registered
                    // child can gain a terminal fact; in_flight never proves
                    // completion or historical OS cleanup. A false negative
                    // costs an investigation; a false positive hides lost work.
                    let entries = output["tasks"].as_array().ok_or_else(|| {
                        ToolError::Exec("confirmed task list missing entries".into())
                    })?;
                    for entry in entries {
                        if entry["kind"] != "subagent" {
                            continue;
                        }
                        let status = match entry["status"].as_str() {
                            Some("done") => "done",
                            Some("failed") => "failed",
                            Some("stopped") => "stopped",
                            _ => continue,
                        };
                        let Some(task) = restored.iter_mut().find(|task| {
                            task.kind == TaskKind::Subagent
                                && entry["id"].as_str() == Some(task.id.as_str())
                                && task.status == "interrupted"
                        }) else {
                            continue;
                        };
                        task.status = status;
                        task.result = entry.get("result").cloned();
                        task.error = entry["error"].as_str().map(Into::into);
                    }
                }
                Some("close" | "stop") => {}
                _ => return Err(ToolError::Exec("invalid confirmed task operation".into())),
            }
        }
        Ok(restored)
    }

    pub(crate) fn validate_operations(operations: &[(Value, Value)]) -> Result<(), ToolError> {
        Self::restored_tasks(operations).map(|_| ())
    }

    /// Restore the validated host root chain, never history prose or tool calls.
    pub(crate) fn restore_operations(
        &self,
        key: &str,
        operations: &[(Value, Value)],
    ) -> Result<(), ToolError> {
        let started = std::time::Instant::now();
        let restored = Self::restored_tasks(operations)?;
        let confirmed_not_dispatched = operations.iter().any(|(input, output)| {
            input["host_tool"] == "subagent" && output["dispatched"] == false
        });
        let mut confirmed_terminal = false;
        let mut max_seq = 0;
        let mut tasks = self.inner.lock().unwrap();
        for mut task in restored {
            if let Some(seq) = task
                .id
                .strip_prefix("task-")
                .and_then(|n| n.parse::<u64>().ok())
            {
                max_seq = max_seq.max(seq);
            }
            if let Some(current) = tasks.iter_mut().find(|t| t.key == key && t.id == task.id) {
                // I3 Spec: repeated recovery can extend the confirmed chain.
                // Fill an unknown restored child, never replace a live worker,
                // manual entry, or already published terminal receipt.
                if current.kind == TaskKind::Subagent
                    && current.status == "interrupted"
                    && current.handle.is_none()
                    && !current.worker_pending
                    && task.kind == TaskKind::Subagent
                    && matches!(task.status, "done" | "failed" | "stopped")
                {
                    current.status = task.status;
                    current.result = task.result;
                    current.error = task.error;
                    confirmed_terminal = true;
                }
                continue;
            }
            confirmed_terminal |= task.kind == TaskKind::Subagent
                && matches!(task.status, "done" | "failed" | "stopped");
            task.key = key.into();
            tasks.push(task);
        }
        let mut seq = self.seq.lock().unwrap();
        *seq = (*seq).max(max_seq);
        drop(seq);
        drop(tasks);
        if confirmed_terminal || confirmed_not_dispatched {
            let identity = serde_json::from_str::<Value>(key).ok();
            for code in [
                confirmed_terminal.then_some("confirmed_terminal"),
                confirmed_not_dispatched.then_some("confirmed_not_dispatched"),
            ]
            .into_iter()
            .flatten()
            {
                crate::diag::note(
                    crate::diag::CLASS_HOST,
                    false,
                    identity.as_ref().and_then(|value| value[0].as_str()),
                    identity.as_ref().and_then(|value| value[1].as_str()),
                    Some(key),
                    None,
                    "task_receipt_restore",
                    code,
                    started,
                );
            }
        }
        Ok(())
    }

    /// 手建条目（父代理的「待办」标记）。返回任务 id。
    pub fn create_manual(&self, key: &str, title: &str) -> String {
        let id = self.next_id();
        self.inner.lock().unwrap().push(Task {
            key: key.into(),
            id: id.clone(),
            title: title.into(),
            kind: TaskKind::Manual,
            status: "open",
            result: None,
            error: None,
            halt: Arc::new(AtomicBool::new(false)),
            handle: None,
            worker_pending: false,
            retired: false,
        });
        id
    }

    /// 子代理派遣登记（容量闸内聚在锁里：查数与落位同一临界区，
    /// 并发派遣不会越过上限）。满 → None。
    pub fn dispatch(&self, key: &str, title: &str) -> Option<Dispatched> {
        self.reap_finished();
        let mut tasks = self.inner.lock().unwrap();
        let running = tasks
            .iter()
            .filter(|t| same_parent(&t.key, key) && Self::in_flight(t))
            .count();
        if running >= MAX_CHILDREN {
            return None;
        }
        let id = self.next_id();
        let halt = Arc::new(AtomicBool::new(false));
        let answer: Arc<Mutex<Option<String>>> = Default::default();
        let reads: Arc<Mutex<Vec<String>>> = Default::default();
        tasks.push(Task {
            key: key.into(),
            id: id.clone(),
            title: title.into(),
            kind: TaskKind::Subagent,
            status: "running",
            result: None,
            error: None,
            halt: halt.clone(),
            handle: None,
            worker_pending: false,
            retired: false,
        });
        Some(Dispatched {
            id,
            halt,
            answer,
            reads,
        })
    }

    pub fn attach(&self, key: &str, id: &str, handle: std::thread::JoinHandle<()>) {
        let mut tasks = self.inner.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.key == key && t.id == id) {
            t.worker_pending = true;
            t.handle = Some(handle);
        }
    }

    fn mark_worker_pending(&self, key: &str, id: &str) {
        if let Some(t) = self
            .inner
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.key == key && t.id == id)
        {
            t.worker_pending = true;
        }
    }

    /// 收尾：线程与内联路径同一出口。
    pub fn finish(
        &self,
        key: &str,
        id: &str,
        status: &'static str,
        result: Option<Value>,
        error: Option<String>,
    ) {
        let mut tasks = self.inner.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.key == key && t.id == id) {
            t.status = status;
            t.result = result;
            t.error = error;
        }
    }

    /// 停掉一条还在跑的：置 halt——嵌套回合在 delta 缝/轮顶看到即收。
    /// 手动条目没有运行体，直接标 stopped。
    pub fn stop(&self, key: &str, id: &str) -> bool {
        let mut tasks = self.inner.lock().unwrap();
        let Some(t) = tasks.iter_mut().find(|t| t.key == key && t.id == id) else {
            return false;
        };
        t.halt.store(true, Ordering::Relaxed);
        if t.kind == TaskKind::Manual && t.status == "open"
            || t.kind == TaskKind::Subagent
                && !Self::in_flight(t)
                && matches!(t.status, "failed" | "interrupted")
        {
            t.status = "stopped";
        }
        true
    }

    /// 手动条目收口（父代理办完自己勾掉）。
    pub fn close(&self, key: &str, id: &str) -> bool {
        let mut tasks = self.inner.lock().unwrap();
        let Some(t) = tasks.iter_mut().find(|t| t.key == key && t.id == id) else {
            return false;
        };
        if t.kind == TaskKind::Manual && t.status == "open" {
            t.status = "done";
            return true;
        }
        false
    }

    /// 该激活的任务视图（父代理的工具结果；测试读回同一份）。
    pub fn list(&self, key: &str) -> Vec<Value> {
        self.reap_finished();
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.key == key)
            .map(|t| {
                json!({
                    "id": t.id,
                    "title": t.title,
                    "kind": if t.kind == TaskKind::Subagent { "subagent" } else { "manual" },
                    "status": t.status,
                    "in_flight": Self::in_flight(t),
                    "result": t.result,
                    "error": t.error,
                })
            })
            .collect()
    }

    pub fn running_subagents(&self, key: &str) -> usize {
        self.reap_finished();
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|t| same_parent(&t.key, key) && Self::in_flight(t))
            .count()
    }

    /// 任一激活还有子代理没交还（失速监视用：子代理未交还不算失速，ADR 0074）。
    /// 派遣线程可以比父回合活得久，父回合收口不代表子代理已交还。
    pub fn any_running_subagent(&self) -> bool {
        self.reap_finished();
        self.inner.lock().unwrap().iter().any(Self::in_flight)
    }

    /// Supplemental completion cannot hide a parent's open todo or unfinished child.
    pub(crate) fn any_unfinished(&self) -> bool {
        self.reap_finished();
        self.inner.lock().unwrap().iter().any(|task| {
            Self::in_flight(task)
                || !task.retired
                    && matches!(task.status, "open" | "running" | "failed" | "interrupted")
        })
    }

    /// 测试接缝：等所有在跑的派遣线程收尾（join 而不是轮询）。
    #[cfg(test)]
    pub fn join_pending(&self) {
        loop {
            let handle = {
                let mut tasks = self.inner.lock().unwrap();
                tasks
                    .iter_mut()
                    .find_map(|t| t.handle.take().map(|h| (t.key.clone(), t.id.clone(), h)))
            };
            match handle {
                Some((key, id, h)) => {
                    let failed = h.join().is_err();
                    if let Some(task) = self
                        .inner
                        .lock()
                        .unwrap()
                        .iter_mut()
                        .find(|t| t.key == key && t.id == id)
                    {
                        task.worker_pending = false;
                    }
                    if failed {
                        self.finish(
                            &key,
                            &id,
                            "failed",
                            None,
                            Some("child worker panicked".into()),
                        );
                    }
                }
                None => {
                    self.reap_finished();
                    return;
                }
            }
        }
    }

    /// Workbench Drop / 项目关闭：全部 halt（线程各自看到旗子自然收；
    /// 不 join——Drop 路径不阻塞）。
    pub fn halt_all(&self) {
        for t in self.inner.lock().unwrap().iter() {
            t.halt.store(true, Ordering::Relaxed);
        }
    }

    fn next_id(&self) -> String {
        let mut s = self.seq.lock().unwrap();
        *s += 1;
        format!("task-{}", *s)
    }
}

// ---------- 派遣（turn 层截获 `subagent` 工具调用）----------

/// 勾选的 mcp 工具是否可带：必须形如 mcp:<svc>:<tool>、在父注册表在场、
/// 且父代理对该服务有 grants 授权（与权限层 L0 同一查询——勾选只是
/// 「这次放行已授权的子集」，绝不扩权）。
fn selectable(
    db: &Db,
    ctx: &ToolContext,
    registry: &Registry,
    name: &str,
) -> Result<bool, ToolError> {
    let Some(service) = name
        .strip_prefix("mcp:")
        .and_then(|s| s.split(':').next())
        .filter(|s| !s.is_empty())
    else {
        return Ok(false);
    };
    if registry.get(name).is_none() {
        return Ok(false);
    }
    let granted: bool = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM grants g JOIN agents a ON a.id = g.agent_id
             WHERE a.project_id=?1 AND g.agent_id=?2 AND g.kind='mcp' AND g.name=?3",
            rusqlite::params![ctx.project_id, ctx.agent_id, service],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    Ok(granted)
}

/// 一次子代理回合的结尾（线程/内联共用）：outcome → 任务状态 + 回执。
struct ChildDone {
    status: &'static str,
    result: Option<Value>,
    error: Option<String>,
}

fn finish_child(outcome: Result<TurnOutcome, crate::turn::TurnError>, scope: &Scope) -> ChildDone {
    let answer = scope.answer.lock().unwrap().take().unwrap_or_default();
    let reads: Vec<String> = {
        let mut r = scope.reads.lock().unwrap().clone();
        r.sort();
        r.dedup();
        r
    };
    let citations: Vec<Value> = reads.into_iter().map(|p| json!({"path": p})).collect();
    let clip = |s: String| -> String { s.chars().take(ANSWER_CAP).collect() };
    match outcome {
        // I3 / ticket 04: a step budget used to masquerade as completed work.
        // Finished is a child receipt, never the parent's acceptance verdict.
        Ok(TurnOutcome::Finished) => ChildDone {
            status: "done",
            result: Some(json!({"answer": clip(answer), "citations": citations})),
            error: None,
        },
        Ok(TurnOutcome::Truncated) => ChildDone {
            status: "failed",
            result: None,
            error: Some("child execution budget exhausted; task incomplete".into()),
        },
        // 叫停/暂停/挂起都是「没收尾」——halt 旗置位的是主动停止。
        Ok(TurnOutcome::Interrupted) => ChildDone {
            status: if scope.halt.load(Ordering::Relaxed) {
                "stopped"
            } else {
                "failed"
            },
            result: None,
            error: Some("interrupted".into()),
        },
        Ok(TurnOutcome::Suspended) => ChildDone {
            status: "failed",
            result: None,
            error: Some("network suspended mid-run".into()),
        },
        Ok(TurnOutcome::SkippedSleeping) => ChildDone {
            status: "failed",
            result: None,
            error: Some("parent agent sleeping".into()),
        },
        Ok(TurnOutcome::SkippedCap) => ChildDone {
            status: "failed",
            result: None,
            error: Some("usage cap reached".into()),
        },
        Ok(TurnOutcome::AwaitingPermission(_)) => ChildDone {
            status: "failed",
            result: None,
            // 结构性不该发生（子代理注册表里没有必问类工具）——发生即 bug，
            // 诚实上报而不是假装成功。
            error: Some("subagent hit an ask-path tool — structural bug".into()),
        },
        Ok(TurnOutcome::Failed(e)) => ChildDone {
            status: "failed",
            result: None,
            error: Some(e),
        },
        Err(e) => ChildDone {
            status: "failed",
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn run_child(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    nctx: &ToolContext,
    task: &str,
) -> ChildDone {
    let outcome = run_turn_streaming(
        db,
        provider,
        registry,
        nctx,
        // 行为纪律在工作台基础层的子代理版里（prompt-engineering 票 07），
        // 回合内核按 ctx.subagent 注入；这里不再另挂角色层。
        vec![],
        task,
        &[],
        false,
        // 子代理没有自己的时间线气泡——delta 全弃（答案走回执格）。
        None,
    );
    let scope = nctx.subagent.clone().expect("child ctx carries scope");
    finish_child(outcome, &scope)
}

/// turn 层截获入口（票 04）：`subagent` 工具调用不往权限管线走——派遣
/// 语义的输入校验/容量闸/线程分派都在这里，工具表上的同名项只提供
/// 模型可见的 def（exec 走到即未接截获的 bug）。
pub fn call_nested(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    input: Value,
) -> Result<CallOutcome, ToolError> {
    call_nested_with_seq(db, provider, registry, ctx, input, None)
}

pub(crate) fn call_nested_with_seq(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    input: Value,
    seq: Option<&str>,
) -> Result<CallOutcome, ToolError> {
    let task = input["task"].as_str().unwrap_or("").trim().to_string();
    let title = input["title"].as_str().unwrap_or("").trim().to_string();
    registry.tracked_nested(db, ctx, &input, seq, || {
        dispatch(db, provider, registry, ctx, &input, task, title).map(CallOutcome::Done)
    })
}

fn dispatch(
    db: &Db,
    provider: &dyn ModelProvider,
    registry: &Registry,
    ctx: &ToolContext,
    input: &Value,
    task: String,
    title: String,
) -> Result<Value, ToolError> {
    let evaluation_work = crate::evaluation::control::work_lease(&ctx.repo_root)
        .map_err(|e| ToolError::NotExecuted(e.to_string()))?;
    if task.is_empty() {
        return Err(ToolError::BadInput("subagent needs a task".into()));
    }
    // 保险闸：subagent_scope 注册表里没有 subagent 工具，这层是「万一
    // 被手动注册进去」的兜底——子代理不许再派生（结构性一层深）。
    if ctx.subagent.is_some() {
        return Ok(json!({"dispatched": false, "reason": "subagents cannot dispatch"}));
    }
    // 父休眠子代理不跑（前置闸；嵌套回合起跑与轮顶还有两道）。
    let status: String = db
        .conn()
        .query_row(
            "SELECT status FROM agents WHERE id=?1",
            [&ctx.agent_id],
            |r| r.get(0),
        )
        .map_err(ToolError::Sqlite)?;
    if status == "sleeping" {
        return Ok(json!({"dispatched": false, "reason": "parent agent sleeping"}));
    }

    let key = activation_key(ctx);
    let title = if title.is_empty() {
        task.chars().take(60).collect()
    } else {
        title
    };
    let Some(disp) = ctx.tasks.dispatch(&key, &title) else {
        // 容量满立即拒（不排队）：父代理应等既有的收尾再派。
        return Ok(json!({
            "dispatched": false,
            "reason": "capacity full",
            "running": MAX_CHILDREN,
        }));
    };
    let id = disp.id;

    // reliability 05: reserve the existing bounded dispatch slot first. A fifth
    // dispatch must not start isolation work before being refused.
    let mut pick: HashSet<String> = HashSet::new();
    let mut dropped: Vec<String> = Vec::new();
    let mut reasons = std::collections::BTreeMap::new();
    let mut isolated = Vec::new();
    if let Some(list) = input["mcp_tools"].as_array() {
        for value in list {
            let Some(name) = value.as_str() else {
                dropped.push(value.to_string());
                continue;
            };
            if pick.contains(name) || reasons.contains_key(name) {
                continue;
            }
            let started = std::time::Instant::now();
            let candidate = if selectable(db, ctx, registry, name)? {
                registry
                    .get(name)
                    .ok_or_else(|| ToolError::Exec("tool unavailable".into()))
                    .and_then(|tool| tool.for_subagent(ctx))
            } else {
                Err(ToolError::Exec(
                    "parent authorization or tool unavailable".into(),
                ))
            };
            let accepted = candidate.is_ok();
            crate::diag::note(
                if accepted {
                    crate::diag::CLASS_JUDGE
                } else {
                    crate::diag::CLASS_REJECT
                },
                !accepted,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "subagent_mcp",
                if accepted {
                    "isolated_read_only"
                } else {
                    "capability_unavailable"
                },
                started,
            );
            match candidate {
                Ok(tool) => {
                    pick.insert(name.to_string());
                    isolated.push(tool);
                }
                Err(error) => {
                    dropped.push(name.to_string());
                    reasons.insert(name.to_string(), error.to_string());
                }
            }
        }
    }

    let scope = Scope {
        approval_mode: crate::approval_mode::read(db, &ctx.project_id)?.mode,
        permission_rules: Arc::new(
            crate::permissions::list_rules(db, &ctx.project_id)?
                .into_iter()
                .map(|rule| rule.id)
                .collect(),
        ),
        halt: disp.halt,
        answer: disp.answer,
        mcp: Arc::new(pick.clone()),
        reads: disp.reads,
    };
    let mut nctx = ctx.clone();
    nctx.subagent = Some(scope);
    let sub_registry = registry.subagent_scope(&isolated);

    // D09: isolating selected MCP tools can wait across a stop. Release the
    // reserved dispatch slot instead of spawning a child after that wait.
    if let Err(error) = crate::evaluation::control::checkpoint(&ctx.repo_root) {
        ctx.tasks.finish(
            &key,
            &id,
            "interrupted",
            None,
            Some("evaluation stopped before child dispatch".into()),
        );
        return Err(ToolError::NotExecuted(error.to_string()));
    }

    // 线程派遣需要两件套：可移动的第二连接（内存库没有文件路径，退化为
    // 内联同步——测试接缝的诚实降级）与可移动的 provider Arc。
    match (db.path(), ctx.subagent_provider.clone()) {
        (Some(path), Some(prov)) => {
            let board = ctx.tasks.clone();
            let id2 = id.clone();
            let key2 = key.clone();
            ctx.tasks.mark_worker_pending(&key, &id);
            let handle = std::thread::spawn(move || {
                let _evaluation_work = evaluation_work;
                let done = match Db::open(&path) {
                    Ok(db2) => run_child(&db2, prov.as_ref(), &sub_registry, &nctx, &task),
                    Err(e) => ChildDone {
                        status: "failed",
                        result: None,
                        error: Some(format!("db open: {e}")),
                    },
                };
                board.finish(&key2, &id2, done.status, done.result, done.error);
            });
            ctx.tasks.attach(&key, &id, handle);
            Ok(json!({
                "dispatched": true, "task_id": id, "status": "running",
                "mcp_selected": pick.iter().collect::<Vec<_>>(),
                "mcp_dropped": dropped, "mcp_drop_reasons": reasons,
                "note": "collect the result via tasks list",
            }))
        }
        _ => {
            // 内联路径（内存库/无 Arc provider 的直测）：同一块板、同一收尾，
            // 只是派遣调用同步返回带结果——测试不需要跨连接可见性。
            let done = run_child(db, provider, &sub_registry, &nctx, &task);
            ctx.tasks.finish(
                &key,
                &id,
                done.status,
                done.result.clone(),
                done.error.clone(),
            );
            Ok(json!({
                "dispatched": true, "task_id": id, "status": done.status,
                "mcp_selected": pick.iter().collect::<Vec<_>>(),
                "mcp_dropped": dropped, "mcp_drop_reasons": reasons,
                "result": done.result, "error": done.error,
            }))
        }
    }
}

// ---------- 工具 ----------

/// `subagent` 工具 def（父代理注册表）：模型可见的派遣入口。
/// exec 永远不该走到（turn 层截获）；走到 = 截获漏接的 bug。
pub struct Subagent;
impl crate::tools::Tool for Subagent {
    fn name(&self) -> &str {
        "subagent"
    }
    fn description(&self) -> &str {
        r#"Dispatch a bounded subagent inside this activation. It can find, read, search and compare repo content, search the web for snippets and run test commands, then hands back a concise conclusion citing the paths it read.
- Use when: an investigation needs many searches or reads whose raw output you do not need in your own context, or several independent questions can run in parallel.
- Do not use: for a single directed lookup (call fs_find, fs_grep or fs_read yourself), or for anything that writes files, runs git or publishes — subagents cannot.
- `task` must be self-contained: the subagent has not seen this conversation. State the goal, what you already know or ruled out, and the answer you need back.
- Errors: at most 4 run at once; a 5th dispatch is rejected — wait for one to finish. Results land on the task list: collect them with tasks (action "list"). Never guess a result before it arrives."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "task":{"type":"string","description":"bounded instruction for the child"},
            "title":{"type":"string","description":"short task-list label (defaults to task prefix)"},
            "mcp_tools":{"type":"array","items":{"type":"string"},
                "description":"mcp:<service>:<tool> names already authorized for you; only separately host-confined local read-only capabilities can be delegated. Unsupported selections return mcp_drop_reasons."}},
            "required":["task"]})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        // 诚实声明：派遣会拉起命令执行（run_test）。截获路径不过权限层；
        // 若哪天被改走管线，Exec 地板让它必问而不是静默放行。
        crate::tools::RiskClass::Exec
    }
    fn exec(&self, _db: &Db, _input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        Err(ToolError::Exec(
            "subagent is intercepted by the turn layer".into(),
        ))
    }
}

/// `tasks` 工具（票 07）：激活任务清单的读/建/停/勾掉。
/// Read 类：操作对象全是本激活内存条目，无持久副作用。
pub struct Tasks;
impl crate::tools::Tool for Tasks {
    fn name(&self) -> &str {
        "tasks"
    }
    fn description(&self) -> &str {
        r#"The activation task list: your own to-do items plus subagent dispatches and their results.
- Use when: the work has 3 or more steps (create items, close each as you finish it), or to collect subagent results.
- Do not use: for a single trivial step.
- `action`: "list" shows every task with its status and result; "create" adds an item (`title`); "close" marks an item done (`task_id`); "stop" halts a running task (`task_id`). Same-root resumes preserve the list. A new task starts a separate list; `in_flight` remains true until a stopped child actually drains.
- Errors: an unknown action or task_id is reported."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "action":{"type":"string","enum":["list","create","close","stop"],"description":"what to do with the list"},
            "title":{"type":"string","description":"for create"},
            "task_id":{"type":"string","description":"for close/stop"}},
            "required":["action"]})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Read
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let key = activation_key(ctx);
        match input["action"].as_str().unwrap_or("") {
            "list" => Ok(json!({"tasks": ctx.tasks.list(&key)})),
            "create" => {
                let title = crate::tools::str_arg(input, "title")?;
                Ok(json!({"task_id": ctx.tasks.create_manual(&key, title)}))
            }
            "close" => {
                let id = crate::tools::str_arg(input, "task_id")?;
                Ok(json!({"closed": ctx.tasks.close(&key, id)}))
            }
            "stop" => {
                let id = crate::tools::str_arg(input, "task_id")?;
                Ok(json!({"stopped": ctx.tasks.stop(&key, id)}))
            }
            other => Err(ToolError::BadInput(format!(
                "unknown tasks action: {other}"
            ))),
        }
    }
}

/// `sem_search` 工具（票 02）：语义搜索共用主/子注册表——工具同一件，
/// 只是子代理那边的是它可用清单的一员。每次调用先增量刷新索引
/// （改动才重嵌），默认引擎保留字面命中，再按全块内积取 top-k。
pub struct SemSearch;
impl crate::tools::Tool for SemSearch {
    fn name(&self) -> &str {
        "sem_search"
    }
    fn description(&self) -> &str {
        r#"Optional local similarity search; prefer scoped fs_find/fs_grep followed by fs_read for repository exploration. Character n-gram similarity only; no model weights. No network or paid calls.
- Use when: you have approximate source text to match. For Chinese questions about English source, derive candidate English terms and use fs_find/fs_grep/fs_read; this tool does not translate or understand synonyms.
- Do not use: when you know the literal text, identifier or filename — prefer fs_grep or fs_find.
- Returns file path, line number, excerpt and match kind (literal or similarity). With built-in engines, case-sensitive literal matches in indexed files come first and point to the matching line. Scores are chunk similarity, not relevance confidence or the sole sort key. Verify each excerpt; for weak or empty hits fall back to fs_grep or fs_find. Unchanged files reuse persisted vectors. Inspect coverage: capped discovery, skipped large/binary/policy files and changed sources make results incomplete; empty hits never prove repository-wide absence. Stops, indexing read failures and broken ignore rules are errors; query read failures are counted in coverage.skipped_unreadable."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string","description":"natural-language question"},
            "count":{"type":"integer","description":"max hits (default 8, cap 10)"}},
            "required":["query"]})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let query = crate::tools::str_arg(input, "query")?.trim();
        if query.is_empty() {
            return Err(ToolError::BadInput("empty query".into()));
        }
        let cap = input["count"].as_u64().unwrap_or(8).clamp(1, 10) as usize;
        let started = std::time::Instant::now();
        // A15 review: bounded, resumable cold indexing. Completed files are
        // reusable after cancellation; check stop between files and chunks.
        let deadline = ctx
            .deadline
            .map_or(started + std::time::Duration::from_secs(900), |d| {
                d.min(started + std::time::Duration::from_secs(900))
            });
        let check = || {
            if std::time::Instant::now() >= deadline
                || ctx
                    .subagent
                    .as_ref()
                    .is_some_and(|scope| scope.halt.load(std::sync::atomic::Ordering::Relaxed))
            {
                return Err(ToolError::Exec(
                    "local retrieval stopped or deadline exceeded; retry resumes indexing".into(),
                ));
            }
            crate::evaluation::control::checkpoint(&ctx.repo_root)
                .map_err(|e| ToolError::Exec(e.to_string()))
        };
        let result = (|| {
            check()?;
            let embedder = match &ctx.embedder {
                Some(embedder) => embedder.clone(),
                None => crate::semsearch::default_embedder(&check)?,
            };
            crate::diag::note(
                crate::diag::CLASS_HOST,
                false,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "retrieval_engine",
                embedder.name(),
                started,
            );
            let mut refreshed =
                crate::semsearch::refresh(db, &ctx.repo_root, embedder.as_ref(), &check)?;
            let queried = crate::semsearch::query(
                db,
                &ctx.repo_root,
                embedder.as_ref(),
                query,
                cap,
                &check,
                &refreshed.paths,
            )?;
            refreshed.coverage.skipped_unreadable += queried.skipped_unreadable;
            refreshed
                .coverage
                .finish(queried.skipped_changed, queried.truncated);
            let hits = queried.hits;
            Ok(json!({
                "count": hits.len(),
                "indexed_files": refreshed.indexed_files,
                "coverage": refreshed.coverage,
                "engine": embedder.name(),
                "hits": hits,
                "fallback": hits.is_empty(),
                "note": if hits.is_empty() { "no similar text — try fs_grep with a literal term" }
                    else if embedder.name() == "hash-ngram-v1" { "character similarity only; no cross-language semantic understanding" } else { "" },
            }))
        })();
        if result.is_err() {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "retrieval_refused",
                "index_or_inference_failed",
                started,
            );
        }
        result
    }
}

// ---------- run_test 的机械命令闸（票 05）----------
//
// 出处与代价模型：子代理跑命令是「父代理不在场时的一次执行」——提示词
// 里写「只跑测试」是行为引导不是闸（spec 明言）。闸必须是机械的：
// 每段命令经词法拆分后逐项过——程序名白名单（测试运行器）、子命令
// 白名单、快照更新标志黑名单、path 形态操作数不得越出仓根。
// fail-closed：认不出的形态一律拒——误拒的代价是子代理回报「跑不了」，
// 放行的代价是一次未审副作用。

/// 子命令白名单：程序 → 允许的子命令形态。
/// 返回 None = 程序本身不在测试运行器名单（连词法都省得看）。
fn test_program_shape(prog: &str, argv: &[String]) -> Option<Result<(), &'static str>> {
    let sub = |i: usize| argv.get(i).map(|s| s.as_str());
    match prog {
        "cargo" => match sub(1) {
            Some("test") | Some("nextest") => Some(Ok(())),
            _ => Some(Err(
                "cargo: only `cargo test`/`cargo nextest` are test commands",
            )),
        },
        "npm" => match sub(1) {
            Some("test") | Some("t") => Some(Ok(())),
            Some("run") if sub(2).is_some_and(|s| s.starts_with("test")) => Some(Ok(())),
            _ => Some(Err("npm: only test/test:*/run test* shapes allowed")),
        },
        "pnpm" => match sub(1) {
            Some("test") | Some("vitest") | Some("jest") => Some(Ok(())),
            Some("run") if sub(2).is_some_and(|s| s.starts_with("test")) => Some(Ok(())),
            _ => Some(Err("pnpm: only test/vitest/jest/run test* shapes allowed")),
        },
        "yarn" => match sub(1) {
            Some("test") | Some("jest") | Some("vitest") => Some(Ok(())),
            _ => Some(Err("yarn: only test/jest/vitest allowed")),
        },
        "python" | "python3" => match (sub(1), sub(2)) {
            (Some("-m"), Some("pytest")) => Some(Ok(())),
            _ => Some(Err("python: only `-m pytest` allowed")),
        },
        "go" => match sub(1) {
            Some("test") => Some(Ok(())),
            _ => Some(Err("go: only `go test` allowed")),
        },
        "dotnet" => match sub(1) {
            Some("test") => Some(Ok(())),
            _ => Some(Err("dotnet: only `dotnet test` allowed")),
        },
        "mix" => match sub(1) {
            Some("test") => Some(Ok(())),
            _ => Some(Err("mix: only `mix test` allowed")),
        },
        "vitest" | "jest" | "mocha" | "pytest" | "rspec" | "ctest" => Some(Ok(())),
        _ => None,
    }
}

/// 快照/更新类标志：测试运行器里唯一会回写源文件族（快照）的口子。
const DENY_TEST_FLAGS: &[&str] = &["-u", "--update", "--update-snapshot", "--updatesnapshot"];

/// run_test 命令闸：Ok = 可跑；Err(reason) = 机械拒。
/// 提示词里写「这条是允许的」对本函数零影响——它只看命令文本。
pub fn test_cmd_gate(cmd: &str, ctx: &ToolContext) -> Result<(), String> {
    // reliability 04: runners need the host-generated output path in flags.
    // Only this exact variable is permitted; unrelated variables/substitution
    // remain opaque. This substitution is for validation, not shell execution.
    let normalized = cmd.replace("${HEXAGON_TEST_OUTPUT}", ".hexagon-test-output");
    let mut parts = normalized.split("$HEXAGON_TEST_OUTPUT");
    let mut checked = parts.next().unwrap_or_default().to_string();
    for part in parts {
        let longer_name = part
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        checked.push_str(if longer_name {
            "$HEXAGON_TEST_OUTPUT"
        } else {
            ".hexagon-test-output"
        });
        checked.push_str(part);
    }
    let cmd = checked.as_str();
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Err("empty command".into());
    }
    // 不透明构造一律拒：重定向（写向哪都担保不了）、命令/变量替换、
    // 子 shell——与权限层的 CMD_OPAQUE 同口径，fail closed。
    for t in ["`", "$(", "$", ">", "<", "("] {
        if cmd.contains(t) {
            return Err(format!(
                "opaque shell construct {t:?} is not a test command"
            ));
        }
    }
    let parts = crate::permissions::split_commands(cmd);
    if parts.is_empty() {
        return Err("unlexable command".into());
    }
    for part in &parts {
        let Ok(argv) = crate::permissions::shell_words(part) else {
            return Err("unlexable segment".into());
        };
        let mut argv = argv.iter().map(|s| s.as_str()).collect::<Vec<_>>();
        // 剥前导 NAME=value 环境赋值（值同样过路径越界检查）。
        while argv.first().is_some_and(|t| {
            t.contains('=')
                && t.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        }) {
            let t = argv.remove(0);
            if let Some((_, v)) = t.split_once('=') {
                if crate::permissions::looks_like_operand_path(v)
                    && crate::permissions::operand_escapes(v, ctx)
                {
                    return Err("env value escapes repo root".into());
                }
            }
        }
        let Some(prog) = argv.first() else {
            return Err("empty segment".into());
        };
        let prog = prog
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(prog)
            .trim_end_matches(".exe")
            .to_lowercase();
        let argv_s: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        match test_program_shape(&prog, &argv_s) {
            None => return Err(format!("{prog}: not a test runner")),
            Some(Err(r)) => return Err(format!("{prog}: {r}")),
            Some(Ok(())) => {}
        }
        for a in &argv[1..] {
            if DENY_TEST_FLAGS.contains(&a.to_lowercase().as_str()) {
                return Err(format!(
                    "{a}: snapshot/self-update flags write source files"
                ));
            }
            // `--flag=/path` 与裸 path 操作数同口径查越界。
            for cand in [*a, a.split_once('=').map(|(_, v)| v).unwrap_or("")] {
                if crate::permissions::looks_like_operand_path(cand)
                    && crate::permissions::operand_escapes(cand, ctx)
                {
                    return Err(format!("{cand}: path escapes repo root"));
                }
            }
        }
    }
    Ok(())
}

// I3: a completed process or a zero exit code cannot prove output was read.
// Refusing an unknown result costs one investigation; accepting it can falsely
// certify a failing test. Keep the existing terminal unknown contract.
fn test_output_known(out: &Value) -> Result<(), ToolError> {
    if out["outcome_unknown"] == true || out.get("read_error").is_some_and(|e| !e.is_null()) {
        Err(ToolError::Exec(
            "test output read failed; outcome unknown".into(),
        ))
    } else {
        Ok(())
    }
}

/// `run_test` 工具（票 05）：只在子代理注册表里。跑「明显是测试」的
/// 命令，回 exit_code + 尾部摘录——完整日志不落回执（任务清单不是
/// 日志通道；要看全文父代理自己跑）。
pub struct RunTest;
impl crate::tools::Tool for RunTest {
    fn name(&self) -> &str {
        "run_test"
    }
    fn description(&self) -> &str {
        r#"Run one test command and get the verdict: pass/fail, exit code and a tail excerpt (full logs are not returned).
- Use when: you need to know whether tests pass (cargo test, npm test, vitest, pytest, go test, dotnet test, …).
- Do not use: for anything that is not a test — writes to source, specs or docs, git, remote publishing and pipe-installs are mechanically rejected.
- Outputs: only the fresh host-assigned HEXAGON_TEST_OUTPUT directory is writable. TMPDIR, CARGO_TARGET_DIR, XDG_CACHE_HOME, npm_config_cache and COVERAGE_FILE point there. Configure other runners to use that directory; source, snapshots and host state stay read-only. The result includes output_dir.
- Errors: a rejected command comes back with the reason; do not reword it to get past the check."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "cmd":{"type":"string","description":"a single test command"},
            "timeout_ms":{"type":"integer","description":"default 120000, cap 120000"}},
            "required":["cmd"],"additionalProperties":false})
    }
    fn risk(&self) -> crate::tools::RiskClass {
        // Exec 声明是诚实的（跑命令）；派遣域里权限层按 Scope 放行——
        // reliability 04: OS write scope enforces isolation. Command shape is
        // an intent filter, not a guarantee that a script cannot modify source.
        crate::tools::RiskClass::Exec
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let cmd = input["cmd"].as_str().unwrap_or("");
        crate::tools::bash_hits_credentials(cmd)
            .then(|| "command touches credential material".to_string())
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let started = std::time::Instant::now();
        let cmd = crate::tools::str_arg(input, "cmd")?;
        if let Err(reason) = test_cmd_gate(cmd, ctx) {
            // 拒绝记诊断（Warn——拒绝类）：哪个 agent 的哪次派遣被拒。
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "subagent_run_test",
                "gate_deny",
                started,
            );
            // Live acceptance 2026-10-01: this gate rejects before spawning.
            // Exec incorrectly created an unknown-effect card for an unexecuted command.
            return Err(ToolError::NotExecuted(format!("run_test denied: {reason}")));
        }
        let timeout = std::time::Duration::from_millis(
            input["timeout_ms"]
                .as_u64()
                .unwrap_or(RUN_TEST_TIMEOUT_MS)
                .clamp(1_000, RUN_TEST_TIMEOUT_MS),
        );
        // spawn_task + 轮询：halt 旗置位即杀进程组（stop 能停掉卡在长
        // 测试上的子代理，不是等 120s 超时）。
        let v = ctx.sessions.spawn_task(db, ctx, cmd, false)?;
        let task_id = v["task_id"].as_str().unwrap_or("").to_string();
        let deadline = std::time::Instant::now() + timeout;
        let mut killed = false;
        let mut last: Value = json!({});
        loop {
            let halt = ctx
                .subagent
                .as_ref()
                .is_some_and(|s| s.halt.load(Ordering::Relaxed));
            if halt {
                let _ = ctx.sessions.kill(db, ctx, Some(&task_id), None);
                killed = true;
            }
            let out = ctx
                .sessions
                .read_output(db, ctx, Some(&task_id), None, 0, 0)?;
            if let Err(error) = test_output_known(&out) {
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&ctx.project_id),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                    None,
                    "subagent_run_test",
                    "output_unknown",
                    started,
                );
                let _ = ctx.sessions.kill(db, ctx, Some(&task_id), None);
                return Err(error);
            }
            let done = out["done"].as_bool().unwrap_or(false);
            last = out;
            if done {
                break;
            }
            if std::time::Instant::now() > deadline {
                let _ = ctx.sessions.kill(db, ctx, Some(&task_id), None);
                killed = true;
            }
            if killed {
                // 杀完再等一拍收割退出态（kill→wait 窗口与 sessions::kill 同款）。
                std::thread::sleep(std::time::Duration::from_millis(50));
                let out = ctx
                    .sessions
                    .read_output(db, ctx, Some(&task_id), None, 0, 0)?;
                test_output_known(&out).inspect_err(|_| {
                    crate::diag::note(
                        crate::diag::CLASS_REJECT,
                        true,
                        Some(&ctx.project_id),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                        None,
                        "subagent_run_test",
                        "output_unknown_after_kill",
                        started,
                    );
                })?;
                last = out;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let exit = last["exit_code"].as_i64().unwrap_or(-1);
        let stdout = last["stdout"].as_str().unwrap_or("");
        let stderr = last["stderr"].as_str().unwrap_or("");
        // 尾部摘录（错误在末尾）：合流后取最后 ~3KB。
        let joined = format!("{stdout}{stderr}");
        let tail: String = joined
            .chars()
            .rev()
            .take(3_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        Ok(json!({
            "passed": !killed && exit == 0,
            "exit_code": exit,
            "killed": killed,
            "excerpt": tail,
            "output_dir": v["output_dir"],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ScriptedProvider;
    use crate::tools::Tool;
    use crate::turn::{text_response, tool_response};

    fn setup() -> (crate::api::Workbench, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let wb = crate::api::Workbench::for_test(dir.path(), &["研究"], None).unwrap();
        crate::orchestra::write_agent_status(&wb.db, "p1", "a0", false).unwrap();
        (wb, dir)
    }

    // Benchmark I3, 2026-10-06: stage ids identify shared workflow runs,
    // not parents or activations. A second parent must not erase another's todo.
    #[test]
    fn same_stage_parents_never_clear_each_other() {
        let (wb, _dir) = setup();
        let mut a = wb.ctx_for("a0", None);
        a.stage_run_id = Some("shared-stage".into());
        a.activation_root_turn_id = Some(101);
        let mut b = a.clone();
        b.agent_id = "a1".into();
        b.activation_root_turn_id = Some(102);
        let akey = activation_key(&a);
        let bkey = activation_key(&b);
        a.tasks.begin_activation(&akey);
        let id = a.tasks.create_manual(&akey, "A remaining acceptance");
        b.tasks.begin_activation(&bkey);
        assert_eq!(a.tasks.list(&akey).len(), 1);
        assert_eq!(a.tasks.list(&akey)[0]["id"], id);
        assert!(b.tasks.list(&bkey).is_empty());
    }

    #[test]
    fn repeated_resume_preserves_the_original_activation_todo() {
        let (wb, _dir) = setup();
        let mut ctx = wb.ctx_for("a0", None);
        ctx.activation_root_turn_id = Some(101);
        let key = activation_key(&ctx);
        ctx.tasks.begin_activation(&key);
        let id = ctx
            .tasks
            .create_manual(&key, "must retain original constraint");
        for parent in [101, 120, 130] {
            ctx.resume_from_turn_id = Some(parent);
            ctx.tasks.begin_activation(&activation_key(&ctx));
            assert_eq!(ctx.tasks.list(&key).len(), 1);
            assert_eq!(ctx.tasks.list(&key)[0]["id"], id);
        }
    }

    #[test]
    fn cancel_cannot_hide_an_undrained_child() {
        let board = TaskBoard::default();
        let key = "parent-activation";
        let dispatched = board.dispatch(key, "blocked child").unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let worker_board = board.clone();
        let id = dispatched.id.clone();
        let handle = std::thread::spawn(move || {
            let _ = wait.recv();
            worker_board.finish(key, &id, "stopped", None, None);
        });
        board.attach(key, &dispatched.id, handle);
        assert!(board.stop(key, &dispatched.id));
        board.begin_activation(key);
        let observed = board.any_running_subagent();
        release.send(()).unwrap();
        board.join_pending();
        assert!(
            observed,
            "a stop request is not evidence that a child has drained"
        );
    }

    #[test]
    fn completed_child_still_counts_until_worker_cleanup_finishes() {
        let board = TaskBoard::default();
        let dispatched = board.dispatch("key", "child").unwrap();
        let (completed, done) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let worker_board = board.clone();
        let id = dispatched.id.clone();
        let handle = std::thread::spawn(move || {
            worker_board.finish("key", &id, "done", Some(json!({"answer":"ready"})), None);
            completed.send(()).unwrap();
            let _ = wait.recv();
        });
        board.attach("key", &dispatched.id, handle);
        done.recv().unwrap();
        let observed = board.any_running_subagent();
        release.send(()).unwrap();
        board.join_pending();
        assert!(
            observed,
            "publishing a result must not hide in-flight cleanup"
        );
    }

    #[test]
    fn production_list_reaps_finished_workers_without_test_join() {
        let board = TaskBoard::default();
        let dispatched = board.dispatch("key", "child").unwrap();
        let worker_board = board.clone();
        let id = dispatched.id.clone();
        board.attach(
            "key",
            &dispatched.id,
            std::thread::spawn(move || {
                worker_board.finish("key", &id, "done", Some(json!({"answer":"done"})), None);
            }),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let finished = board.inner.lock().unwrap()[0]
                .handle
                .as_ref()
                .unwrap()
                .is_finished();
            if finished {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owned fixture worker failed to exit"
            );
            std::thread::yield_now();
        }
        assert_eq!(board.list("key")[0]["in_flight"], false);
        let tasks = board.inner.lock().unwrap();
        assert!(tasks[0].handle.is_none());
        assert!(!tasks[0].worker_pending);
    }

    #[test]
    fn confirmed_task_receipts_restore_without_reopening_or_id_collision() {
        let board = TaskBoard::default();
        let other = board.create_manual("other", "other parent");
        let ops = vec![
            (
                json!({"action":"create","title":"A"}),
                json!({"task_id":"task-1"}),
            ),
            (
                json!({"action":"close","task_id":"task-1"}),
                json!({"closed":true}),
            ),
            (
                json!({"action":"create","title":"B"}),
                json!({"task_id":"task-9"}),
            ),
        ];
        board.restore_operations("restored", &ops).unwrap();
        board.restore_operations("restored", &ops).unwrap();
        assert_eq!(board.list("restored").len(), 2);
        assert_eq!(board.list("restored")[0]["status"], "done");
        assert_eq!(board.list("other")[0]["id"], other);
        assert_eq!(board.list("other")[0]["status"], "open");
        assert_eq!(board.create_manual("restored", "C"), "task-10");
        let before = board.list("restored");
        let unknown = vec![(
            json!({"action":"stop","task_id":"unrestored-child"}),
            json!({"stopped":true}),
        )];
        assert!(board.restore_operations("restored", &unknown).is_err());
        assert_eq!(board.list("restored"), before);
    }

    #[test]
    fn confirmed_dispatch_restore_is_unknown_without_restarting_worker() {
        let board = TaskBoard::default();
        let ops = vec![(
            json!({"host_tool":"subagent","task":"read dependencies"}),
            json!({"task_id":"task-12","status":"running","dispatched":true}),
        )];
        TaskBoard::validate_operations(&ops).unwrap();
        board.restore_operations("root", &ops).unwrap();
        assert_eq!(board.list("root")[0]["kind"], "subagent");
        assert_eq!(board.list("root")[0]["status"], "interrupted");
        assert_eq!(board.list("root")[0]["in_flight"], false);
        assert!(board.any_unfinished());
        assert!(!board.any_running_subagent());
        assert!(board.stop("root", "task-12"));
        assert!(!board.any_unfinished());
    }

    proptest::proptest! {
        #[test]
        fn confirmed_list_preserves_terminal_receipts_without_creating_workers(
            terminal in 0usize..3, in_flight in proptest::prelude::any::<bool>(),
            answer in "[a-zA-Z0-9]{1,80}"
        ) {
            use proptest::prelude::*;
            let status=["done","failed","stopped"][terminal];
            let result=json!({"answer":answer,"citations":[{"path":"input.txt"}]});
            let board=TaskBoard::default();
            let operations=vec![
                (json!({"host_tool":"subagent","task":"read"}),json!({"task_id":"task-1","status":"running"})),
                (json!({"host_tool":"tasks","action":"list"}),json!({"tasks":[{"id":"task-1","kind":"subagent","status":status,"in_flight":in_flight,"result":result,"error":null}]})),
            ];
            // The same root can first restore an unobserved dispatch, then
            // receive a confirmed terminal list in the longer validated chain.
            board.restore_operations("key",&operations[..1]).unwrap();
            board.restore_operations("key",&operations).unwrap();
            board.restore_operations("key",&operations).unwrap();
            let mut older_running = operations.clone();
            older_running.push((json!({"host_tool":"tasks","action":"list"}),
                json!({"tasks":[{"id":"task-1","kind":"subagent","status":"running","result":null,"error":null}]})));
            board.restore_operations("key",&older_running).unwrap();
            let tasks=board.list("key");
            prop_assert_eq!(&tasks[0]["status"],&json!(status));
            prop_assert_eq!(&tasks[0]["result"],&result);
            prop_assert!(!board.any_running_subagent());
            prop_assert!(board.inner.lock().unwrap()[0].handle.is_none());
        }
    }

    proptest::proptest! {
        #[test]
        fn list_cannot_create_workers_or_promote_unconfirmed_entries(nonterminal in 0usize..4) {
            use proptest::prelude::*;
            let status=["open","running","interrupted","unknown"][nonterminal];
            let operations=vec![
                (json!({"host_tool":"subagent","task":"read"}),json!({"task_id":"task-1","status":"running"})),
                (json!({"host_tool":"tasks","action":"list"}),json!({"tasks":[
                    {"id":"task-1","kind":"subagent","status":status,"in_flight":false,"result":null,"error":null},
                    {"id":"unregistered","kind":"subagent","status":"done","result":{"answer":"not a registered child"}},
                    {"id":"task-1","kind":"manual","status":"done","result":{"answer":"wrong kind"}}
                ]})),
            ];
            let board=TaskBoard::default();
            board.restore_operations("root",&operations).unwrap();
            let tasks=board.list("root");
            prop_assert_eq!(tasks.len(),1);
            prop_assert_eq!(&tasks[0]["status"],&json!("interrupted"));
            prop_assert!(board.any_unfinished());
            prop_assert!(!board.any_running_subagent());
        }
    }

    #[test]
    fn list_receipts_do_not_cross_roots_or_overwrite_manual_or_live_work() {
        let dispatch = (
            json!({"host_tool":"subagent","task":"read"}),
            json!({"task_id":"task-1","status":"running"}),
        );
        let list = (
            json!({"host_tool":"tasks","action":"list"}),
            json!({"tasks":[{"id":"task-1","kind":"subagent","status":"done","result":{"answer":"confirmed"},"error":null}]}),
        );
        let board = TaskBoard::default();
        board
            .restore_operations("root-a", std::slice::from_ref(&dispatch))
            .unwrap();
        board
            .restore_operations("root-b", std::slice::from_ref(&list))
            .unwrap();
        assert_eq!(board.list("root-a")[0]["status"], "interrupted");
        assert!(board.list("root-b").is_empty());
        let manual = TaskBoard::default();
        assert_eq!(manual.create_manual("root", "manual"), "task-1");
        manual
            .restore_operations("root", &[dispatch.clone(), list.clone()])
            .unwrap();
        assert_eq!(manual.list("root")[0]["kind"], "manual");
        assert_eq!(manual.list("root")[0]["status"], "open");
        let live = TaskBoard::default();
        assert_eq!(live.dispatch("root", "live").unwrap().id, "task-1");
        live.restore_operations("root", &[dispatch, list]).unwrap();
        assert_eq!(live.list("root")[0]["status"], "running");
        assert!(live.any_running_subagent());
    }

    #[test]
    fn fresh_root_retains_old_workers_capacity_until_reaped() {
        let board = TaskBoard::default();
        let old = json!(["p", "a", "s", 1]).to_string();
        let fresh = json!(["p", "a", "s", 2]).to_string();
        let mut releases = Vec::new();
        for _ in 0..MAX_CHILDREN {
            let d = board.dispatch(&old, "old child").unwrap();
            let (release, wait) = std::sync::mpsc::channel();
            let b = board.clone();
            let id = d.id.clone();
            let key = old.clone();
            board.attach(
                &old,
                &d.id,
                std::thread::spawn(move || {
                    wait.recv().unwrap();
                    b.finish(&key, &id, "stopped", None, None);
                }),
            );
            releases.push(release);
        }
        board.begin_activation(&fresh);
        let fifth_refused = board.dispatch(&fresh, "fifth").is_none();
        for release in releases {
            release.send(()).unwrap();
        }
        board.join_pending();
        assert!(fifth_refused);
        assert!(board.dispatch(&fresh, "after cleanup").is_some());
        assert!(board.list(&old).is_empty());
    }

    proptest::proptest! {
        #[test]
        fn activation_identity_and_resume_preservation(
            parent in "[a-z]{1,12}", other in "[a-z]{1,12}", stage in "[a-z]{0,12}",
            root in 1i64..100000, resumes in 1usize..20
        ) {
            use proptest::prelude::*;
            let a = json!(["p",parent,stage,root]).to_string();
            let b = json!(["p",other,stage,root]).to_string();
            prop_assert_eq!(same_parent(&a,&b), parent == other);
            let board = TaskBoard::default();
            let id = board.create_manual(&a,"acceptance");
            for _ in 0..resumes { board.begin_activation(&a); }
            prop_assert_eq!(board.list(&a).len(),1);
            prop_assert_eq!(&board.list(&a)[0]["id"], &json!(id));
        }
    }

    #[test]
    fn truncated_child_does_not_report_a_completed_task() {
        let scope = Scope {
            approval_mode: crate::approval_mode::ApprovalMode::Restricted,
            permission_rules: Default::default(),
            halt: Arc::new(AtomicBool::new(false)),
            answer: Arc::new(Mutex::new(Some("partial answer".into()))),
            mcp: Default::default(),
            reads: Default::default(),
        };
        let done = finish_child(Ok(TurnOutcome::Truncated), &scope);
        assert_eq!(done.status, "failed");
        assert!(done.result.is_none());
        assert!(done.error.is_some());
    }

    proptest::proptest! {
        #[test]
        fn unknown_test_output_cannot_become_a_passed_check(
            done in proptest::prelude::any::<bool>(), code in proptest::prelude::any::<i32>(),
            flag in proptest::prelude::any::<bool>(), stream in "stdout|stderr"
        ) {
            use proptest::prelude::*;
            let out = json!({"done":done,"exit_code":code,"outcome_unknown":flag,
                "read_error":if flag { Value::Null } else { json!(stream) }});
            prop_assert!(test_output_known(&out).is_err());
            prop_assert!(test_output_known(&json!({"done":done,"exit_code":code,
                "outcome_unknown":false,"read_error":null})).is_ok(), "known terminal output must remain readable");
        }
    }

    /// 内联派遣（内存库）：子代理回合吃脚本、回执进任务清单、
    /// 实读路径进引用、写工具结构性缺席。
    #[test]
    fn inline_dispatch_collects_answer_and_citations() {
        let (wb, dir) = setup();
        std::fs::write(dir.path().join("note.md"), "hello").unwrap();
        let provider = ScriptedProvider::new(vec![
            tool_response(vec![("t1", "fs_read", json!({"path": "note.md"}))]),
            tool_response(vec![(
                "t2",
                "fs_write",
                json!({"path": "x", "content": "y"}),
            )]),
            text_response("结论是42"),
        ]);
        let ctx = wb.ctx_for("a0", None);
        let out = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx,
            json!({"task": "note.md 里写了什么"}),
        )
        .unwrap();
        let CallOutcome::Done(v) = out else {
            panic!("expected Done: {out:?}")
        };
        assert_eq!(v["dispatched"], true, "{v}");
        assert_eq!(v["status"], "done", "{v}");
        assert_eq!(v["result"]["answer"], "结论是42");
        let cites = v["result"]["citations"].as_array().unwrap();
        assert!(cites.iter().any(|c| c["path"] == "note.md"), "{cites:?}");
        assert!(!dir.path().join("x").exists(), "派遣域不许写盘");
    }

    /// 空任务连回合都不起——BadInput 且不留派遣事件。
    #[test]
    fn empty_task_rejected_before_turn() {
        let (wb, _d) = setup();
        let provider = ScriptedProvider::new(vec![text_response("x")]);
        let ctx = wb.ctx_for("a0", None);
        let r = call_nested(&wb.db, &provider, &wb.registry, &ctx, json!({}));
        assert!(matches!(r, Err(ToolError::BadInput(_))));
        assert!(provider.recorded().is_empty(), "空任务不许召模型");
    }

    /// 容量闸：同激活第五个派遣立即拒（不排队）。
    #[test]
    fn fifth_dispatch_rejected() {
        let (wb, _d) = setup();
        let ctx = wb.ctx_for("a0", None);
        let key = activation_key(&ctx);
        for _ in 0..MAX_CHILDREN {
            assert!(ctx.tasks.dispatch(&key, "t").is_some());
        }
        let provider = ScriptedProvider::new(vec![text_response("x")]);
        let out = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx,
            json!({"task": "full"}),
        )
        .unwrap();
        let CallOutcome::Done(v) = out else {
            panic!("expected Done")
        };
        assert_eq!(v["dispatched"], false);
        assert_eq!(v["reason"], "capacity full");
        assert!(provider.recorded().is_empty());
    }

    // I2/I3 Spec sibling: a succeeded dispatch refusal is a model-visible
    // receipt, not an accepted child. Feed the actual native producer output
    // through the shared recovery parser rather than inventing a task id.
    #[test]
    fn confirmed_capacity_refusal_preserves_existing_tasks() {
        let (wb, _dir) = setup();
        let ctx = wb.ctx_for("a0", None);
        let key = activation_key(&ctx);
        let mut operations = Vec::new();
        for _ in 0..MAX_CHILDREN {
            let accepted = ctx.tasks.dispatch(&key, "existing").unwrap();
            operations.push((
                json!({"host_tool":"subagent","task":"existing"}),
                json!({"dispatched":true,"task_id":accepted.id,"status":"running"}),
            ));
        }
        let provider = ScriptedProvider::new(vec![text_response("must not run")]);
        let CallOutcome::Done(receipt) = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx,
            json!({"task":"fifth"}),
        )
        .unwrap() else {
            panic!("refusal must remain a successful tool receipt")
        };
        assert_eq!(
            receipt,
            json!({"dispatched":false,"reason":"capacity full","running":MAX_CHILDREN})
        );
        let state: String = wb
            .db
            .conn()
            .query_row(
                "SELECT state FROM tool_actions WHERE tool='subagent' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "succeeded");
        assert!(provider.recorded().is_empty());
        operations.push((json!({"host_tool":"subagent","task":"fifth"}), receipt));
        TaskBoard::validate_operations(&operations).unwrap();
        let restored = TaskBoard::default();
        restored.restore_operations(&key, &operations).unwrap();
        assert_eq!(restored.list(&key).len(), MAX_CHILDREN);
        assert!(restored
            .list(&key)
            .iter()
            .all(|task| task["status"] == "interrupted"));
        assert!(!restored.any_running_subagent());
        assert_eq!(
            restored.create_manual(&key, "next"),
            format!("task-{}", MAX_CHILDREN + 1)
        );
    }

    #[test]
    fn confirmed_scope_refusals_do_not_create_children() {
        let (wb, _dir) = setup();
        let provider = ScriptedProvider::new(vec![text_response("must not run")]);
        let mut operations = Vec::new();
        for child_scope in [false, true] {
            crate::orchestra::write_agent_status(&wb.db, "p1", "a0", !child_scope).unwrap();
            let mut ctx = wb.ctx_for("a0", None);
            if child_scope {
                ctx.subagent = Some(Scope {
                    approval_mode: crate::approval_mode::ApprovalMode::Restricted,
                    permission_rules: Default::default(),
                    halt: Arc::new(AtomicBool::new(false)),
                    answer: Default::default(),
                    mcp: Default::default(),
                    reads: Default::default(),
                });
            }
            let CallOutcome::Done(receipt) = call_nested(
                &wb.db,
                &provider,
                &wb.registry,
                &ctx,
                json!({"task":"denied scope"}),
            )
            .unwrap() else {
                panic!("scope refusal is a successful tool receipt")
            };
            assert_eq!(
                receipt,
                json!({"dispatched":false,"reason":if child_scope {"subagents cannot dispatch"}else{"parent agent sleeping"}})
            );
            operations.push((
                json!({"host_tool":"subagent","task":"denied scope"}),
                receipt,
            ));
        }
        assert!(provider.recorded().is_empty());
        TaskBoard::validate_operations(&operations).unwrap();
        let restored = TaskBoard::default();
        restored.restore_operations("root", &operations).unwrap();
        assert!(restored.list("root").is_empty());
        assert!(!restored.any_running_subagent());
    }

    proptest::proptest! {
        #[test]
        fn known_undispatched_receipts_do_not_complete_or_create_children(reason in 0usize..3) {
            use proptest::prelude::*;
            let receipt=match reason {
                0=>json!({"dispatched":false,"reason":"capacity full","running":MAX_CHILDREN}),
                1=>json!({"dispatched":false,"reason":"parent agent sleeping"}),
                _=>json!({"dispatched":false,"reason":"subagents cannot dispatch"}),
            };
            let operations=vec![
                (json!({"host_tool":"tasks","action":"create","title":"acceptance"}),json!({"task_id":"task-1"})),
                (json!({"host_tool":"subagent","task":"prior"}),json!({"task_id":"task-2","status":"running"})),
                (json!({"host_tool":"subagent","task":"refused"}),receipt),
            ];
            let board=TaskBoard::default();
            prop_assert!(TaskBoard::validate_operations(&operations).is_ok());
            board.restore_operations("root",&operations).unwrap();
            let tasks=board.list("root");
            prop_assert_eq!(tasks.len(),2);
            prop_assert_eq!(tasks[0]["status"].as_str(),Some("open"));
            prop_assert_eq!(tasks[1]["status"].as_str(),Some("interrupted"));
            prop_assert!(!board.any_running_subagent());
            prop_assert_eq!(board.create_manual("root","next"),"task-3");
        }

        #[test]
        fn unknown_or_malformed_undispatched_receipts_remain_rejected(shape in 0usize..15) {
            use proptest::prelude::*;
            let receipt=match shape {
                0=>json!({"dispatched":false,"reason":"unknown refusal"}),
                1=>json!({"dispatched":false,"reason":"capacity full"}),
                2=>json!({"dispatched":false,"reason":"capacity full","running":MAX_CHILDREN-1}),
                3=>json!({"dispatched":false,"reason":"capacity full","running":"4"}),
                4=>json!({"dispatched":false,"reason":"parent agent sleeping","task_id":"task-1","status":"done","result":{"answer":"invented"}}),
                5=>json!({"dispatched":false,"reason":"subagents cannot dispatch","status":"done"}),
                6=>json!({"dispatched":"false","reason":"parent agent sleeping"}),
                7=>json!({"dispatched":false}),
                8=>json!({"reason":"parent agent sleeping"}),
                9=>json!({"dispatched":false,"reason":"capacity full","running":MAX_CHILDREN,"unexpected":true}),
                _=>json!({"dispatched":false,"reason":"parent agent sleeping"}),
            };
            let input=match shape {
                10=>json!({"host_tool":"subagent","task":null}),
                11=>json!({"host_tool":"subagent"}),
                12=>json!({"host_tool":"subagent","task":""}),
                13=>json!({"host_tool":"subagent","task":"  \n\t"}),
                14=>json!({"host_tool":"subagent","task":42}),
                _=>json!({"host_tool":"subagent","task":"refused"}),
            };
            let operations=[(input,receipt)];
            prop_assert!(TaskBoard::validate_operations(&operations).is_err());
            let board=TaskBoard::default();
            prop_assert!(board.restore_operations("root",&operations).is_err());
            prop_assert!(board.list("root").is_empty());
            prop_assert!(!board.any_running_subagent());
        }
    }

    /// mcp 勾选过滤：未授权/不在场的名字进 dropped 不进选择集。
    #[test]
    fn mcp_pick_filters_unauthorized() {
        let (wb, _d) = setup();
        let provider = ScriptedProvider::new(vec![text_response("done")]);
        let ctx = wb.ctx_for("a0", None);
        let out = call_nested(
            &wb.db,
            &provider,
            &wb.registry,
            &ctx,
            json!({"task": "t", "mcp_tools": ["mcp:ghost:tool", "not-mcp", 42]}),
        )
        .unwrap();
        let CallOutcome::Done(v) = out else {
            panic!("expected Done")
        };
        assert_eq!(v["dispatched"], true);
        assert!(v["mcp_selected"].as_array().unwrap().is_empty());
        assert_eq!(v["mcp_dropped"].as_array().unwrap().len(), 3);
    }

    /// run_test 闸：测试形态过；写文件/git/管道/越界路径全机械拒。
    #[test]
    fn run_test_gate_shapes() {
        let (wb, dir) = setup();
        let ctx = wb.ctx_for("a0", None);
        let ok = [
            "cargo test",
            "cargo test --package hexagon-core",
            "npm test",
            "npm run test:unit",
            "pnpm vitest run",
            "python3 -m pytest tests/",
            "go test ./...",
            "CI=true cargo test",
        ];
        for c in ok {
            assert!(test_cmd_gate(c, &ctx).is_ok(), "should pass: {c}");
        }
        let bad = [
            "rm -rf x",
            "cargo build",
            "cargo test && rm -f a",
            "cargo test -u",        // 快照更新=写源文件
            "cargo test > out.txt", // 重定向
            "cat $(secret)",        // 命令替换
            "vitest --update",
            "sh -c 'cargo test'",
            "cargo test ../outside", // 越界操作数
        ];
        let _ = dir;
        for c in bad {
            assert!(test_cmd_gate(c, &ctx).is_err(), "should deny: {c}");
        }
    }

    /// D12：新判定器配 proptest。fail-closed 方向钉死——含不透明构造
    /// （重定向/替换/子 shell）或非白名单首词的命令永不放行；任意字节
    /// 序列不得 panic。
    #[test]
    fn run_test_gate_never_passes_opaque_or_foreign() {
        use proptest::prelude::*;
        let (wb, _d) = setup();
        let ctx = wb.ctx_for("a0", None);
        proptest!(|(cmd in ".*")| {
            let r = test_cmd_gate(&cmd, &ctx);
            let _ = r; // 不 panic 即半条不变量
            // reliability 04 intentionally permits the host-assigned output
            // variable; all other dynamic shell sources still fail closed.
            if cmd.contains('$') && !cmd.contains("$HEXAGON_TEST_OUTPUT") && !cmd.contains("${HEXAGON_TEST_OUTPUT}") {
                prop_assert!(r.is_err());
            }
            for t in ["`", "$(", ">", "<", "("] {
                if cmd.contains(t) {
                    prop_assert!(r.is_err(), "opaque {t:?} passed: {cmd:?}");
                }
            }
            // 首词落在测试白名单之外 → 必拒（剥 env 赋值与路径前缀后）。
            let first = cmd
                .split_whitespace()
                .find(|t| !t.contains('='))
                .unwrap_or("")
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("")
                .trim_end_matches(".exe")
                .to_lowercase();
            let runners = [
                "cargo", "npm", "npx", "pnpm", "yarn", "python", "python3", "go",
                "dotnet", "mix", "vitest", "jest", "mocha", "pytest", "rspec", "ctest",
            ];
            if !first.is_empty() && !runners.contains(&first.as_str()) && r.is_ok() {
                // 唯一豁免：非 runner 首词但 split_commands 切出的段全合法——
                // 不可能，闸门逐段查首词。命中即真漏洞。
                prop_assert!(false, "non-runner passed: {cmd:?}");
            }
        });
    }

    /// tasks 工具：create/close/stop/list 生命周期。
    #[test]
    fn tasks_tool_lifecycle() {
        let (wb, _d) = setup();
        let db = &wb.db;
        let ctx = wb.ctx_for("a0", None);
        let t = Tasks;
        let id = t
            .exec(db, &json!({"action": "create", "title": "盘点接口"}), &ctx)
            .unwrap()["task_id"]
            .as_str()
            .unwrap()
            .to_string();
        let list = t.exec(db, &json!({"action": "list"}), &ctx).unwrap();
        assert_eq!(list["tasks"].as_array().unwrap().len(), 1);
        assert!(t
            .exec(db, &json!({"action": "close", "task_id": id}), &ctx)
            .unwrap()["closed"]
            .as_bool()
            .unwrap());
        // 已关闭不能再勾
        assert!(!t
            .exec(db, &json!({"action": "close", "task_id": id}), &ctx)
            .unwrap()["closed"]
            .as_bool()
            .unwrap());
        // I3: starting another turn of this root used to erase the board.
        ctx.tasks.begin_activation(&activation_key(&ctx));
        let list = t.exec(db, &json!({"action": "list"}), &ctx).unwrap();
        assert_eq!(list["tasks"].as_array().unwrap().len(), 1);
        let mut fresh = ctx.clone();
        fresh.activation_root_turn_id = Some(900);
        fresh.tasks.begin_activation(&activation_key(&fresh));
        assert!(
            t.exec(db, &json!({"action":"list"}), &fresh).unwrap()["tasks"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
