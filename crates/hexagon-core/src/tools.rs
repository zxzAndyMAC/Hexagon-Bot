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
        r.register(ArtifactWrite);
        r.register(ArtifactRead);
        r.register(LoadSkill);
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
                let qid = format!("q{}", db.next_id("q")?);
                // 票 03：必问卡附溯源注记 + 激活冻结的 known world——
                // 负责人能看到「这文件是 agent N 步前写的」「这 remote 不在初始列表」
                let prov = crate::provenance::note(db, ctx, name, &input);
                let world = crate::provenance::known_world(
                    db,
                    &ctx.project_id,
                    ctx.stage_run_id.as_deref(),
                );
                let delta = crate::provenance::remote_delta(name, &input, world.as_ref());
                db.conn().execute(
                    "INSERT INTO pending_questions (id, project_id, agent_id, kind, payload, idem_key)
                     VALUES (?1, ?2, ?3, 'permission', ?4, ?5)",
                    rusqlite::params![
                        qid,
                        ctx.project_id,
                        ctx.agent_id,
                        json!({ "tool": name, "input": scrub_input(name, &input),
                                "raw_input": input, "reason": reason, "safety_net": safety_net,
                                "provenance": prov, "known_world": world,
                                "remote_delta": delta })
                        .to_string(),
                        idem_key,
                    ],
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
        let row = db
            .conn()
            .query_row(
                "SELECT payload, state FROM pending_questions WHERE id = ?1 AND kind = 'permission'",
                [question_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(|_| ToolError::UnknownQuestion(question_id.into()))?;
        if row.1 != "queued" {
            return Err(ToolError::BadInput(format!(
                "question {question_id} already {}",
                row.1
            )));
        }
        let payload: Value = serde_json::from_str(&row.0).unwrap_or_default();
        let tool_name = payload["tool"].as_str().unwrap_or("").to_string();
        let raw_input = payload["raw_input"].clone();

        db.conn().execute(
            "UPDATE pending_questions SET state='answered', answered_at=datetime('now'), answered_by=?2 WHERE id=?1",
            rusqlite::params![question_id, origin],
        )?;

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
        let row = db
            .conn()
            .query_row(
                "SELECT id, state, payload FROM pending_questions
                 WHERE agent_id=?1 AND idem_key=?2
                 AND kind='permission' ORDER BY id DESC LIMIT 1",
                rusqlite::params![ctx.agent_id, idem_key],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((qid, state, payload)) = row else {
            return Ok(None);
        };
        if state == "queued" {
            db.append_event(
                &ctx.project_id,
                EventKind::PermissionAsked,
                json!({ "question_id": qid, "deduped": true, "idem_key": idem_key }),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            return Ok(Some(CallOutcome::Asked(qid)));
        }
        if state != "answered" {
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
            let p: Value = serde_json::from_str(&payload).unwrap_or_default();
            let msg = if p["reviewer_denied"].is_string() {
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
        let p: Value = serde_json::from_str(&payload).unwrap_or_default();
        let tool_name = p["tool"].as_str().unwrap_or("").to_string();
        let raw_input = p["raw_input"].clone();
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

// ---------- 内置 deny 与路径安全 ----------

/// 凭据类路径：正文永远不进上下文（内置 deny，最高优先）。
fn is_credential_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    lower.contains("/.ssh/")
        || lower.contains("/.aws/")
        || lower.contains("keychain")
        || base.starts_with(".env")
        || base.ends_with(".pem")
        || base.ends_with(".key")
        || base.starts_with("id_rsa")
        || base.starts_with("id_ed25519")
        || base == "credentials"
        || base == "credentials.json"
        || base == "netrc"
        || base == ".netrc"
}

/// 权限规则文件：Agent 不得自改规则（内置 deny）。
fn is_permission_rule_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.contains("permission_rules") || lower.ends_with(".hexagon/permissions.toml")
}

/// agent 自授策略面（票 09）：skills 定义与本地会话偏好是负责人管理的
/// 提示词内容——写进 SKILL.md = 给自己追加指令，与 permission_rules 同类。
/// bash 侧不设防（bash 本就逐次必问/形状记忆，路径语义管不了）；
/// 这里的守卫只针对结构化写工具。
pub fn is_agent_policy_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.starts_with(".hexagon/skills/") || lower.starts_with(".hexagon/local/")
}

/// bash 命令里的凭据探测模式。
fn bash_hits_credentials(cmd: &str) -> bool {
    let c = cmd.to_lowercase();
    c.contains("security find-")
        || c.contains("security dump-")
        || c.contains("~/.ssh")
        || c.contains("$home/.ssh")
        || c.contains("cat .env")
        || c.contains("keychain")
        || is_credential_path(&c)
}

/// 事件载荷里不落 fs_write 正文等敏感字段。
/// FNV-1a 64：幂等键入参指纹（票 11）。选它而非 DefaultHasher——后者
/// 种子随版本/进程不定，幂等键要跨重启稳定（重放查重正是崩溃后场景）。
pub(crate) fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 参数递归脱敏（票 12，搬 audit.py::_sanitize_args）：键名含机密词
/// → `[redacted]`；body/content/html 及 `*_body` 等后缀 → `[redacted body]`
/// （消息体可能含机密）。不对称性：脱狠了丢一行审计细节，漏脱了密钥
/// 进持久存储——宁可脱狠。
fn redact(v: &Value) -> Value {
    const SECRET_KEYS: &[&str] = &[
        "token",
        "secret",
        "password",
        "api_key",
        "access_token",
        "bot_token",
        "app_token",
    ];
    const BODY_KEYS: &[&str] = &["body", "content", "html"];
    match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, val)| {
                let kl = k.to_lowercase();
                let redacted = if SECRET_KEYS.iter().any(|s| kl.contains(s)) {
                    json!("[redacted]")
                } else if BODY_KEYS
                    .iter()
                    .any(|b| kl == *b || kl.ends_with(&format!("_{b}")))
                {
                    json!("[redacted body]")
                } else {
                    redact(val)
                };
                (k.clone(), redacted)
            })
            .collect(),
        Value::Array(a) => a.iter().map(redact).collect(),
        _ => v.clone(),
    }
}

fn scrub_input(tool: &str, input: &Value) -> Value {
    match tool {
        "fs_write" | "artifact_write" => json!({
            "path": input["path"],
            "bytes": input["content"].as_str().map(|s| s.len()).unwrap_or(0),
        }),
        "fs_patch" => json!({ "path": input["path"] }),
        _ => redact(input),
    }
}

// ---------- 工具结果 spill（openworker-borrow 票 02）----------
//
// 出处：OpenWorker coworker/toolresult.py。超长结果完整落盘，上下文里只留
// head+marker+tail——纯 head 截断丢的恰是末尾的错误输出（事故教训：截断点
// 之后才是死因）。文件名取内容哈希：trim_context 每轮重跑同一结果幂等不重写，
// 且路径无时间戳、对 prompt cache 前缀稳定。模型要全文走 fs_read——
// spill 落在仓内正是为此。

/// spill 目录（仓内相对路径）。
pub const SPILL_DIR: &str = ".hexagon/spill";

/// FNV-1a 64：内容寻址文件名，无需引入哈希依赖。委托 fnv64——
/// 两份 FNV 实现曾并存（review 发现），单源防配方漂移。
fn fnv1a(s: &str) -> String {
    format!("{:016x}", fnv64(s))
}

/// 完整结果落 spill 文件，返回仓内相对路径。IO 失败 → None：
/// spill 是上下文优化不是安全门，失败退回旧式纯截断，不拖垮工具调用。
pub fn spill_result(ctx: &ToolContext, full: &str) -> Option<String> {
    let dir = ctx.repo_root.join(SPILL_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    ensure_gitignore(ctx);
    let base = fnv1a(full);
    for i in 0u32.. {
        let name = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let rel = format!("{SPILL_DIR}/{name}.txt");
        let p = ctx.repo_root.join(&rel);
        match std::fs::read(&p) {
            // 同内容同文件名 → 幂等命中，不重写
            Ok(existing) if existing == full.as_bytes() => return Some(rel),
            // 哈希碰撞：换名再来
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(&p, full).ok()?;
                return Some(rel);
            }
            Err(_) => return None,
        }
    }
    unreachable!()
}

/// `.hexagon/spill/` 是上下文缓存不是产物——进 .gitignore 防 `git add -A` 提交。
fn ensure_gitignore(ctx: &ToolContext) {
    const ENTRY: &str = ".hexagon/spill/";
    let p = ctx.repo_root.join(".gitignore");
    let cur = std::fs::read_to_string(&p).unwrap_or_default();
    if cur.lines().any(|l| l.trim() == ENTRY) {
        return;
    }
    let mut new = cur;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(ENTRY);
    new.push('\n');
    if let Err(e) = std::fs::write(&p, new) {
        log::warn!("gitignore update failed: {e}");
    }
}

/// head+marker+tail 三段式截断：完整内容 spill 落盘，显示段保留尾部
/// （错误输出通常在末尾）。cap 为显示段总预算。
pub fn spill_trim(ctx: &ToolContext, content: &str, cap: usize) -> String {
    if content.len() <= cap {
        return content.to_string();
    }
    let Some(path) = spill_result(ctx, content) else {
        let mut end = cap.min(content.len());
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        let mut s = content[..end].to_string();
        s.push_str(&format!("\n…[{} chars trimmed]", content.len() - end));
        return s;
    };
    // 预算分配：尾部 1/4（错误位），头部拿余量；marker 预留 96 字符。
    let tail_len = cap / 4;
    let head_len = cap.saturating_sub(tail_len + 96);
    let mut head_end = head_len.min(content.len());
    while !content.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = content.len() - tail_len.min(content.len());
    while !content.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    if tail_start < head_end {
        tail_start = head_end;
    }
    let cut = tail_start - head_end;
    let marker = format!("\n…[{cut} chars trimmed; full output: {path}]\n");
    format!(
        "{}{}{}",
        &content[..head_end],
        marker,
        &content[tail_start..]
    )
}

/// 仓内路径归一化：拒绝越出 repo_root。
pub fn repo_path(root: &Path, rel: &str) -> Result<PathBuf, ToolError> {
    let joined = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        root.join(rel)
    };
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other),
        }
    }
    // 已存在的文件按真实路径校验（挡符号链接逃逸）
    let check = if norm.exists() {
        norm.canonicalize().unwrap_or(norm.clone())
    } else {
        norm.clone()
    };
    let root_c = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !check.starts_with(&root_c) && !norm.starts_with(&root_c) && !norm.starts_with(root) {
        return Err(ToolError::PathEscape(rel.into()));
    }
    Ok(norm)
}

