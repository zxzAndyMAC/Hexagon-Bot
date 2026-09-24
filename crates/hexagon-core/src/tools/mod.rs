//! 工具层：一切 Agent 动作的单一管线。
//!
//! 注册表 → 内置 deny（最高优先，不可覆盖）→ 权限求值（本票为占位：
//! 默认 ask，完整五层在票 11）→ 执行。调用与结果全部落轨迹事件。
//! MCP 工具在票 13 汇入同一管线。

use crate::db::Db;
use crate::trace::{EventKind, TraceError};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const BASH_OUTPUT_CAP: usize = 64 * 1024;
pub const FS_READ_CAP: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
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
}

#[derive(Clone)]
pub struct ToolContext {
    pub project_id: String,
    pub agent_id: String,
    pub repo_root: PathBuf,
    pub stage_run_id: Option<String>,
    /// 路径归属 glob：非空时 fs 写入必须命中其一，否则转必问。
    /// 由编排内核按阶段注入；空 = 未限定（开发早期）。
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

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数命名约定（票 14 备忘）：参数名别撞宿主模板/语言方法名——
    /// OpenWorker 真实事故：todo 工具参数叫 `items`，minijinja 把它
    /// 解析成 dict.items() 方法调用，直接 400。`items`/`keys`/`values`/
    /// `get`/`update` 这类名字禁用；宁可 `entries`/`todo_list`。
    fn input_schema(&self) -> Value;
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError>;
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
    /// 失败的调用不弹卡。负责人批准后的执行不复查（见 readstate 模块头）。
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
    /// 仓内搜索 + web 摘要 + 测试执行（自带硬闸）+ `pick` 点名的 mcp:*。
    /// 没有写/bash/git/web_fetch/子代理入口 → 结构性不可写、不可再派生、
    /// 不可外带。`pick` 之外的 mcp 工具连名字都不可见（defs 不列出）——
    /// 授权闸门是第二道，第一道是注册表本身。
    pub fn subagent_scope(&self, pick: &std::collections::HashSet<String>) -> Self {
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
        let guard = self.tools.lock().unwrap();
        for (name, t) in guard.iter() {
            if name.starts_with("mcp:") && pick.contains(name) {
                r.tools.lock().unwrap().insert(name.clone(), t.clone());
            }
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
        let tool = self
            .tools
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::BadInput(format!("unknown tool: {name}")))?;

        // 调用事件：敏感入参（fs_write 的 content 等）只记元信息。
        // seq 入载荷（票 11）：中断重放时同一调用可被指认。
        db.append_event(
            &ctx.project_id,
            EventKind::ToolCalled,
            json!({ "tool": name, "input": scrub_input(name, &input), "seq": call_seq }),
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
                // 票 11 幂等键：(激活,位置序,工具,入参指纹)。中断重放同一调用
                // 命中既有卡——queued 复用不弹新卡；answered 沿用裁决。
                let idem_key = call_seq.map(|seq| {
                    format!(
                        "{}:{seq}:{name}:{:016x}",
                        ctx.stage_run_id.as_deref().unwrap_or("-"),
                        fnv64(&input.to_string())
                    )
                });
                if let Some(idem) = &idem_key {
                    if let Some(outcome) = self.idem_reuse(db, ctx, idem)? {
                        return Ok(outcome);
                    }
                }
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
                let qid = crate::cards::enqueue(
                    db,
                    &ctx.project_id,
                    Some(&ctx.agent_id),
                    crate::cards::CardKind::Permission,
                    json!({ "tool": name, "input": scrub_input(name, &input),
                            "raw_input": input, "reason": reason, "safety_net": safety_net,
                            "provenance": prov, "known_world": world,
                            "remote_delta": delta }),
                    idem_key.as_deref(),
                )?;
                db.append_event(
                    &ctx.project_id,
                    EventKind::PermissionAsked,
                    json!({ "tool": name, "question_id": qid, "reason": reason, "safety_net": safety_net }),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                )?;
                Ok(CallOutcome::Asked(qid))
            }
            crate::permissions::Decision::Allow { via } => {
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
                self.exec_and_log(db, ctx, name, input, call_seq)
                    .map(CallOutcome::Done)
            }
        }
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
        let payload = card.payload;
        let tool_name = payload["tool"].as_str().unwrap_or("").to_string();
        let raw_input = payload["raw_input"].clone();

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
            return Ok(CallOutcome::Denied(format!("(owner): {msg}")));
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

