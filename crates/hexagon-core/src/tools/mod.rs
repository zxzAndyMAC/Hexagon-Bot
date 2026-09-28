//! 工具层：一切 Agent 动作的单一管线。
//!
//! 注册表 → 内置 deny（最高优先，不可覆盖）→ 权限求值（本票为占位：
//! 默认 ask，完整五层在票 11）→ 执行。调用与结果全部落轨迹事件。
//! MCP 工具在票 13 汇入同一管线。

use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const BASH_OUTPUT_CAP: usize = 64 * 1024;
pub const FS_READ_CAP: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error(
        "write conflict for action {action_id}: {path}; reread targets and create a new action"
    )]
    WriteConflict { action_id: String, path: String },
    #[error("path escapes repo root: {0}")]
    PathEscape(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("exec: {0}")]
    Exec(String),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Cards(#[from] crate::cards::CardsError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("artifact: {0}")]
    Artifact(#[from] Box<crate::artifacts::ArtifactError>),
    #[error("unknown question: {0}")]
    UnknownQuestion(String),
    #[error("tool outcome unknown; reconcile before continuing: {0}")]
    OutcomeUnknown(String),
    #[error("tool action already executing: {0}")]
    ActionInProgress(String),
    #[error("authorized action must be resumed before new work: {0}")]
    ActionReady(String),
    #[error("tool was not executed: {0}")]
    NotExecuted(String),
}

#[derive(Clone)]
pub struct ToolContext {
    pub project_id: String,
    pub agent_id: String,
    pub repo_root: PathBuf,
    pub stage_run_id: Option<String>,
    /// 路径归属 glob：非空时写入必须命中其一，否则拒绝。
    /// 空 = 不增加归属限制；宿主保护和其他硬拒绝仍生效。
    pub owned_globs: Vec<String>,
    /// 产物档位注册表（自定义类型挂档用；内置映射不可降级）。
    pub tiers: crate::artifacts::TierMap,
    /// 终端会话表（agent-senses 票 05）：Workbench 注入共享表，
    /// 临时构造的 ctx 拿独立空表（一次性路径无会话复用需求）。
    pub sessions: crate::sessions::SessionTable,
    /// 模型槽能力集（agent-senses 票 02）：`vision` 缺位时 fs_read
    /// 读图降级为路径文本——字节不进上下文（fail-closed 默认空集，
    /// 临时构造的 ctx 自然无图能力）。
    pub caps: std::collections::HashSet<String>,
    /// 断网等待策略（network-resilience 票 01）：Transport 快重试耗尽后
    /// 的等网参数——生产常量，测试注入毫秒级。`Default` = 生产值。
    pub wait: crate::turn::WaitPolicy,
    /// reliability 10: caller deadline can only shorten the MCP tool budget.
    pub deadline: Option<std::time::Instant>,
    pub mcp_timeout: std::time::Duration,
    /// Host-supplied durable key; only tools declaring a real idempotency
    /// contract may rely on it. Never supplied by the model or provider call ID.
    pub action_key: Option<String>,
    /// Host-owned execution lease, reused by nested local materialization.
    pub(crate) write_lease: Option<Arc<std::fs::File>>,
    /// Fresh for each execution; remote tools cannot populate this host channel.
    pub(crate) native_effect: Option<Arc<std::sync::Mutex<Option<effects::NativeEffect>>>>,
    /// 激活任务清单（code-search 票 07）：Workbench 注入共享板；
    /// 临时构造的 ctx 拿独立空板（一次性路径无跨回合任务）。
    pub tasks: crate::subagent::TaskBoard,
    /// 非空 = 本 ctx 是一次子代理派遣（票 04/06）：halt 旗、回执格、
    /// 本次勾选的 mcp 集、实读路径格。父级 ctx 恒为 None。
    pub subagent: Option<crate::subagent::Scope>,
    /// web 搜索摘要槽（票 03）：None = 未配置（工具回报「槽未配置」）。
    pub websearch: Option<Arc<dyn crate::websearch::SearchBackend>>,
    /// 语义索引嵌入器（票 02）：None = 默认本地哈希嵌入器。
    /// 测试注入替身——嵌入向量本身是实现细节，替身只要确定性。
    pub embedder: Option<Arc<dyn crate::semsearch::Embedder>>,
    /// 子代理线程派遣要移动的 provider Arc（run_turn_opts 注入；
    /// 直测/内存库路径为 None → 派遣退化为同连接内联执行）。
    pub subagent_provider: Option<Arc<dyn crate::provider::ModelProvider>>,
    /// 激活级已读账本（prompt-engineering 票 02）：先读后改判定的依据。
    /// ctx 每回合新建，账本随激活清空。
    pub reads: readstate::ReadLedger,
}

impl Default for ToolContext {
    /// 字面量构造点的填充底（`..Default::default()`）：project_id 取
    /// 项目常量，其余零值——各构造点照旧显式写它在乎的字段。
    fn default() -> Self {
        Self {
            project_id: crate::PROJECT_ID.into(),
            agent_id: String::new(),
            repo_root: PathBuf::new(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
            wait: Default::default(),
            deadline: None,
            mcp_timeout: std::time::Duration::from_secs(120),
            action_key: None,
            write_lease: None,
            native_effect: None,
            tasks: Default::default(),
            subagent: None,
            websearch: None,
            embedder: None,
            subagent_provider: None,
            reads: Default::default(),
        }
    }
}

impl ToolContext {
    /// 负责人上下文（ADR 0052 控制通道）：壳层裁决类命令经第二连接
    /// 操作时构造——与 `Workbench::ctx_for("owner", None)` 同一配方。
    pub fn owner(db: &Db, repo_root: &Path) -> Self {
        Self::for_agent(db, repo_root, "owner")
    }

    /// 指定 agent 的无阶段上下文——同 `Workbench::ctx_for(agent, None)`。
    /// 复审者裁决（review_proposal 的 reviewer_agent）走这个。
    pub fn for_agent(db: &Db, repo_root: &Path, agent_id: &str) -> Self {
        Self {
            project_id: crate::PROJECT_ID.into(),
            agent_id: agent_id.into(),
            repo_root: repo_root.to_path_buf(),
            stage_run_id: None,
            owned_globs: crate::permissions::agent_globs(db, agent_id).unwrap_or_default(),
            tiers: crate::artifacts::TierMap::new(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        }
    }
}

/// 一次工具调用的结局。
#[derive(Debug)]
pub enum CallOutcome {
    /// 已执行，结果已落 ToolResult 事件
    Done(Value),
    /// 被内置 deny 拦下（事件已落，带原因）
    Denied(String),
    /// 转必问：pending_questions 已入队，返回问题 id
    Asked(String),
}

/// 工具风险类（openworker-borrow 票 08）：权限求值的单一声明轴，取代
/// `needs_ask` 零散布尔——加新工具只声明类，不碰 evaluate 管线。
/// `builtin_deny` 保留不动：它是带理由的「输入级」硬拒（凭据路径、权限
/// 规则文件），与类级声明不同轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskClass {
    /// 仓内纯读：默认放行（L1 deny/安全网/项目规则照常先跑）。
    Read,
    /// 出网/外发：域名绑定规则可记忆放行，其余必问。
    Egress,
    /// 本地写入：ownership globs 圈内默认放行。
    WriteLocal,
    /// 任意命令执行：形状记忆可放行，其余必问。
    Exec,
    /// 外部服务调用（mcp:*）：语义由第三方服务器自定。记忆 allow 和规则写入
    /// 永不生效。自治 L3+ 把这一次询问放行并留轨迹，不等于记住——降回 L2
    /// 仍逐次必问（票 03 / ADR 0059）。被否决：高档位也弹卡。
    External,
}

/// Reliability 11: only host-installed contracts may assert an observed outcome.
/// Tool descriptions, model text and MCP annotations cannot implement this hook.
#[derive(Debug)]
pub enum Reconciliation {
    Unresolved { evidence: String },
    Succeeded { output: Value, evidence: String },
    NotExecuted { evidence: String },
}

pub struct IdempotencyContract {
    pub version: String,
    pub validity: std::time::Duration,
}

pub trait Tool: Send + Sync {
    /// Host-owned complete file target manifest. Opaque external tools do not
    /// claim this guarantee. Paths are repository-relative, never model grants.
    fn write_targets(&self, _input: &Value) -> Result<Vec<String>, ToolError> {
        Ok(vec![])
    }

    /// Trusted host adapter guarantee, not a remote annotation. exec must pass
    /// ctx.action_key to the actual provider's deduplication mechanism.
    fn idempotency_contract(&self) -> Option<IdempotencyContract> {
        None
    }

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数命名约定（票 14 备忘）：参数名别撞宿主模板/语言方法名——
    /// OpenWorker 真实事故：todo 工具参数叫 `items`，minijinja 把它
    /// 解析成 dict.items() 方法调用，直接 400。`items`/`keys`/`values`/
    /// `get`/`update` 这类名字禁用；宁可 `entries`/`todo_list`。
    fn input_schema(&self) -> Value;
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError>;
    /// Must only read a receipt/postcondition; never execute the original action.
    fn reconcile(
        &self,
        _db: &Db,
        _input: &Value,
        _ctx: &ToolContext,
        _action_id: &str,
    ) -> Result<Reconciliation, ToolError> {
        Ok(Reconciliation::Unresolved {
            evidence: "tool has no read-only reconciliation contract".into(),
        })
    }
    /// reliability 05: metadata and a parent's grant cannot attest read-only
    /// behavior. Only a host implementation creating a separately confined
    /// capability may override this; never return a parent's writable session.
    fn for_subagent(&self, _ctx: &ToolContext) -> Result<Arc<dyn Tool>, ToolError> {
        Err(ToolError::Exec(
            "host-enforced read-only capability unavailable".into(),
        ))
    }
    /// 风险类声明（票 08）。默认 Exec——最保守的「默认必问」档，
    /// 新工具忘了声明也不会意外变成免问。
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    /// 内置 deny 检查：Some(reason) = 拦下。最高优先，不可覆盖。
    fn builtin_deny(&self, _input: &Value, _ctx: &ToolContext) -> Option<String> {
        None
    }
    /// 执行前提（票 02）：只在 `call_with_seq` 里、权限判定之前检查——注定
    /// 失败的调用不弹卡。批准后的执行由 writeguard 核对持久目标前提。
    fn precondition(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ToolError> {
        Ok(())
    }
}

/// 拒绝来源标签（prompt-engineering 票 04）：进模型可见的拒绝结果，让模型
/// 分得清「换个做法能过」（rule）与「这条路结构性不通」（builtin）。
fn deny_source(layer: &str) -> &'static str {
    match layer {
        "builtin_deny" => "builtin",
        _ => "rule",
    }
}

pub struct Registry {
    /// Arc 共享：只读视图（研究助手嵌套回合）按名拷 mcp:* 而不起新进程。
    /// Mutex：MCP 握手在后台完成后再注册，回合进行中也能加工具（2026-09-22）。
    tools: std::sync::Mutex<HashMap<String, Arc<dyn Tool>>>,
    schemas: schema::SchemaCache,
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Registry {
    pub fn builtin() -> Self {
        let r = Self {
            tools: std::sync::Mutex::new(HashMap::new()),
            schemas: Default::default(),
        };
        r.register(FsRead);
        r.register(FsFind);
        r.register(FsGrep);
        r.register(crate::subagent::SemSearch);
        r.register(crate::websearch::WebSearch);
        r.register(crate::subagent::Subagent);
        r.register(crate::subagent::Tasks);
        r.register(FsWrite);
        r.register(FsPatch);
        r.register(Bash);
        r.register(BashOutput);
        r.register(BashKill);
        r.register(ArtifactWrite);
        r.register(ArtifactRead);
        r.register(LoadSkill);
        r.register(ProposeExperience);
        r.register(WebFetch);
        r.register(crate::git::GitBaselineMerge);
        r
    }

    pub fn register(&self, tool: impl Tool + 'static) {
        self.schemas.invalidate(tool.name());
        self.tools
            .lock()
            .unwrap()
            .insert(tool.name().to_string(), Arc::new(tool));
    }

    /// 工具是否在场（子代理勾选的合法性闸看注册表不看权限——
    /// 不在场的名字连「这次勾选」都谈不上）。
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.lock().unwrap().get(name).cloned()
    }

    /// 子代理注册表（code-search-and-subagent 票 04/06）：只读工具 +
    /// 仓内搜索 + web 摘要 + 测试执行（自带 OS 隔离）+ 宿主新建的只读 MCP 能力。
    /// 没有写/bash/git/web_fetch/子代理入口 → 结构性不可写、不可再派生、
    /// 不可外带。未通过授权/勾选/隔离的 MCP 连名字都不可见（defs 不列出）——
    /// 授权闸门是第二道，第一道是注册表本身。
    pub fn subagent_scope(&self, isolated: &[Arc<dyn Tool>]) -> Self {
        let r = Self {
            tools: std::sync::Mutex::new(HashMap::new()),
            schemas: Default::default(),
        };
        r.register(FsRead);
        r.register(FsFind);
        r.register(FsGrep);
        r.register(crate::subagent::SemSearch);
        r.register(crate::subagent::RunTest);
        r.register(ArtifactRead);
        r.register(LoadSkill);
        r.register(crate::websearch::WebSearch);
        for tool in isolated {
            r.tools
                .lock()
                .unwrap()
                .insert(tool.name().to_string(), tool.clone());
        }
        r
    }

    /// 供供应商请求用的工具清单（名字 + 描述 + schema）。
    pub fn defs(&self) -> Vec<crate::provider::ToolDef> {
        self.defs_for_ctx(None)
    }

    /// ctx 相关的工具清单（票 03）：模型槽 caps 含 `web` 时本地
    /// web_search 不上清单——供应商原生 web_search 在场（Anthropic
    /// server tool 与本工具同名，并存会撞名；规格本就要求二选一）。
    pub fn defs_for_ctx(&self, ctx: Option<&ToolContext>) -> Vec<crate::provider::ToolDef> {
        let guard = self.tools.lock().unwrap();
        let native_web = ctx.is_some_and(|c| c.caps.contains("web"));
        let mut v: Vec<_> = guard
            .values()
            .filter(|t| !(native_web && t.name() == "web_search"))
            .map(|t| crate::provider::ToolDef {
                name: t.name().into(),
                description: t.description().into(),
                input_schema: t.input_schema(),
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// 入参按工具 schema 校验（票 03）。`call_with_seq` 与回合层截获的
    /// `subagent` 共用——截获路径不经主管线，不单独调用就漏校验。
    pub fn validate_input(
        &self,
        ctx: &ToolContext,
        tool: &dyn Tool,
        input: &Value,
    ) -> Result<(), ToolError> {
        let started = std::time::Instant::now();
        let name = tool.name();
        self.schemas
            .validate(name, &tool.input_schema(), input)
            .map_err(|why| {
                crate::diag::note(
                    crate::diag::CLASS_REJECT,
                    true,
                    Some(&ctx.project_id),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                    None,
                    "tool_input",
                    &format!("schema_invalid:{name}"),
                    started,
                );
                ToolError::BadInput(format!(
                    "invalid arguments for {name}: {why}. Fix the arguments to match the tool's input schema and call again."
                ))
            })
    }

    /// 主管线：deny → ask → exec，全程落事件。
    pub fn call(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        input: Value,
    ) -> Result<CallOutcome, ToolError> {
        self.call_with_seq(db, ctx, name, input, None)
    }

    /// 带幂等序号的调用（票 11）。`call_seq` 形如 "r{round}:i{index}"——
    /// 回合内位置在重放中稳定；只有模型回合的工具循环传它，
    /// 其余调用点（owner resolve/git/mcp 测试）不经重放，传 None。
    pub fn call_with_seq(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        input: Value,
        call_seq: Option<&str>,
    ) -> Result<CallOutcome, ToolError> {
        crate::evaluation::control::checkpoint(&ctx.repo_root)
            .map_err(|e| ToolError::NotExecuted(e.to_string()))?;
        let tool = self
            .tools
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::BadInput(format!("unknown tool: {name}")))?;

        let action = crate::actions::prepare(db, ctx, name, &input, call_seq)?;
        if let Err(error) = crate::actions::ensure_clear_except(db, ctx, &action.id) {
            // Evaluation 27: a different unresolved action can reject this fresh
            // call before validation. Close only its still-unstarted CAS row;
            // the original unknown/authorized action and its card stay intact.
            crate::actions::fail_unstarted_attempt(
                db,
                ctx,
                &action.id,
                &ToolError::NotExecuted(error.to_string()),
            )?;
            return Err(error);
        }
        if let Some(outcome) = crate::actions::replay(&action)? {
            if let CallOutcome::Asked(qid) = &outcome {
                db.append_event(
                    &ctx.project_id,
                    EventKind::PermissionAsked,
                    json!({"question_id":qid,"action_id":action.id,"deduped":true}),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                )?;
            }
            return Ok(outcome);
        }
        if action.state == "authorized" {
            return self.resume_action(db, ctx, &action.id);
        }

        // Evaluation 27 (eval-6/action17): ordinary preflight failures used to
        // strand pending actions; recovery-only cleanup missed this common path.
        // Reuse the CAS finalizer: authorized/dispatched/queued actions must keep
        // their evidence, never turn an uncertain effect into "not executed".
        let result = (|| {
            // 调用事件：敏感入参（fs_write 的 content 等）只记元信息。
            // seq 入载荷（票 11）：中断重放时同一调用可被指认。
            db.append_event(
            &ctx.project_id,
            EventKind::ToolCalled,
            json!({ "tool": name, "input": scrub_input(name, &input), "seq": call_seq, "action_id":action.id }),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;

            log::debug!(
                "tool call: {} agent={} input={}",
                name,
                ctx.agent_id,
                scrub_input(name, &input)
            );
            // 票 03（prompt-engineering）：入参校验早于权限——坏参数不弹卡、
            // 不落权限事件，以 BadInput 回喂模型并计入同错熔断。
            self.validate_input(ctx, tool.as_ref(), &input)?;
            tool.precondition(&input, ctx)?;
            match crate::permissions::evaluate_logged(db, ctx, tool.as_ref(), name, &input)? {
                crate::permissions::Decision::Deny { reason, layer } => {
                    crate::actions::denied(
                        db,
                        ctx,
                        &action.id,
                        &format!("({}): {reason}", deny_source(layer)),
                    )?;
                    db.append_event(
                        &ctx.project_id,
                        EventKind::PermissionDenied,
                        json!({ "tool": name, "layer": layer, "reason": reason }),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                    Ok(CallOutcome::Denied(format!(
                        "({}): {reason}",
                        deny_source(layer)
                    )))
                }
                crate::permissions::Decision::Ask { reason, safety_net } => {
                    writeguard::capture(db, ctx, tool.as_ref(), &input, &action.id)?;
                    let idem_key = Some(action.id.clone());
                    // 票 03：必问卡附溯源注记 + 激活冻结的 known world——
                    // 负责人能看到「这文件是 agent N 步前写的」「这 remote 不在初始列表」
                    let prov = crate::provenance::note(db, ctx, name, &input);
                    let world = crate::provenance::known_world(
                        db,
                        &ctx.project_id,
                        ctx.stage_run_id.as_deref(),
                    );
                    let delta = crate::provenance::remote_delta(name, &input, world.as_ref());
                    // 卡表写口归 cards.rs（arch-review 票 04）
                    let tx = db.conn().unchecked_transaction()?;
                    let qid = crate::cards::enqueue(
                        db,
                        &ctx.project_id,
                        Some(&ctx.agent_id),
                        crate::cards::CardKind::Permission,
                        json!({ "tool": name, "input": scrub_input(name, &input),
                            "raw_input": input, "reason": reason, "safety_net": safety_net, "action_id":action.id,
                            "write_targets": tool.write_targets(&input)?,
                            "provenance": prov, "known_world": world,
                            "remote_delta": delta }),
                        idem_key.as_deref(),
                    )?;
                    crate::actions::set_question(db, ctx, &action.id, &qid)?;
                    db.append_event(
                    &ctx.project_id,
                    EventKind::PermissionAsked,
                    json!({ "tool": name, "question_id": qid, "reason": reason, "safety_net": safety_net }),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                )?;
                    tx.commit()?;
                    Ok(CallOutcome::Asked(qid))
                }
                crate::permissions::Decision::Allow { via } => {
                    writeguard::capture(db, ctx, tool.as_ref(), &input, &action.id)?;
                    match &via {
                        crate::permissions::AllowVia::Remembered { shape, scope } => {
                            db.append_event(
                            &ctx.project_id,
                            EventKind::PermissionAllowed,
                            json!({ "tool": name, "layer": "remembered", "shape": shape, "scope": scope }),
                            Some(&ctx.agent_id),
                            ctx.stage_run_id.as_deref(),
                        )?;
                        }
                        // 票 03：和负责人裁决（via=owner）分开，时间线能看出是档位放行。
                        crate::permissions::AllowVia::Autonomy {
                            level,
                            safety_net,
                            reason,
                        } => {
                            db.append_event(
                                &ctx.project_id,
                                EventKind::PermissionAllowed,
                                json!({
                                    "tool": name,
                                    "via": "autonomy",
                                    "level": format!("L{level}"),
                                    "safety_net": safety_net,
                                    "reason": reason,
                                }),
                                Some(&ctx.agent_id),
                                ctx.stage_run_id.as_deref(),
                            )?;
                        }
                        crate::permissions::AllowVia::Default => {}
                    }
                    crate::actions::authorize(db, ctx, &action.id)?;
                    self.exec_and_log(db, ctx, name, input, call_seq, &action.id, true)
                        .map(CallOutcome::Done)
                }
            }
        })();
        if let Err(error) = &result {
            crate::actions::fail_unstarted_attempt(db, ctx, &action.id, error)?;
        }
        result
    }

    /// 必问裁决：批准则执行并落结果，拒绝则落 PermissionDenied。
    /// `remember_shape` 形如 "npm install *"，写入 permission_rules（票 11 消费）。
    /// `origin` 是裁决人（"owner"/"reviewer"），落进事件载荷——回放
    /// 「为什么这么放行」的唯一凭据（票 12 裁决人落账的前置）。
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        db: &Db,
        ctx: &ToolContext,
        question_id: &str,
        allow: bool,
        remember_shape: Option<&str>,
        scope: &str,
        pack: Option<&crate::orchestra::PackDef>,
        origin: &str,
    ) -> Result<CallOutcome, ToolError> {
        let card = crate::cards::get_queued(db, question_id, crate::cards::CardKind::Permission)
            .map_err(|e| match e {
                crate::cards::CardsError::NotQueued { state, .. } => {
                    ToolError::BadInput(format!("question {question_id} already {state}"))
                }
                _ => ToolError::UnknownQuestion(question_id.into()),
            })?;
        if card.project_id != ctx.project_id
            || card.agent_id.as_deref() != Some(ctx.agent_id.as_str())
        {
            return Err(ToolError::BadInput("permission owner mismatch".into()));
        }
        let payload = card.payload;
        let tool_name = payload["tool"].as_str().unwrap_or("").to_string();
        let raw_input = payload["raw_input"].clone();

        let action_id = match payload["action_id"].as_str() {
            Some(id) => id.to_string(),
            None => {
                // A still-queued legacy card proves authorization has not occurred.
                let action = crate::actions::prepare(
                    db,
                    ctx,
                    &tool_name,
                    &raw_input,
                    Some(&format!("legacy:{question_id}")),
                )?;
                crate::cards::annotate(db, question_id, &[("action_id", json!(action.id))])?;
                action.id
            }
        };
        let action = crate::actions::get(db, &ctx.project_id, &action_id)?;
        if action.agent_id != ctx.agent_id || action.tool != tool_name || action.input != raw_input
        {
            return Err(ToolError::BadInput("permission action mismatch".into()));
        }
        let mut origin_ctx = ctx.clone();
        origin_ctx.stage_run_id = action.stage_run_id;
        let ctx = &origin_ctx;
        // reliability 08: existing permission cards must not bypass another
        // unknown effect. Rejecting a card is harmless; approving is not.
        if allow {
            crate::actions::ensure_clear_except(db, ctx, &action_id)?;
        }
        let tx = db.conn().unchecked_transaction()?;
        crate::cards::answer(db, question_id, origin)?;

        if !allow {
            db.append_event(
                &ctx.project_id,
                EventKind::PermissionDenied,
                json!({ "tool": tool_name, "layer": origin, "question_id": question_id }),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            // 票 05：reviewer 判过 deny 的卡被负责人确认拒绝时，agent 只收到
            // 无信息量回执——reviewer 的具体理由永不进 agent 上下文，防
            // oracle-retry 试探（理由已进负责人卡 + trace）。
            let msg = if payload["reviewer_denied"].is_string() {
                crate::reviewer::AGENT_DENY_MESSAGE
            } else {
                "owner denied"
            };
            let reason = format!("(owner): {msg}");
            crate::actions::denied(db, ctx, &action_id, &reason)?;
            tx.commit()?;
            return Ok(CallOutcome::Denied(reason));
        }

        db.append_event(
            &ctx.project_id,
            EventKind::PermissionAllowed,
            json!({ "tool": tool_name, "question_id": question_id, "via": origin }),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        // 记形：显式 shape 或检验命令自动沉淀；安全网永不进记忆（persist_rule 内拦）
        if crate::permissions::persist_rule(
            db,
            ctx,
            &tool_name,
            &raw_input,
            remember_shape,
            scope,
            pack,
        )? {
            db.append_event(
                &ctx.project_id,
                EventKind::PermissionShapeRemembered,
                json!({ "tool": tool_name,
                        "shape": remember_shape.or(raw_input["cmd"].as_str()),
                        "scope": scope }),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
        }

        crate::actions::authorize(db, ctx, &action_id)?;
        tx.commit()?;
        self.exec_and_log(db, ctx, &tool_name, raw_input, None, &action_id, true)
            .map(CallOutcome::Done)
    }

    pub(crate) fn resume_action(
        &self,
        db: &Db,
        ctx: &ToolContext,
        id: &str,
    ) -> Result<CallOutcome, ToolError> {
        let action = crate::actions::get(db, &ctx.project_id, id)?;
        if action.agent_id != ctx.agent_id {
            return Err(ToolError::BadInput("action owner mismatch".into()));
        }
        if let Some(result) = crate::actions::replay(&action)? {
            return Ok(result);
        }
        crate::actions::ensure_clear_except(db, ctx, id)?;
        if action.state != "authorized" {
            return Err(ToolError::BadInput("action is not authorized".into()));
        }
        self.exec_and_log(db, ctx, &action.tool, action.input, None, id, true)
            .map(CallOutcome::Done)
    }

    pub(crate) fn tracked_nested(
        &self,
        db: &Db,
        ctx: &ToolContext,
        input: &Value,
        seq: Option<&str>,
        execute: impl FnOnce() -> Result<CallOutcome, ToolError>,
    ) -> Result<CallOutcome, ToolError> {
        // D09: nested dispatch bypasses exec_and_log; gate both its intent
        // and the actual dispatch after authorization/lease waits.
        crate::evaluation::control::checkpoint(&ctx.repo_root)
            .map_err(|e| ToolError::NotExecuted(e.to_string()))?;
        let action = crate::actions::prepare(db, ctx, "subagent", input, seq)?;
        crate::actions::ensure_clear_except(db, ctx, &action.id)?;
        if let Some(outcome) = crate::actions::replay(&action)? {
            return Ok(outcome);
        }
        db.append_event(&ctx.project_id,EventKind::ToolCalled,
            json!({"tool":"subagent","input":scrub_input("subagent",input),"seq":seq,"action_id":action.id}),
            Some(&ctx.agent_id),ctx.stage_run_id.as_deref())?;
        crate::actions::authorize(db, ctx, &action.id)?;
        #[cfg(test)]
        crate::actions::checkpoint(crate::actions::CrashPoint::Authorization);
        crate::actions::start(db, ctx, &action.id)?;
        let outcome = crate::evaluation::control::checkpoint(&ctx.repo_root)
            .map_err(|e| ToolError::NotExecuted(e.to_string()))
            .and_then(|()| execute());
        let result = match outcome {
            Ok(CallOutcome::Done(value)) => Ok(value),
            Ok(_) => Err(ToolError::Exec(
                "nested dispatch returned an unexpected permission state".into(),
            )),
            Err(e) => Err(e),
        };
        let uncertain = result
            .as_ref()
            .err()
            .is_some_and(|e| effect_uncertain(RiskClass::Exec, e));
        crate::actions::finish(db, ctx, &action.id, &result, uncertain)?;
        if uncertain {
            return Err(ToolError::OutcomeUnknown(action.id));
        }
        result.map(CallOutcome::Done)
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_and_log(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        input: Value,
        call_seq: Option<&str>,
        action_id: &str,
        allow_recovery: bool,
    ) -> Result<Value, ToolError> {
        // D09: recheck at the final side-effect boundary, including owner
        // permission resumes; a late model response cannot authorize new work.
        let _evaluation_work = crate::evaluation::control::work_lease(&ctx.repo_root)
            .map_err(|e| ToolError::NotExecuted(e.to_string()))?;
        let tool = self
            .tools
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::BadInput(format!("unknown tool: {name}")))?;
        // exec-cards 票 04：登记调用归属——bash 三形态 spawn/run_in
        // 时把输出口子绑进 Shared；执行完即摘，防非 bash 工具吃到
        // 陈旧 meta（口子只被 sessions 读，登记对它是无成本的）。
        ctx.sessions.set_call_meta(&ctx.agent_id, call_seq);
        let retry = crate::actions::is_retry(db, action_id)?;
        let allow_recovery = allow_recovery && !retry;
        let permission = crate::permissions::evaluate_logged(db, ctx, tool.as_ref(), name, &input)?;
        if !retry {
            if let crate::permissions::Decision::Deny { reason, .. } = permission {
                ctx.sessions.clear_call_meta();
                crate::actions::denied(db, ctx, action_id, &reason)?;
                return Err(ToolError::Exec(reason));
            }
        }
        #[cfg(test)]
        crate::actions::checkpoint(crate::actions::CrashPoint::Authorization);
        if retry {
            if !crate::actions::start_retry(
                db,
                ctx,
                action_id,
                tool.idempotency_contract(),
                matches!(permission, crate::permissions::Decision::Allow { .. }),
            )? {
                ctx.sessions.clear_call_meta();
                return Err(ToolError::OutcomeUnknown(action_id.into()));
            }
        } else {
            crate::actions::capture_retry_contract(db, action_id, tool.idempotency_contract())?;
            crate::actions::start(db, ctx, action_id)?;
        }
        let mut execution_ctx = ctx.clone();
        execution_ctx.action_key = Some(action_id.into());
        execution_ctx.native_effect = ctx
            .repo_root
            .join(".hexagon/evaluation-worker")
            .is_file()
            .then(|| Arc::new(std::sync::Mutex::new(None)));
        // Reliability 15: approval is bound to the original target snapshot.
        // Keep the host locks until execution returns; an empty fresh read ledger
        // on the control connection cannot waive the persisted precondition.
        let result = match writeguard::verify(db, &execution_ctx, tool.as_ref(), &input, action_id)
        {
            Ok(mut locks) => {
                execution_ctx.write_lease = locks.pop().map(Arc::new);
                crate::evaluation::control::checkpoint(&ctx.repo_root)
                    .map_err(|e| ToolError::NotExecuted(e.to_string()))
                    .and_then(|()| tool.exec(db, &input, &execution_ctx))
            }
            Err(error) => Err(error),
        };
        execution_ctx.write_lease.take();
        ctx.sessions.clear_call_meta();
        let uncertain = result
            .as_ref()
            .err()
            .is_some_and(|e| effect_uncertain(tool.risk(), e))
            || (tool.risk() != RiskClass::Read
                && result
                    .as_ref()
                    .is_ok_and(|v| v["timed_out"] == true || v["isError"] == true));
        // A failed retry proves nothing about the first uncertain attempt.
        // In particular NotExecuted here must not erase the original unknown.
        let uncertain = uncertain || (!allow_recovery && result.is_err());
        crate::actions::finish(db, &execution_ctx, action_id, &result, uncertain)?;
        if uncertain && !allow_recovery {
            return Err(ToolError::OutcomeUnknown(action_id.into()));
        }
        if uncertain {
            return self
                .reconcile_action(db, ctx, action_id)
                .and_then(|outcome| match outcome {
                    CallOutcome::Done(output) => Ok(output),
                    _ => Err(ToolError::OutcomeUnknown(action_id.into())),
                });
        }
        result
    }

    /// Recovery keeps the original identity; neither this API nor its default
    /// contract executes the uncertain operation again (D05 / ticket 11).
    pub fn reconcile_action(
        &self,
        db: &Db,
        ctx: &ToolContext,
        action_id: &str,
    ) -> Result<CallOutcome, ToolError> {
        let action = crate::actions::get(db, &ctx.project_id, action_id)?;
        if action.agent_id != ctx.agent_id {
            return Err(ToolError::BadInput("reconciliation owner mismatch".into()));
        }
        if crate::actions::has_resolution(db, action_id)? {
            return Err(ToolError::BadInput(
                "action already resolved by owner".into(),
            ));
        }
        if action.state != "unknown" {
            return crate::actions::replay(&action)?
                .ok_or_else(|| ToolError::BadInput("action is not unknown".into()));
        }
        let tool = self
            .get(&action.tool)
            .ok_or_else(|| ToolError::BadInput("reconciliation tool unavailable".into()))?;
        // Revocation also prevents querying an external service through an old
        // grant. False refusal costs owner intervention, not unauthorized IO.
        if !matches!(
            crate::permissions::evaluate_logged(
                db,
                ctx,
                tool.as_ref(),
                &action.tool,
                &action.input
            )?,
            crate::permissions::Decision::Allow { .. }
        ) {
            return Err(ToolError::OutcomeUnknown(action_id.into()));
        }
        let observed = tool
            .reconcile(db, &action.input, ctx, action_id)
            .unwrap_or_else(|_| Reconciliation::Unresolved {
                evidence: "read-only reconciliation failed".into(),
            });
        let result = crate::actions::record_reconciliation(db, ctx, action_id, observed);
        if matches!(result, Err(ToolError::OutcomeUnknown(_)))
            && crate::actions::authorize_idempotent_retry(
                db,
                ctx,
                action_id,
                tool.idempotency_contract(),
            )?
        {
            return self
                .exec_and_log(db, ctx, &action.tool, action.input, None, action_id, false)
                .map(CallOutcome::Done);
        }
        result
    }
}

// reliability 08: only structured validation errors prove no side effect;
// transport/IO/opaque execution failures after intent are conservatively unknown.
fn effect_uncertain(risk: RiskClass, error: &ToolError) -> bool {
    if risk == RiskClass::Read {
        return false;
    }
    match error {
        ToolError::BadInput(_)
        | ToolError::PathEscape(_)
        | ToolError::NotExecuted(_)
        | ToolError::WriteConflict { .. } => false,
        ToolError::Artifact(e) => !matches!(
            e.as_ref(),
            crate::artifacts::ArtifactError::MissingHeader
                | crate::artifacts::ArtifactError::MetadataConflict
                | crate::artifacts::ArtifactError::StaleReview
                | crate::artifacts::ArtifactError::MissingField(_)
                | crate::artifacts::ArtifactError::MissingSection(_)
        ),
        _ => true,
    }
}

// ---------- 子模块（arch-review 票 11 / D14 拆分）----------
// 护栏层与内建实现各成文件；`pub use` 再导出保 `crate::tools::X` 路径不变。
mod builtin;
pub(crate) mod effects;
pub mod readstate;
mod safety;
pub(crate) mod schema;
pub(crate) mod writeguard;
pub use builtin::*;
pub use safety::*;

#[cfg(test)]
mod tests;
