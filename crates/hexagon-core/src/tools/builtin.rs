//! 内建工具实现：Research/FsRead/FsWrite/FsPatch/Bash/ArtifactWrite/ArtifactRead/LoadSkill。

use super::*;

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
        // load_skill 同样吃全局静音（此前只看会话集——owner 关掉的技能
        // 仍能被点名加载，是漏口）。effective_muted = 会话∪"*"∪全局。
        let muted = crate::skills::effective_muted(&ctx.repo_root, &session);
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
