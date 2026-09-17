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

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError>;
    /// 求值占位：true = 默认转必问（安全网类操作）。票 11 换成五层管线。
    fn needs_ask(&self, _input: &Value, _ctx: &ToolContext) -> bool {
        false
    }
    /// 内置 deny 检查：Some(reason) = 拦下。最高优先，不可覆盖。
    fn builtin_deny(&self, _input: &Value, _ctx: &ToolContext) -> Option<String> {
        None
    }
}

pub struct Registry {
    tools: HashMap<String, Box<dyn Tool>>,
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
        r.register(FsWrite);
        r.register(FsPatch);
        r.register(Bash);
        r.register(ArtifactWrite);
        r.register(ArtifactRead);
        r.register(crate::git::GitBaselineMerge);
        r
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
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
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::BadInput(format!("unknown tool: {name}")))?;

        // 调用事件：敏感入参（fs_write 的 content 等）只记元信息。
        db.append_event(
            &ctx.project_id,
            EventKind::ToolCalled,
            json!({ "tool": name, "input": scrub_input(name, &input) }),
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
                let qid = format!("q{}", db.next_id("q")?);
                db.conn().execute(
                    "INSERT INTO pending_questions (id, project_id, agent_id, kind, payload)
                     VALUES (?1, ?2, ?3, 'permission', ?4)",
                    rusqlite::params![
                        qid,
                        ctx.project_id,
                        ctx.agent_id,
                        json!({ "tool": name, "input": scrub_input(name, &input),
                                "raw_input": input, "reason": reason, "safety_net": safety_net })
                        .to_string()
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
            "UPDATE pending_questions SET state='answered', answered_at=datetime('now') WHERE id=?1",
            [question_id],
        )?;

        if !allow {
            db.append_event(
                &ctx.project_id,
                EventKind::PermissionDenied,
                json!({ "tool": tool_name, "layer": "owner", "question_id": question_id }),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
            )?;
            return Ok(CallOutcome::Denied("owner denied".into()));
        }

        db.append_event(
            &ctx.project_id,
            EventKind::PermissionAllowed,
            json!({ "tool": tool_name, "question_id": question_id }),
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
fn scrub_input(tool: &str, input: &Value) -> Value {
    match tool {
        "fs_write" | "artifact_write" => json!({
            "path": input["path"],
            "bytes": input["content"].as_str().map(|s| s.len()).unwrap_or(0),
        }),
        "fs_patch" => json!({ "path": input["path"] }),
        _ => input.clone(),
    }
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
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let p = input["path"].as_str().unwrap_or("");
        is_credential_path(p).then(|| "credential content never enters context".into())
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        let bytes = std::fs::read(&p)?;
        if bytes.len() > FS_READ_CAP {
            return Ok(
                json!({"truncated": true, "content": String::from_utf8_lossy(&bytes[..FS_READ_CAP])}),
            );
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
        "Write a file inside the repo (ownership-gated)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let p = input["path"].as_str().unwrap_or("");
        if is_permission_rule_path(p) {
            return Some("agents cannot modify permission rules".into());
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
        "Replace an exact string in a repo file"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}},"required":["path","old","new"]})
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
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let cmd = input["cmd"].as_str().unwrap_or("");
        bash_hits_credentials(cmd).then(|| "command touches credential material".into())
    }
    // 占位求值：bash 永远走必问（安全网），票 11 接入规则形匹配。
    fn needs_ask(&self, _input: &Value, _ctx: &ToolContext) -> bool {
        true
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
            stdout.truncate(BASH_OUTPUT_CAP);
            stdout.push_str("…[truncated]");
        }
        if stderr.len() > BASH_OUTPUT_CAP {
            stderr.truncate(BASH_OUTPUT_CAP);
            stderr.push_str("…[truncated]");
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
        "Deliver an artifact under .hexagon/ (metadata header required for enforced tiers)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"kind":{"type":"string"}},"required":["path","content"]})
    }
    fn builtin_deny(&self, input: &Value, ctx: &ToolContext) -> Option<String> {
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
            .resolve(&db, &ctx, &qid, true, None, "activation", None)
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
        reg.resolve(&db, &ctx, &qid, false, None, "activation", None)
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
}