/// 极简 glob：`*` 匹配任意段内字符，`**` 跨段。
pub fn glob_match(pat: &str, path: &str) -> bool {
    fn inner(p: &[u8], s: &[u8]) -> bool {
        if p.is_empty() {
            return s.is_empty();
        }
        if p.starts_with(b"**/") {
            return inner(&p[3..], s) || (!s.is_empty() && inner(p, &s[1..]));
        }
        if p.starts_with(b"**") {
            return inner(&p[2..], s) || (!s.is_empty() && inner(p, &s[1..]));
        }
        match p[0] {
            b'*' => {
                (0..=s.len()).any(|i| s[..i].iter().all(|&c| c != b'/') && inner(&p[1..], &s[i..]))
            }
            b'?' => !s.is_empty() && s[0] != b'/' && inner(&p[1..], &s[1..]),
            c => !s.is_empty() && s[0] == c && inner(&p[1..], &s[1..]),
        }
    }
    inner(pat.as_bytes(), path.as_bytes())
}

fn str_arg<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input[key]
        .as_str()
        .ok_or_else(|| ToolError::BadInput(format!("missing string arg: {key}")))
}

// ---------- 内置工具 ----------

/// 研究助手入口（US36）：模型可见的 `research` 工具；实际执行由 turn 层
/// 截获跑嵌套只读回合（`research::call_nested`），exec 走到即未接截获的 bug。
pub struct Research;
impl Tool for Research {
    fn name(&self) -> &str {
        "research"
    }
    fn description(&self) -> &str {
        "spawn a read-only nested research pass: search/read/compare and return observations with citations. Cannot write files, run commands, or use git."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object",
               "properties": {"question": {"type": "string",
                 "description": "what to research"}},
               "required": ["question"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _db: &Db, _input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        Err(ToolError::Exec(
            "research is intercepted by the turn layer".into(),
        ))
    }
}

pub struct FsRead;
impl Tool for FsRead {
    fn name(&self) -> &str {
        "fs_read"
    }
    fn description(&self) -> &str {
        "Read a file inside the repo"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let p = input["path"].as_str().unwrap_or("");
        is_credential_path(p).then(|| "credential content never enters context".into())
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        let bytes = std::fs::read(&p)?;
        if bytes.len() > FS_READ_CAP {
            let full = String::from_utf8_lossy(&bytes).into_owned();
            let content = spill_trim(ctx, &full, FS_READ_CAP);
            return Ok(json!({"truncated": true, "content": content}));
        }
        Ok(json!({"content": String::from_utf8_lossy(&bytes)}))
    }
}

pub struct FsWrite;
impl Tool for FsWrite {
    fn name(&self) -> &str {
        "fs_write"
    }
    fn description(&self) -> &str {
        "Write a whole file inside the repo (ownership-gated). For partial edits prefer fs_patch — rewriting an entire file when only a few lines change doubles context cost (OpenWorker incident: 1812 full rewrites)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let p = input["path"].as_str().unwrap_or("");
        if is_permission_rule_path(p) {
            return Some("agents cannot modify permission rules".into());
        }
        if is_agent_policy_path(p) {
            return Some("skill/policy files are owner-managed".into());
        }
        is_credential_path(p).then(|| "credential files are not writable by agents".into())
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, str_arg(input, "content")?)?;
        Ok(json!({"written": p.strip_prefix(&ctx.repo_root).unwrap_or(&p)}))
    }
}

