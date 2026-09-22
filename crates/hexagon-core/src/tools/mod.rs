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

#[derive(Debug, Clone)]
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
    /// 外部服务调用（mcp:*）：语义由第三方服务器自定，永远逐次必问，
    /// 不可被授权/层级/形状记忆降级——焊死的地板，不对称代价：
    /// 多问一次花一次点击，漏问一次是语义不明的副作用外发。
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
}

pub struct Registry {
    /// Arc 共享：只读视图（研究助手嵌套回合）按名拷 mcp:* 而不起新进程。
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Registry {
    pub fn builtin() -> Self {
        let mut r = Self {
            tools: HashMap::new(),
        };
        r.register(FsRead);
        r.register(Research);
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

    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    /// 只读视图（US36 研究助手嵌套回合）：fs_read/artifact_read + 共享 mcp:*
    /// —— 无写/bash/git/research → 结构性不可写、不可再派生；
    /// mcp:* 走 L0 授权闸门按调用方 agent_id 判 → 用不了父未授权服务。
    pub fn readonly(&self) -> Self {
        let mut r = Self {
            tools: HashMap::new(),
        };
        r.register(FsRead);
        r.register(ArtifactRead);
        r.register(LoadSkill);
        for (name, t) in &self.tools {
            if name.starts_with("mcp:") {
                r.tools.insert(name.clone(), t.clone());
            }
        }
        r
    }

    /// 供供应商请求用的工具清单（名字 + 描述 + schema）。
    pub fn defs(&self) -> Vec<crate::provider::ToolDef> {
        let mut v: Vec<_> = self
            .tools
            .values()
            .map(|t| crate::provider::ToolDef {
                name: t.name().into(),
                description: t.description().into(),
                input_schema: t.input_schema(),
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
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
            .get(name)
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
        match crate::permissions::evaluate_logged(db, ctx, tool.as_ref(), name, &input)? {
            crate::permissions::Decision::Deny { reason, layer } => {
                db.append_event(
                    &ctx.project_id,
                    EventKind::PermissionDenied,
                    json!({ "tool": name, "layer": layer, "reason": reason }),
                    Some(&ctx.agent_id),
                    ctx.stage_run_id.as_deref(),
                )?;
                Ok(CallOutcome::Denied(reason))
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
                if let crate::permissions::AllowVia::Remembered { shape, scope } = via {
                    db.append_event(
                        &ctx.project_id,
                        EventKind::PermissionAllowed,
                        json!({ "tool": name, "layer": "remembered", "shape": shape, "scope": scope }),
                        Some(&ctx.agent_id),
                        ctx.stage_run_id.as_deref(),
                    )?;
                }
                self.exec_and_log(db, ctx, name, input)
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
            return Ok(CallOutcome::Denied(msg.into()));
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

        self.exec_and_log(db, ctx, &tool_name, raw_input)
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
            return Ok(Some(CallOutcome::Denied(msg.into())));
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
        self.exec_and_log(db, ctx, &tool_name, raw_input)
            .map(|v| Some(CallOutcome::Done(v)))
    }

    fn exec_and_log(
        &self,
        db: &Db,
        ctx: &ToolContext,
        name: &str,
        input: Value,
    ) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::BadInput(format!("unknown tool: {name}")))?;
        let result = tool.exec(db, &input, ctx);
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
mod safety;
pub use builtin::*;
pub use safety::*;

#[cfg(test)]
mod tests;