        // 票 04：resolve 路径 seq=None（原 seq 在 idem_key 里不拆，
        // UI 回退最新在途匹配）。
        self.exec_and_log(db, ctx, &tool_name, raw_input, None)
            .map(CallOutcome::Done)
    }

    /// 票 11：同幂等键的既有卡复用（对照 OpenWorker `inbox.for_tool_call`）。
    /// 返回 Some = 本次调用已按既有卡处理；None = 无命中，走新卡。
    /// - queued   → 复用同一张卡，不重复弹（带 deduped 标记事件留痕）
    /// - answered → 沿用裁决：allow 看执行痕迹（执行过→Done 标记，崩在
    ///   允许后执行前的窄窗→此刻补执行）；deny → 同样的无信息量回执
    /// - expired  → 不算命中，走新卡
    ///
    /// 不对称性：漏查重 = 同一动作弹两次卡；错沿用 = 跳过一次人工。
    /// 故指纹含完整 canonical 入参——同名不同参绝不共享一张卡。
    fn idem_reuse(
        &self,
        db: &Db,
        ctx: &ToolContext,
        idem_key: &str,
    ) -> Result<Option<CallOutcome>, ToolError> {
        // 卡表读口归 cards.rs（票 04）；命中后的裁决沿用逻辑留在这里——
        // 它要查 events（trace 域）并可能补执行（registry 域），都不归 cards。
        let Some(card) = crate::cards::find_by_idem(db, &ctx.agent_id, idem_key)? else {
            return Ok(None);
        };
        let (qid, state, payload) = (card.id, card.state, card.payload);
        if state == crate::cards::CardState::Queued {
            db.append_event(
                &ctx.project_id,
                EventKind::PermissionAsked,
                json!({ "question_id": qid, "deduped": true, "idem_key": idem_key }),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            return Ok(Some(CallOutcome::Asked(qid)));
        }
        if state != crate::cards::CardState::Answered {
            return Ok(None);
        }
        // 已答：找裁决事件（allow 记 PermissionAllowed，deny 记 PermissionDenied）
        let verdict = db
            .conn()
            .query_row(
                "SELECT id, kind FROM events
                 WHERE project_id=?1 AND kind IN ('permission_allowed','permission_denied')
                 AND json_extract(payload,'$.question_id')=?2 ORDER BY id DESC LIMIT 1",
                rusqlite::params![ctx.project_id, qid],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((eid, kind)) = verdict else {
            return Ok(None);
        };
        if kind == "permission_denied" {
            let msg = if payload["reviewer_denied"].is_string() {
                crate::reviewer::AGENT_DENY_MESSAGE
            } else {
                "owner denied"
            };
            return Ok(Some(CallOutcome::Denied(format!("(owner): {msg}"))));
        }
        // allowed：执行过吗？allow 事件之后同 agent 有 tool_result → 已执行
        let ran: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id=?1 AND agent_id=?2
                 AND kind='tool_result' AND id > ?3",
                rusqlite::params![ctx.project_id, ctx.agent_id, eid],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);
        if ran {
            return Ok(Some(CallOutcome::Done(json!({
                "duplicate_of": qid,
                "note": "该调用已批准并执行过；结果见 trace，不要重复执行"
            }))));
        }
        // 允许后崩在 exec 前：负责人已同意，此刻补执行（不再弹卡）
        let tool_name = payload["tool"].as_str().unwrap_or("").to_string();
        let raw_input = payload["raw_input"].clone();
        // 票 04：resolve 路径 seq=None——执行发生在负责人裁决后，
        // 原调用的 seq 沉在 idem_key 里不拆；UI 回退按该 agent 最新
        // 在途 bash 匹配（票 04 DTO 注释）。
        self.exec_and_log(db, ctx, &tool_name, raw_input, None)
            .map(|v| Some(CallOutcome::Done(v)))
    }

    fn exec_and_log(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        input: Value,
        call_seq: Option<&str>,
    ) -> Result<Value, ToolError> {
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
        let result = tool.exec(db, &input, ctx);
        ctx.sessions.clear_call_meta();
        let (ok, payload) = match &result {
            Ok(v) => (true, json!({ "tool": name, "output": v })),
            Err(e) => (false, json!({ "tool": name, "error": e.to_string() })),
        };
        db.append_event(
            &ctx.project_id,
            EventKind::ToolResult,
            json!({ "tool": name, "ok": ok, "result": payload }),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        result
    }
}

// ---------- 子模块（arch-review 票 11 / D14 拆分）----------
// 护栏层与内建实现各成文件；`pub use` 再导出保 `crate::tools::X` 路径不变。
mod builtin;
pub mod readstate;
mod safety;
pub(crate) mod schema;
pub use builtin::*;
pub use safety::*;

#[cfg(test)]
mod tests;