pub struct FsPatch;
impl Tool for FsPatch {
    fn name(&self) -> &str {
        "fs_patch"
    }
    fn description(&self) -> &str {
        "Replace an exact string in a repo file — the preferred way to make partial edits. `old` must match uniquely; read the file first if unsure"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}},"required":["path","old","new"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn builtin_deny(&self, input: &Value, ctx: &ToolContext) -> Option<String> {
        FsWrite.builtin_deny(input, ctx)
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        let old = str_arg(input, "old")?;
        let content = std::fs::read_to_string(&p)?;
        if !content.contains(old) {
            return Err(ToolError::BadInput("old string not found".into()));
        }
        std::fs::write(&p, content.replacen(old, str_arg(input, "new")?, 1))?;
        Ok(json!({"patched": str_arg(input, "path")?}))
    }
}

pub struct Bash;
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Run a shell command in the repo (always asks)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let cmd = input["cmd"].as_str().unwrap_or("");
        bash_hits_credentials(cmd).then(|| "command touches credential material".into())
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(str_arg(input, "cmd")?)
            .current_dir(&ctx.repo_root)
            .output()?;
        let mut stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        if stdout.len() > BASH_OUTPUT_CAP {
            stdout = spill_trim(ctx, &stdout, BASH_OUTPUT_CAP);
        }
        if stderr.len() > BASH_OUTPUT_CAP {
            stderr = spill_trim(ctx, &stderr, BASH_OUTPUT_CAP);
        }
        Ok(json!({
            "exit_code": out.status.code().unwrap_or(-1),
            "stdout": stdout,
            "stderr": stderr,
        }))
    }
}

