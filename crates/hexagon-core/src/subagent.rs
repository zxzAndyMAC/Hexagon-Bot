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
//! 随激活生灭，不碰阶段指针，不是第二个后台任务系统。

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

/// 任务键 = 激活标识：阶段 run 在场用 run id（同一激活跨回合共享），
/// 快速通道用 agent id（该 Agent 当前激活）。
pub fn activation_key(ctx: &ToolContext) -> String {
    ctx.stage_run_id
        .clone()
        .unwrap_or_else(|| ctx.agent_id.clone())
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
    /// 新激活起跑清场：该键下残留条目 halt + 清空。
    /// 「激活结束清单随之清空」的落点——激活边界就是回合起跑线：
    /// 一次激活里可以开多回合吗？当前实现每回合即一次激活派发单位，
    /// 清在回合起跑线保守且可观测（票 07：不留到下一个激活）。
    pub fn begin_activation(&self, key: &str) {
        let mut tasks = self.inner.lock().unwrap();
        for t in tasks.iter_mut().filter(|t| t.key == key) {
            t.halt.store(true, Ordering::Relaxed);
        }
        tasks.retain(|t| t.key != key);
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
        });
        id
    }

    /// 子代理派遣登记（容量闸内聚在锁里：查数与落位同一临界区，
    /// 并发派遣不会越过上限）。满 → None。
    pub fn dispatch(&self, key: &str, title: &str) -> Option<Dispatched> {
        let mut tasks = self.inner.lock().unwrap();
        let running = tasks
            .iter()
            .filter(|t| t.key == key && t.kind == TaskKind::Subagent && t.status == "running")
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
        });
        Some(Dispatched {
            id,
            halt,
            answer,
            reads,
        })
    }

    pub fn attach(&self, id: &str, handle: std::thread::JoinHandle<()>) {
        let mut tasks = self.inner.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
            t.handle = Some(handle);
        }
    }

    /// 收尾：线程与内联路径同一出口。
    pub fn finish(
        &self,
        id: &str,
        status: &'static str,
        result: Option<Value>,
        error: Option<String>,
    ) {
        let mut tasks = self.inner.lock().unwrap();
        if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
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
        if t.kind == TaskKind::Manual && t.status == "open" {
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
                    "result": t.result,
                    "error": t.error,
                })
            })
            .collect()
    }

    pub fn running_subagents(&self, key: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.key == key && t.kind == TaskKind::Subagent && t.status == "running")
            .count()
    }

    /// 任一激活还有子代理没交还（失速监视用：子代理未交还不算失速，ADR 0074）。
    /// 派遣线程可以比父回合活得久，父回合收口不代表子代理已交还。
    pub fn any_running_subagent(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .any(|t| t.kind == TaskKind::Subagent && t.status == "running")
    }

    /// 测试接缝：等所有在跑的派遣线程收尾（join 而不是轮询）。
    #[cfg(test)]
    pub fn join_pending(&self) {
        loop {
            let handle = {
                let mut tasks = self.inner.lock().unwrap();
                tasks.iter_mut().find_map(|t| t.handle.take())
            };
            match handle {
                Some(h) => {
                    let _ = h.join();
                }
                None => return,
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
        Ok(TurnOutcome::Finished) | Ok(TurnOutcome::Truncated) => ChildDone {
            status: "done",
            result: Some(json!({"answer": clip(answer), "citations": citations})),
            error: None,
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
        halt: disp.halt,
        answer: disp.answer,
        mcp: Arc::new(pick.clone()),
        reads: disp.reads,
    };
    let mut nctx = ctx.clone();
    nctx.subagent = Some(scope);
    let sub_registry = registry.subagent_scope(&isolated);

    // 线程派遣需要两件套：可移动的第二连接（内存库没有文件路径，退化为
    // 内联同步——测试接缝的诚实降级）与可移动的 provider Arc。
    match (db.path(), ctx.subagent_provider.clone()) {
        (Some(path), Some(prov)) => {
            let board = ctx.tasks.clone();
            let id2 = id.clone();
            let handle = std::thread::spawn(move || {
                let done = match Db::open(&path) {
                    Ok(db2) => run_child(&db2, prov.as_ref(), &sub_registry, &nctx, &task),
                    Err(e) => ChildDone {
                        status: "failed",
                        result: None,
                        error: Some(format!("db open: {e}")),
                    },
                };
                board.finish(&id2, done.status, done.result, done.error);
            });
            ctx.tasks.attach(&id, handle);
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
            ctx.tasks
                .finish(&id, done.status, done.result.clone(), done.error.clone());
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
- `action`: "list" shows every task with its status and result; "create" adds an item (`title`); "close" marks an item done (`task_id`); "stop" halts a running task (`task_id`). The list is cleared when the activation ends.
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
/// （改动才重嵌），再全块内积取 top-k。
pub struct SemSearch;
impl crate::tools::Tool for SemSearch {
    fn name(&self) -> &str {
        "sem_search"
    }
    fn description(&self) -> &str {
        r#"Semantic search over the repo using a local index (no network).
- Use when: you are looking for code by meaning ("where are permissions evaluated?") and do not know the exact identifier.
- Do not use: when you know the literal text — fs_grep is exact and cheaper.
- Returns file path, line number and a short excerpt per hit. Empty hits mean nothing close was found: fall back to fs_grep or fs_find."#
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
        let embedder = ctx
            .embedder
            .clone()
            .unwrap_or_else(crate::semsearch::default_embedder);
        let indexed = crate::semsearch::refresh(db, &ctx.repo_root, embedder.as_ref())?;
        let hits = crate::semsearch::query(db, &ctx.repo_root, embedder.as_ref(), query, cap)?;
        Ok(json!({
            "count": hits.len(),
            "indexed_files": indexed,
            "hits": hits,
            "fallback": hits.is_empty(),
            "note": if hits.is_empty() { "no semantic hits — try fs_grep with a literal term" } else { "" },
        }))
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
            return Err(ToolError::Exec(format!("run_test denied: {reason}")));
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
        // 激活边界清场
        ctx.tasks.begin_activation(&activation_key(&ctx));
        let list = t.exec(db, &json!({"action": "list"}), &ctx).unwrap();
        assert!(list["tasks"].as_array().unwrap().is_empty());
    }
}