/// 产物写入：走产物管道（元数据头校验 + `.hexagon/` 落盘 + 版本取代 + 登记）。
pub struct ArtifactWrite;
impl Tool for ArtifactWrite {
    fn name(&self) -> &str {
        "artifact_write"
    }
    fn description(&self) -> &str {
        "Deliver an artifact under .hexagon/ (metadata header required for enforced tiers). For partial edits prefer fs_patch over re-writing the whole artifact"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"kind":{"type":"string"}},"required":["path","content"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn builtin_deny(&self, input: &Value, ctx: &ToolContext) -> Option<String> {
        // 产物落在 .hexagon/<path>——守卫看的是生效路径，
        // "skills/x/SKILL.md" 不能借产物管道写进策略面（票 09）。
        let eff = format!(
            ".hexagon/{}",
            input["path"].as_str().unwrap_or("").trim_start_matches('/')
        );
        if is_agent_policy_path(&eff) {
            return Some("skill/policy files are owner-managed".into());
        }
        FsWrite.builtin_deny(input, ctx)
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let content = str_arg(input, "content")?;
        let id = crate::artifacts::deliver(
            db,
            ctx,
            &ctx.tiers,
            str_arg(input, "path")?,
            content,
            input["kind"].as_str(),
        )
        .map_err(Box::new)?;
        // 改进提案：交付即入提案队列（校验不过只拒提案不拒产物）
        if crate::artifacts::parse_header(content)
            .map(|(m, _)| m.kind.as_str() == "改进提案")
            .unwrap_or(false)
        {
            match crate::proposals::submit(db, ctx, &id, content) {
                Ok(pid) => return Ok(json!({"artifact_id": id, "proposal_id": pid})),
                Err(e) => {
                    log::info!("proposal submit rejected for {id}: {e}");
                    return Ok(json!({"artifact_id": id, "proposal_rejected": e.to_string()}));
                }
            }
        }
        Ok(json!({"artifact_id": id}))
    }
}

pub struct ArtifactRead;
impl Tool for ArtifactRead {
    fn name(&self) -> &str {
        "artifact_read"
    }
    fn description(&self) -> &str {
        "Read a registered artifact by path"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn builtin_deny(&self, input: &Value, ctx: &ToolContext) -> Option<String> {
        FsRead.builtin_deny(input, ctx)
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let rel = format!(
            ".hexagon/{}",
            str_arg(input, "path")?.trim_start_matches('/')
        );
        let p = repo_path(&ctx.repo_root, &rel)?;
        let bytes = std::fs::read(&p)?;
        Ok(json!({"content": String::from_utf8_lossy(&bytes[..bytes.len().min(FS_READ_CAP)])}))
    }
}

/// 技能按需加载（票 09）：catalog 在系统提示里是一行式指针，
/// 全文从这里取。每次调用重扫目录——会话中新建的技能也能取到。
pub struct LoadSkill;
impl Tool for LoadSkill {
    fn name(&self) -> &str {
        "load_skill"
    }
    fn description(&self) -> &str {
        "Load a skill's full instructions by name (see the skills catalog in your brief)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let name =
            crate::skills::validate_name(str_arg(input, "name")?).map_err(ToolError::BadInput)?;
        let mut loader = crate::skills::SkillLoader::new(crate::skills::skill_dirs(&ctx.repo_root));
        let session = ctx
            .stage_run_id
            .clone()
            .unwrap_or_else(|| ctx.agent_id.clone());
        let muted = crate::skills::SkillMutes::load(&ctx.repo_root).muted_set(&session);
        if loader.get(&name).is_none() {
            loader.rescan();
        }
        let skill = loader.get(&name).filter(|_| !muted.contains(&name));
        match skill {
            Some(s) => Ok(json!({
                "name": s.name,
                "instructions": s.instructions,
                "resources_path": s.path,
            })),
            None => Err(ToolError::BadInput(format!(
                "unknown skill: {name}; available: {:?}",
                loader.visible_names(&muted)
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (Db, Registry, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role) VALUES ('a1','p1','后端开发')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            project_id: "p1".into(),
            agent_id: "a1".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: crate::artifacts::TierMap::new(),
        };
        (db, Registry::builtin(), ctx, dir)
    }

    #[test]
    fn fs_read_write_roundtrip_within_repo() {
        let (db, reg, ctx, dir) = setup();
        let out = reg
            .call(
                &db,
                &ctx,
                "fs_write",
                json!({"path": "src/main.rs", "content": "fn main() {}"}),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Done(_)));
        assert!(dir.path().join("src/main.rs").exists());
        let out = reg
            .call(&db, &ctx, "fs_read", json!({"path": "src/main.rs"}))
            .unwrap();
        let CallOutcome::Done(v) = out else { panic!() };
        assert_eq!(v["content"], "fn main() {}");
        // 调用 + 结果事件都落了
        let items = db.timeline("p1", None, 50, None).unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
        assert!(kinds.contains(&EventKind::ToolCalled));
        assert!(kinds.contains(&EventKind::ToolResult));
    }

    #[test]
    fn fs_cannot_escape_repo_root() {
        let (db, reg, ctx, _dir) = setup();
        for evil in ["../outside.txt", "/etc/hostname", "a/../../b.txt"] {
            let out = reg.call(&db, &ctx, "fs_write", json!({"path": evil, "content": "x"}));
            match out {
                Err(ToolError::PathEscape(_)) => {}
                other => panic!("{evil} should escape-deny, got {other:?}"),
            }
        }
    }

    #[test]
    fn credential_reads_denied_with_event() {
        let (db, reg, ctx, _dir) = setup();
        for p in [".env", "id_rsa", "certs/server.pem", ".aws/credentials"] {
            let out = reg.call(&db, &ctx, "fs_read", json!({"path": p})).unwrap();
            assert!(matches!(out, CallOutcome::Denied(_)), "{p}");
        }
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::PermissionDenied]))
            .unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].event.payload["layer"], "builtin_deny");
    }

    #[test]
    fn agent_cannot_write_permission_rules() {
        let (db, reg, ctx, _dir) = setup();
        let out = reg
            .call(
                &db,
                &ctx,
                "fs_write",
                json!({"path": ".hexagon/permissions.toml", "content": "allow = *"}),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Denied(_)));
    }

    #[test]
    fn bash_credential_probe_denied() {
        let (db, reg, ctx, _dir) = setup();
        let out = reg
            .call(
                &db,
                &ctx,
                "bash",
                json!({"cmd": "security find-generic-password -a x"}),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Denied(_)));
    }

    #[test]
    fn bash_asks_then_executes_on_allow() {
        let (db, reg, ctx, dir) = setup();
        let out = reg
            .call(&db, &ctx, "bash", json!({"cmd": "echo hi > out.txt"}))
            .unwrap();
        let CallOutcome::Asked(qid) = out else {
            panic!()
        };
        // 未执行
        assert!(!dir.path().join("out.txt").exists());
        let out = reg
            .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
            .unwrap();
        let CallOutcome::Done(v) = out else { panic!() };
        assert_eq!(v["exit_code"], 0);
        assert!(dir.path().join("out.txt").exists());
        let items = db.timeline("p1", None, 50, None).unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.event.kind).collect();
        for k in [
            EventKind::ToolCalled,
            EventKind::PermissionAsked,
            EventKind::PermissionAllowed,
            EventKind::ToolResult,
        ] {
            assert!(kinds.contains(&k), "missing {k:?}");
        }
    }

    #[test]
    fn denied_question_does_not_execute() {
        let (db, reg, ctx, dir) = setup();
        let CallOutcome::Asked(qid) = reg
            .call(&db, &ctx, "bash", json!({"cmd": "touch nope"}))
            .unwrap()
        else {
            panic!()
        };
        reg.resolve(&db, &ctx, &qid, false, None, "activation", None, "owner")
            .unwrap();
        assert!(!dir.path().join("nope").exists());
    }

    #[test]
    fn remember_shape_writes_rule() {
        let (db, reg, ctx, _dir) = setup();
        let CallOutcome::Asked(qid) = reg
            .call(&db, &ctx, "bash", json!({"cmd": "npm install zod"}))
            .unwrap()
        else {
            panic!()
        };
        reg.resolve(
            &db,
            &ctx,
            &qid,
            true,
            Some("npm install *"),
            "project",
            None,
            "owner",
        )
        .unwrap();
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM permission_rules WHERE shape='npm install *' AND scope='project'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn ownership_glob_gates_writes() {
        let (db, reg, mut ctx, _dir) = setup();
        ctx.owned_globs = vec!["src/**".into()];
        // 界内自动放行
        let out = reg
            .call(
                &db,
                &ctx,
                "fs_write",
                json!({"path": "src/a.rs", "content": "x"}),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Done(_)));
        // 界外转必问
        let out = reg
            .call(
                &db,
                &ctx,
                "fs_write",
                json!({"path": "docs/b.md", "content": "x"}),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Asked(_)));
    }

    // ---------- openworker-borrow 票 02：工具结果 spill ----------

    #[test]
    fn bash_oversize_output_spills_head_marker_tail() {
        let (db, _reg, ctx, dir) = setup();
        // stdout 超 BASH_OUTPUT_CAP：直接调 exec 贴近断言面
        // （权限管线已在 bash_asks_then_executes_on_allow 覆盖）
        let head = "H".repeat(BASH_OUTPUT_CAP);
        let tail = "TAIL_ERROR_MARKER";
        let body = format!("{head}{}{tail}", "x".repeat(1024));
        let v = Bash
            .exec(
                &db,
                &json!({"cmd": format!("printf '%s' '{}'", body)}),
                &ctx,
            )
            .unwrap();
        let out = v["stdout"].as_str().unwrap();
        assert!(out.contains("full output: .hexagon/spill/"), "{out:?}");
        assert!(out.starts_with("HHH"), "head kept");
        assert!(out.ends_with(tail), "tail kept: {}", &out[out.len() - 80..]);
        // spill 文件可经 fs_read 读回完整内容
        let spill_rel = out
            .split("full output: ")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap()
            .trim();
        let full = std::fs::read_to_string(dir.path().join(spill_rel)).unwrap();
        assert_eq!(full, body);
        // .gitignore 已登记
        assert!(std::fs::read_to_string(dir.path().join(".gitignore"))
            .unwrap()
            .contains(".hexagon/spill/"));
    }

    #[test]
    fn fs_read_oversize_spills_and_is_idempotent() {
        let (_db, _reg, ctx, dir) = setup();
        let big = "a".repeat(FS_READ_CAP + 4096) + "END";
        std::fs::write(dir.path().join("big.txt"), &big).unwrap();
        let v = FsRead
            .exec(&_db, &json!({"path": "big.txt"}), &ctx)
            .unwrap();
        assert_eq!(v["truncated"], true);
        let content = v["content"].as_str().unwrap();
        assert!(content.contains("full output:"));
        assert!(content.ends_with("END"), "tail preserved");
        // 幂等：同内容再 spill 不产生第二个文件
        let again = spill_result(&ctx, &big).unwrap();
        assert!(dir.path().join(&again).exists());
        let n = std::fs::read_dir(dir.path().join(SPILL_DIR))
            .unwrap()
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn write_content_not_logged_in_events() {
        let (db, reg, ctx, _dir) = setup();
        reg.call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": "a.txt", "content": "SUPERSECRET"}),
        )
        .unwrap();
        let items = db.timeline("p1", None, 50, None).unwrap();
        for i in &items {
            assert!(!i.event.payload.to_string().contains("SUPERSECRET"));
        }
        // 但文件本身写进去了
        assert_eq!(
            fs::read_to_string(_dir.path().join("a.txt")).unwrap(),
            "SUPERSECRET"
        );
    }

    // ---- 票 11：必问卡幂等键 ----

    #[test]
    fn queued_question_reused_on_same_call_seq() {
        let (db, reg, ctx, _d) = setup();
        let inp = || json!({"cmd": "rm -rf build"});
        let o1 = reg
            .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i0"))
            .unwrap();
        let CallOutcome::Asked(q1) = o1 else { panic!() };
        // 同 seq+同参重放 → 同一张卡，队列不增
        let o2 = reg
            .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i0"))
            .unwrap();
        assert!(matches!(o2, CallOutcome::Asked(ref q) if *q == q1));
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM pending_questions WHERE kind='permission'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // deduped 事件留痕
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::PermissionAsked]))
            .unwrap();
        assert_eq!(
            items[1].event.payload["deduped"], true,
            "第二次命中应有 deduped 标记"
        );
        // 不同 seq 或不同参 → 各自独立新卡
        let o3 = reg
            .call_with_seq(&db, &ctx, "bash", inp(), Some("r0i1"))
            .unwrap();
        assert!(matches!(o3, CallOutcome::Asked(ref q) if *q != q1));
        let o4 = reg
            .call_with_seq(&db, &ctx, "bash", json!({"cmd":"ls"}), Some("r0i0"))
            .unwrap();
        assert!(matches!(o4, CallOutcome::Asked(ref q) if *q != q1));
    }

    #[test]
    fn answered_deny_replays_as_denied_without_new_card() {
        let (db, reg, ctx, _d) = setup();
        let CallOutcome::Asked(qid) = reg
            .call_with_seq(&db, &ctx, "bash", json!({"cmd":"rm -rf x"}), Some("r0i0"))
            .unwrap()
        else {
            panic!()
        };
        reg.resolve(&db, &ctx, &qid, false, None, "activation", None, "owner")
            .unwrap();
        // 重放同调用 → 沿用 deny 裁决，不弹新卡；answered_by 落账
        let out = reg
            .call_with_seq(&db, &ctx, "bash", json!({"cmd":"rm -rf x"}), Some("r0i0"))
            .unwrap();
        assert!(matches!(out, CallOutcome::Denied(ref r) if r == "owner denied"));
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM pending_questions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let by: String = db
            .conn()
            .query_row(
                "SELECT answered_by FROM pending_questions WHERE id=?1",
                [&qid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(by, "owner");
    }

    #[test]
    fn answered_allow_with_result_replays_as_done_marker() {
        let (db, reg, ctx, _d) = setup();
        let CallOutcome::Asked(qid) = reg
            .call_with_seq(&db, &ctx, "bash", json!({"cmd":"echo hi"}), Some("r0i0"))
            .unwrap()
        else {
            panic!()
        };
        // owner 批准 → resolve 内已执行
        let out = reg
            .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
            .unwrap();
        assert!(matches!(out, CallOutcome::Done(_)));
        // 重放 → Done 标记，不再执行（副作用不双跑）
        let out = reg
            .call_with_seq(&db, &ctx, "bash", json!({"cmd":"echo hi"}), Some("r0i0"))
            .unwrap();
        let CallOutcome::Done(v) = out else { panic!() };
        assert_eq!(v["duplicate_of"], qid);
        let execs: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='tool_result' AND json_extract(payload,'$.tool')='bash'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(execs, 1, "副作用只跑一次");
    }

    // ---- 票 12：审计脱敏 ----

    #[test]
    fn secret_keys_redacted_in_trace_payload() {
        let (db, reg, ctx, _d) = setup();
        // bash 走 Ask → 卡；入参里带机密字段
        let _ = reg.call(
            &db,
            &ctx,
            "bash",
            json!({"cmd":"x","api_key":"AKIA123","nested":{"password":"p@ss","note":"ok"},
                   "reply_body":"<机密正文>","safe":"visible"}),
        );
        let items = db
            .timeline("p1", None, 50, Some(&[EventKind::ToolCalled]))
            .unwrap();
        let inp = &items[0].event.payload["input"];
        assert_eq!(inp["api_key"], "[redacted]");
        assert_eq!(inp["nested"]["password"], "[redacted]");
        assert_eq!(inp["nested"]["note"], "ok");
        assert_eq!(inp["reply_body"], "[redacted body]");
        assert_eq!(inp["safe"], "visible");
        // 整个事件文本里不能出现机密值
        let raw = items[0].event.payload.to_string();
        assert!(!raw.contains("AKIA123"));
        assert!(!raw.contains("p@ss"));
        assert!(!raw.contains("机密正文"));
    }

    #[test]
    fn oversized_event_payload_spills_to_file() {
        let (db, _reg, _ctx, dir) = setup();
        db.conn()
            .execute(
                "UPDATE projects SET dir=?1 WHERE id='p1'",
                [dir.path().to_str().unwrap()],
            )
            .unwrap();
        let big = "x".repeat(200 * 1024);
        db.append_event(
            "p1",
            EventKind::System,
            json!({"kind":"big","blob": big}),
            Some("a1"),
            None,
        )
        .unwrap();
        let (payload,): (String,) = db
            .conn()
            .query_row(
                "SELECT payload FROM events WHERE kind='system' AND json_extract(payload,'$.kind') IS NULL",
                [],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap_or_else(|_| {
                db.conn()
                    .query_row(
                        "SELECT payload FROM events ORDER BY id DESC LIMIT 1",
                        [],
                        |r| Ok((r.get(0)?,)),
                    )
                    .unwrap()
            });
        let p: Value = serde_json::from_str(&payload).unwrap();
        assert!(p["spilled"].is_string(), "超限载荷应落 spill 指针: {p}");
        let spilled_path = p["spilled"].as_str().unwrap().to_string();
        let full = std::fs::read_to_string(&spilled_path).unwrap();
        assert!(full.contains(&"x".repeat(1000)), "spill 文件存全量");
        assert!(std::path::Path::new(&spilled_path)
            .to_string_lossy()
            .contains(".hexagon/spill"));
    }

    // ---- rsi-research 票 05：裁决器不对称不变量 ----

    #[test]
    fn allow_once_does_not_persist_next_call_asks_again() {
        // 授权一次性：批准一次（不记形）只执行那一发；同形状新调用再弹卡。
        // （idem 复用只担保「同一调用重放不双弹」,不同 call_seq = 新调用。）
        let (db, reg, ctx, dir) = setup();
        let CallOutcome::Asked(q1) = reg
            .call(&db, &ctx, "bash", json!({"cmd": "touch once1"}))
            .unwrap()
        else {
            panic!()
        };
        reg.resolve(&db, &ctx, &q1, true, None, "activation", None, "owner")
            .unwrap();
        assert!(dir.path().join("once1").exists());
        // 无规则沉淀
        let n: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM permission_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        // 新 seq 的同形状调用 → 再必问
        let out = reg
            .call_with_seq(
                &db,
                &ctx,
                "bash",
                json!({"cmd": "touch once2"}),
                Some("r7:i0"),
            )
            .unwrap();
        assert!(matches!(out, CallOutcome::Asked(_)));
        assert!(!dir.path().join("once2").exists());
    }

    #[test]
    fn safety_net_ask_survives_any_memory() {
        // 安全网永不进记忆且永远必问——即使规则表被手工塞进匹配 allow。
        let (db, reg, ctx, _dir) = setup();
        db.conn()
            .execute(
                "INSERT INTO permission_rules (id, project_id, tool, shape, effect, scope)
                 VALUES ('prX','p1','bash','git push *','allow','project')",
                [],
            )
            .unwrap();
        let out = reg
            .call(&db, &ctx, "bash", json!({"cmd": "git push origin main"}))
            .unwrap();
        assert!(matches!(out, CallOutcome::Asked(_)));
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='permission_allowed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }
}
