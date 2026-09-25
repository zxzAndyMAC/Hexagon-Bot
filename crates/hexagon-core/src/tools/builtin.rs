//! 内建工具实现：Research/FsRead/FsWrite/FsPatch/Bash/ArtifactWrite/ArtifactRead/LoadSkill。

use super::*;

// ---------- 内置工具 ----------

/// 读图上限（票 02）：>5MB 按原文本路径处理并注明，不塞图。
const FS_IMAGE_CAP: usize = 5 * 1024 * 1024;

/// 魔数嗅探（票 02/03 共用）：只认字节不认名——附件上传与 fs_read
/// 双判共用同一真相源，浏览器声明的 MIME 永远只是参考。
pub(crate) fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// 图片类型双判（票 02）：扩展名 ∧ 魔数同时吻合才认图——伪造扩展名
/// 的任意字节走文本路径（单看后缀是注入面：把二进制伪装 .png 会让
/// 垃圾字节以 image 块进上下文）。
fn image_media_type(path: &std::path::Path, bytes: &[u8]) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let wanted = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    (sniff_image(bytes) == Some(wanted)).then_some(wanted)
}

/// 票 03 附件上限：单图 ≤5MB、单条消息 ≤4 图（与 UI 拦截同一口径）。
pub(crate) const ATTACH_IMG_CAP: usize = FS_IMAGE_CAP;
pub(crate) const ATTACH_MAX_COUNT: usize = 4;

pub struct FsRead;
impl Tool for FsRead {
    fn name(&self) -> &str {
        "fs_read"
    }
    fn description(&self) -> &str {
        r#"Read a file in the repo. Returns its text; png/jpg/gif/webp files whose magic bytes match come back as an image (vision-capable model slots only).
- Use when: you need a file's contents, including before any edit — fs_patch and fs_write reject edits to files you have not read in this activation.
- Do not use: to find files (fs_find), to search contents (fs_grep, sem_search), or for delivered artifacts under .hexagon/ (artifact_read).
- `offset` / `limit` select a 1-based line range; the result reports `total_lines` and the `lines` you got. Use them for large files and for the spill files that trimmed results point to.
- Errors: a missing path returns an I/O error; credential files are always denied; an offset past the end reports the file's line count."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string","description":"repo-relative file path"},
            "offset":{"type":"integer","minimum":1,"description":"first line to read, 1-based"},
            "limit":{"type":"integer","minimum":1,"description":"number of lines to read"}},
            "required":["path"]})
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
        // 指纹取在读之前：读与取指纹之间若有人改文件，指纹是旧的，下次编辑
        // 判「读后已变」——偏差落在误判一侧（readstate 代价模型）。
        let before = readstate::stamp(&p);
        let bytes = std::fs::read(&p)?;
        // 票 02（prompt-engineering）：父代理读过才可改。子代理不能写盘，
        // 它的读不代父记账——父 ctx 与子代理共享 Arc，记了就等于代读。
        if ctx.subagent.is_none() {
            if let Some(s) = before {
                ctx.reads.record_stamp(&p, s);
            }
        }
        // 票 04：子代理域记实读路径——交回的引用以这格为准（实际读过的，
        // 不是模型自称读过什么）。
        if let Some(s) = &ctx.subagent {
            if let Ok(rel) = p.strip_prefix(&ctx.repo_root) {
                s.reads
                    .lock()
                    .unwrap()
                    .push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        // 票 02 读图路径：双判认图 → caps 闸门 → 尺寸闸 → image 载荷。
        // 非 vision 槽字节不进上下文（fail-closed，ADR 0058-2）。
        if let Some(mt) = image_media_type(&p, &bytes) {
            let rel = p.strip_prefix(&ctx.repo_root).unwrap_or(&p);
            if !ctx.caps.contains("vision") {
                return Ok(json!({
                    "path": rel, "bytes": bytes.len(),
                    "note": "model slot lacks vision — image bytes not loaded into context"
                }));
            }
            if bytes.len() <= FS_IMAGE_CAP {
                use base64::Engine;
                return Ok(json!({
                    "path": rel, "bytes": bytes.len(),
                    "image": {"media_type": mt,
                              "data": base64::engine::general_purpose::STANDARD.encode(&bytes)}
                }));
            }
            // >5MB：落回文本路径并注明（字节量对上下文是灾难不是信息）
            let full = String::from_utf8_lossy(&bytes).into_owned();
            let content = spill_trim(ctx, &full, FS_READ_CAP);
            return Ok(json!({
                "truncated": true, "content": content,
                "note": "image exceeds 5MB — returned as text, not sent as image"
            }));
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let total = lines.len();
        let offset = input["offset"].as_u64();
        let limit = input["limit"].as_u64();
        // 区间读（prompt-engineering 票 01）：spill 文件与大文件的中段靠它
        // 读回——之前没有区间参数，超 cap 的部分只能看头尾。
        if offset.is_some() || limit.is_some() {
            let start = offset.unwrap_or(1).max(1) as usize;
            if start > total.max(1) {
                return Err(ToolError::BadInput(format!(
                    "offset {start} is past the end: file has {total} lines"
                )));
            }
            // saturating：schema 不设 limit 上限，u64::MAX 曾让加法溢出后切片越界 panic。
            let end = limit
                .map(|l| {
                    (start - 1)
                        .saturating_add(usize::try_from(l.max(1)).unwrap_or(usize::MAX))
                        .min(total)
                })
                .unwrap_or(total);
            let slice: String = lines[start - 1..end].concat();
            let truncated = slice.len() > FS_READ_CAP;
            let content = spill_trim(ctx, &slice, FS_READ_CAP);
            return Ok(json!({
                "content": content, "total_lines": total,
                "lines": [start, end], "truncated": truncated,
            }));
        }
        if bytes.len() > FS_READ_CAP {
            let content = spill_trim(ctx, &text, FS_READ_CAP);
            return Ok(json!({"truncated": true, "content": content, "total_lines": total}));
        }
        Ok(json!({"content": text, "total_lines": total}))
    }
}

/// 先读后改（票 02，判定见 `readstate`）：拒绝记 Warn 级诊断。
fn require_fresh_read(ctx: &ToolContext, rel: &str) -> Result<(), ToolError> {
    let started = std::time::Instant::now();
    let p = repo_path(&ctx.repo_root, rel)?;
    let verdict = ctx.reads.check(&p);
    let msg = match verdict {
        readstate::Verdict::Ok => return Ok(()),
        readstate::Verdict::NotRead => format!(
            "{rel} has not been read in this activation. Read it with fs_read first, then retry the edit."
        ),
        readstate::Verdict::Changed => format!(
            "{rel} changed since you last read it (another role, a command or the owner edited it). Read it again with fs_read, then redo the edit against the current content."
        ),
    };
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "read_before_edit",
        verdict.code(),
        started,
    );
    Err(ToolError::BadInput(msg))
}

pub struct FsWrite;
impl Tool for FsWrite {
    fn name(&self) -> &str {
        "fs_write"
    }
    fn description(&self) -> &str {
        r#"Write a whole file in the repo, creating parent directories.
- Use when: creating a new file, or rewriting most of an existing one.
- Do not use: for partial edits — use fs_patch (rewriting a whole file to change a few lines doubles context cost; OpenWorker incident: 1812 full rewrites) — or for stage deliverables (artifact_write).
- Errors: overwriting an existing file you have not read in this activation, or that changed since you read it, is rejected — read it with fs_read first.
- Writes outside your owned paths raise a pending card for the owner. Permission rules, skill/policy files and credential files are never writable."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"repo-relative file path"},"content":{"type":"string","description":"the complete new file content"}},"required":["path","content"]})
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
    fn precondition(&self, input: &Value, ctx: &ToolContext) -> Result<(), ToolError> {
        require_fresh_read(ctx, str_arg(input, "path")?)
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, str_arg(input, "content")?)?;
        ctx.reads.record(&p);
        Ok(json!({"written": p.strip_prefix(&ctx.repo_root).unwrap_or(&p)}))
    }
}

pub struct FsPatch;
impl Tool for FsPatch {
    fn name(&self) -> &str {
        "fs_patch"
    }
    fn description(&self) -> &str {
        r#"Replace an exact string in a repo file. The preferred way to make partial edits.
- Use when: changing part of an existing file.
- Do not use: for new files (fs_write) or stage deliverables (artifact_write).
- Read the file with fs_read first; patches to unread files, or files changed since your last read, are rejected.
- `old` must match exactly once, including whitespace and indentation. If it matches several times, add surrounding lines to make it unique, or set `replace_all: true` to change every occurrence. Keep `old` small but unique: usually 2–4 adjacent lines.
- Errors: "not found" means your copy of the text is stale or mistyped — re-read the file; "N matches" means `old` is ambiguous. Writes outside your owned paths raise a pending card."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string","description":"repo-relative file path"},"old":{"type":"string","description":"exact text to replace, copied from the file"},"new":{"type":"string","description":"replacement text"},
            "replace_all":{"type":"boolean","description":"replace every occurrence of old (default false)"}},
            "required":["path","old","new"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn builtin_deny(&self, input: &Value, ctx: &ToolContext) -> Option<String> {
        FsWrite.builtin_deny(input, ctx)
    }
    fn precondition(&self, input: &Value, ctx: &ToolContext) -> Result<(), ToolError> {
        require_fresh_read(ctx, str_arg(input, "path")?)
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let p = repo_path(&ctx.repo_root, str_arg(input, "path")?)?;
        let old = str_arg(input, "old")?;
        let new = str_arg(input, "new")?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);
        if old.is_empty() {
            return Err(ToolError::BadInput("`old` must not be empty".into()));
        }
        let content = std::fs::read_to_string(&p)?;
        // 票 02（prompt-engineering）：描述一直承诺 old 唯一，旧写法却是
        // `replacen(old, new, 1)`——多处匹配时静默改第一处，可能改错位置。
        let matches = content.matches(old).count();
        if matches == 0 {
            let head: String = old.chars().take(200).collect();
            return Err(ToolError::BadInput(format!(
                "`old` not found in {}. Your copy of the text is stale or mistyped — re-read the file with fs_read and copy the exact text. old starts with: {head:?}",
                str_arg(input, "path")?
            )));
        }
        if matches > 1 && !replace_all {
            return Err(ToolError::BadInput(format!(
                "`old` is ambiguous: {matches} matches in {}. Add surrounding lines to make it unique, or set replace_all: true to change every occurrence.",
                str_arg(input, "path")?
            )));
        }
        let patched = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        std::fs::write(&p, patched)?;
        ctx.reads.record(&p);
        let n = if replace_all { matches } else { 1 };
        // exec-cards 票 01（spec D6）：old/new 行数即替换区间的增删行——
        // 精确值不是估算。随返回值进 tool_result 事件，执行卡的 +N −M
        // 徽标数据源；模型上下文顺带得到变更规模信号。
        Ok(json!({
            "patched": str_arg(input, "path")?,
            "replacements": n,
            "diff_added": new.lines().count() * n,
            "diff_removed": old.lines().count() * n,
        }))
    }
}

// ---------- 仓内搜索（code-search-and-subagent 票 01）----------
//
// 不走执行命令（bash rg/find）的原因：执行命令过 Exec 必问卡，把「找文件」
// 这种只读定位动作变成人工裁决——票据动机就是去掉这条权限开销与上下文噪声。

/// 按文件名找仓内文件（glob 或子串）。Read 类：纯定位无副作用。
pub struct FsFind;
impl Tool for FsFind {
    fn name(&self) -> &str {
        "fs_find"
    }
    fn description(&self) -> &str {
        r#"Find repo files by name.
- Use when: you know all or part of a file name, or a path pattern.
- Do not use: to search file contents (fs_grep or sem_search). Never run find or ls through bash for this.
- `pattern` with glob characters (* ? **) matches the path or the basename; plain text matches as a case-insensitive substring. Files ignored by .gitignore are skipped. At most 100 relative paths come back — narrow the pattern if you hit the cap.
- Errors: zero hits returns count 0 — try a shorter substring or a glob such as `**/name*`."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "pattern":{"type":"string","description":"glob like 'src/**/*.rs' or plain substring"}},
            "required":["pattern"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let hits = crate::search::find_by_name(&ctx.repo_root, str_arg(input, "pattern")?);
        Ok(json!({"count": hits.len(), "paths": hits}))
    }
}

/// 仓内正文搜索（字面量，返回 path:line）。Read 类。
pub struct FsGrep;
impl Tool for FsGrep {
    fn name(&self) -> &str {
        "fs_grep"
    }
    fn description(&self) -> &str {
        r#"Search file contents for a literal substring.
- Use when: you know an exact identifier, string or error message.
- Do not use: for regular expressions (the query is literal text, not a regex), for questions about meaning (sem_search), or through bash grep/rg.
- Returns path, line number and line text for up to 100 hits; ignored and binary files are skipped. Hitting 100 means the query is too broad — make it more specific."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "query":{"type":"string","description":"literal text to search for"}},
            "required":["query"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let hits = crate::search::grep_content(&ctx.repo_root, str_arg(input, "query")?);
        Ok(json!({"count": hits.len(), "hits": hits}))
    }
}

/// bash 族（agent-senses 票 05 / ADR 0058-3）：同一执行漏斗三种形态——
/// 一次性（默认）、命名会话（`session`，cwd/env 随 sh 进程保持）、
/// 后台任务（`background`→task_id，`bash_output` 游标读增量，
/// `bash_kill` 终止）。形状记忆轴仍是 `cmd`——session/background/
/// timeout_ms 不参与匹配（票 05-④，sessions.rs 头注）。
pub struct Bash;
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        r#"Run a shell command in the repo. Commands go through the permission layer and may raise a pending card, which costs the owner a decision.
- Use when: the job needs a shell — builds, tests, git, package managers, project scripts.
- Do not use: to find files (fs_find), search contents (fs_grep, sem_search), read files (fs_read), edit files (fs_patch, fs_write) or print messages (reply in text instead).
- Chain dependent commands with && in one call. `session` reuses a persistent named shell; `background` returns a task_id at once — read it with bash_output, stop it with bash_kill.
- Never skip hooks or checks (--no-verify) to make a failure go away. Commands that touch credential material are always denied."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "cmd":{"type":"string","description":"the shell command to run"},
            "session":{"type":"string","description":"persistent shell session name — same name reuses one sh (cwd/env kept)"},
            "background":{"type":"boolean","description":"run detached, return task_id immediately"},
            "timeout_ms":{"type":"integer","description":"default 30000, max 600000"},
            "net":{"type":"boolean","description":"allow network inside the OS sandbox (default false — declare when the command needs egress; visible on the approval card)"}},
            "required":["cmd"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let cmd = input["cmd"].as_str().unwrap_or("");
        bash_hits_credentials(cmd).then(|| "command touches credential material".into())
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let cmd = str_arg(input, "cmd")?;
        let timeout = std::time::Duration::from_millis(
            input["timeout_ms"]
                .as_u64()
                .unwrap_or(crate::sessions::DEFAULT_TIMEOUT_MS)
                .clamp(1, crate::sessions::MAX_TIMEOUT_MS),
        );
        // 票 06：net 默认关——命令要出网须显式声明，声明进必问卡
        // raw_input 给负责人看（ADR 0058-4：应用层判域名，OS 沙箱管开关）。
        let net = input["net"].as_bool().unwrap_or(false);
        if input["background"].as_bool().unwrap_or(false) {
            return ctx.sessions.spawn_task(db, ctx, cmd, net);
        }
        match input["session"].as_str() {
            Some(name) => ctx.sessions.run_in(db, ctx, name, cmd, timeout, net),
            None => ctx.sessions.run_oneshot(db, ctx, cmd, timeout, net),
        }
    }
}

/// 后台任务/会话输出游标读（票 05）：纯观测无执行副作用——命令本身
/// 已在 bash 调用处过 Exec 必问，读它的输出不新增风险面，故 Read 类。
pub struct BashOutput;
impl Tool for BashOutput {
    fn name(&self) -> &str {
        "bash_output"
    }
    fn description(&self) -> &str {
        r#"Read new output from a background task or a named shell session.
- Use when: you started bash with `background` or `session` and need its output.
- Do not use: for a one-shot bash call — its output is already in that call's result.
- Pass `task_id` or `session`. `cursor_out` / `cursor_err` continue from the cursors in the previous response; omit them on the first read. Do other useful work between reads instead of polling in a tight loop.
- Errors: an unknown task_id or session is reported; check the id returned by bash."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "task_id":{"type":"string","description":"task id returned by a background bash call"},
            "session":{"type":"string","description":"named shell session"},
            "cursor_out":{"type":"integer","description":"stdout cursor from the previous response"},
            "cursor_err":{"type":"integer","description":"stderr cursor from the previous response"}}})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        ctx.sessions.read_output(
            db,
            ctx,
            input["task_id"].as_str(),
            input["session"].as_str(),
            input["cursor_out"].as_u64().unwrap_or(0),
            input["cursor_err"].as_u64().unwrap_or(0),
        )
    }
}

/// 终止后台任务或会话（票 05）：幂等——已退出者回报不重复杀。
/// Exec 类与 bash 同轴：终止运行中进程是有副作用的动作。
pub struct BashKill;
impl Tool for BashKill {
    fn name(&self) -> &str {
        "bash_kill"
    }
    fn description(&self) -> &str {
        r#"Stop a background task or a named shell session, including its process group.
- Use when: a background command is no longer needed, hangs or runs away.
- Do not use: to cancel a one-shot bash call (it has already returned).
- Pass `task_id` or `session`. Idempotent: an already-exited target reports killed=false.
- Errors: an unknown task_id or session is reported."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "task_id":{"type":"string","description":"task id returned by a background bash call"},
            "session":{"type":"string","description":"named shell session"}}})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        ctx.sessions.kill(
            db,
            ctx,
            input["task_id"].as_str(),
            input["session"].as_str(),
        )
    }
}

/// 产物写入：走产物管道（元数据头校验 + `.hexagon/` 落盘 + 版本取代 + 登记）。
pub struct ArtifactWrite;
impl Tool for ArtifactWrite {
    fn name(&self) -> &str {
        "artifact_write"
    }
    fn description(&self) -> &str {
        r#"Deliver an artifact to .hexagon/<path>: checks the metadata header, registers the artifact and supersedes earlier versions.
- Use when: producing a deliverable of the current stage or a handoff for another role.
- Do not use: for ordinary repository files (fs_write, fs_patch). For small changes to an existing artifact, prefer fs_patch on its file. Runnable code (html, css, js, and kind 代码) also needs the same relative path written at the repository root with fs_write; a copy that exists only under .hexagon/ does not satisfy the stage.
- Pass `kind` equal to the due deliverable name, verbatim (for example 范围说明), or start `content` with the header: line 1 `---`, line 2 `kind: <name>`, line 3 `---`. Without either it registers as misc and does not count toward the stage.
- Errors: a malformed metadata header is reported — fix the header and deliver again."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"artifact path under .hexagon/ (without the prefix)"},"content":{"type":"string","description":"artifact content"},"kind":{"type":"string","description":"due deliverable name, verbatim"}},"required":["path","content"]})
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
        r#"Read a registered artifact by its path under .hexagon/.
- Use when: reading an artifact listed in context.artifacts of your brief.
- Do not use: for other repository files (fs_read).
- Pass the path as listed in the brief, without the .hexagon/ prefix. Returns up to 256KB of text.
- Errors: an unknown path returns an I/O error — check context.artifacts for the exact path."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"artifact path under .hexagon/ (without the prefix)"}},"required":["path"]})
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
        if let Some(s) = &ctx.subagent {
            s.reads.lock().unwrap().push(rel.clone());
        }
        Ok(json!({"content": String::from_utf8_lossy(&bytes[..bytes.len().min(FS_READ_CAP)])}))
    }
}

// ---------- web_fetch（agent-senses 票 01 / ADR 0058-1）----------
//
// 出处：ADR 0058-1——fetch 自研内置（ureq 已在依赖，不引新 HTTP 栈）。
// 重定向不自动跟随：3xx 返回 {redirect} 让 Agent 重新发起，每一跳
// 都重新过 Egress 判定——自动跟随会让 302 到未授权域名的出站绕过
// 必问卡。不注册进 readonly()：只读嵌套借 URL 参数可外带仓内信息
// （exfiltration 面），研究助手保持「只进不出」。
// HTML→文本是自研极简提取而非 readability 库：后者要拉 html5ever
// 依赖树，与「fetch 自研轻量」同向；局限是属性值内含 '>' 的畸形
// 标签会留下残渣字符（可容忍，不进安全面）。

/// web_fetch 正文进上下文的显示预算；完整内容照常 spill 落盘。
pub const WEB_FETCH_CAP: usize = 96 * 1024;
/// 原始响应体硬读上限（进内存前截断，与显示预算不同轴）。
const WEB_FETCH_RAW_CAP: u64 = 8 * 1024 * 1024;

pub struct WebFetch;
impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        r#"Fetch an http(s) URL and return readable text (HTML is converted to plain text).
- Use when: you need the content of a specific page, such as a web_search result or a URL the owner gave.
- Do not use: for local files (fs_read) or non-http schemes (always rejected).
- Network access may raise a pending card. Redirects are not followed: a 3xx returns {redirect: url} — call web_fetch again with that URL so each hop re-runs the permission check.
- Long pages are trimmed and the full text is saved to a spill file you can read with fs_read.
- Errors: non-text content types return an error instead of bytes."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "url":{"type":"string","description":"http(s) URL to fetch"},
            "max_chars":{"type":"integer","description":"display cap override (default ~96k, min 4k)"}},
            "required":["url"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Egress
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        // 输入级硬拒：web_fetch 只碰 http(s)——file:// 会把本地文件读进上下文
        let u = input["url"].as_str().unwrap_or("");
        match u.split_once(':') {
            Some((s, _)) if s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https") => {
                None
            }
            _ => Some("web_fetch only fetches http(s) URLs".into()),
        }
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let url = str_arg(input, "url")?;
        let cap = input["max_chars"]
            .as_u64()
            .unwrap_or(WEB_FETCH_CAP as u64)
            .clamp(4096, WEB_FETCH_CAP as u64) as usize;
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .new_agent();
        let mut resp = agent
            .get(url)
            .call()
            .map_err(|e| ToolError::Exec(format!("fetch {url}: {e}")))?;
        let status = resp.status().as_u16();
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        if (300..400).contains(&status) {
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            return Ok(json!({
                "status": status,
                "redirect": join_url(url, loc),
                "note": "not followed — call web_fetch again with the redirect target",
            }));
        }
        let mut body = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(resp.body_mut().as_reader(), WEB_FETCH_RAW_CAP),
            &mut body,
        )
        .map_err(|e| ToolError::Exec(format!("read body: {e}")))?;
        let is_html = ct.contains("text/html") || ct.contains("application/xhtml");
        let is_text = ct.starts_with("text/")
            || ct.contains("json")
            || ct.contains("xml")
            || ct.contains("javascript")
            || ct.is_empty();
        if !is_text {
            return Ok(json!({
                "status": status,
                "content_type": ct,
                "error": "unsupported content-type (not text)",
            }));
        }
        let raw = String::from_utf8_lossy(&body).into_owned();
        let text = if is_html { html_to_text(&raw) } else { raw };
        let truncated = text.len() > cap;
        let content = if truncated {
            spill_trim(ctx, &text, cap)
        } else {
            text
        };
        Ok(json!({
            "status": status,
            "content_type": ct,
            "truncated": truncated,
            "content": content,
        }))
    }
}

/// 相对 Location 解析成绝对 URL（不引 url crate——web_fetch 只需要
/// scheme/host/path 三段拼装；非标形态原样返回交给下一轮判定）。
fn join_url(base: &str, loc: &str) -> String {
    let loc = loc.trim();
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return loc.into();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return loc.into();
    };
    if let Some(p) = loc.strip_prefix("//") {
        return format!("{scheme}://{p}");
    }
    let host_end = rest.find('/').unwrap_or(rest.len());
    let origin = &rest[..host_end];
    if loc.starts_with('/') {
        return format!("{scheme}://{origin}{loc}");
    }
    let dir = rest[host_end..]
        .rsplit_once('/')
        .map(|(d, _)| d)
        .unwrap_or("");
    format!("{scheme}://{origin}{dir}/{loc}")
}

/// `<pat`（pat 已含 '<'）的大小写不敏感定位；要求标签名后跟
/// 非字母数字边界，`<scriptx` 不命中 `<script`。
fn find_tag(s: &str, pat: &str) -> Option<usize> {
    let b = s.as_bytes();
    let p = pat.as_bytes();
    let mut i = 0;
    while i + p.len() <= b.len() {
        if b[i] == b'<'
            && b[i + 1..i + p.len()]
                .iter()
                .zip(&p[1..])
                .all(|(a, c)| a.eq_ignore_ascii_case(c))
        {
            let next = b.get(i + p.len()).copied();
            let boundary = next.is_none_or(|c| c == b'>' || c == b'/' || c.is_ascii_whitespace());
            if boundary {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 整块删除 `<tag>…</tag>`（含自闭合）；未闭合的块截到串尾。
fn strip_tag_blocks(s: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = find_tag(rest, &open) {
        out.push_str(&rest[..i]);
        let Some(gt) = rest[i..].find('>').map(|o| i + o) else {
            return out;
        };
        // 自闭合 `<tag/>` 只丢标签本身，不吞后续内容
        if gt > i && rest.as_bytes()[gt - 1] == b'/' {
            rest = &rest[gt + 1..];
            continue;
        }
        match find_tag(&rest[gt + 1..], &close) {
            Some(c) => {
                let c = gt + 1 + c;
                let end = rest[c..].find('>').map(|o| c + o + 1).unwrap_or(rest.len());
                rest = &rest[end..];
            }
            None => return out, // 未闭合：开标签之后全丢
        }
    }
    out.push_str(rest);
    out
}

/// 换行语义的块级标签（剥标签时替换为 '\n'）。
const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "tr",
    "td",
    "th",
    "table",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "blockquote",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "figure",
    "figcaption",
    "pre",
    "hr",
    "dl",
    "dt",
    "dd",
    "form",
    "fieldset",
];

/// 常用与数字实体解码；返回 (字符, 消费字节数)。
fn decode_entity(s: &str) -> Option<(char, usize)> {
    let end = s.find(';')?;
    if end > 12 {
        return None;
    }
    let ent = &s[1..end];
    let ch = match ent {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" | "#39" => '\'',
        "nbsp" => ' ',
        _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16)
            .ok()
            .and_then(char::from_u32)?,
        _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32)?,
        _ => return None,
    };
    Some((ch, end + 1))
}

/// 极简 HTML→文本提取（v1 不追求 readability 级正文识别）。
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "template", "svg", "select"] {
        s = strip_tag_blocks(&s, tag);
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(o) => i += o + 3,
                None => break,
            }
            continue;
        }
        let c = rest.chars().next().unwrap();
        if c == '<' {
            match rest.find('>') {
                Some(o) => {
                    let name = rest[1..o]
                        .trim_start_matches('/')
                        .split(|c: char| c.is_whitespace() || c == '/')
                        .next()
                        .unwrap_or("");
                    if BLOCK_TAGS.iter().any(|t| t.eq_ignore_ascii_case(name)) {
                        out.push('\n');
                    }
                    i += o + 1;
                }
                None => break,
            }
            continue;
        }
        if c == '&' {
            if let Some((ch, adv)) = decode_entity(rest) {
                out.push(ch);
                i += adv;
                continue;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    // 折叠空白：行内空白并一格，连续空行合一
    let mut res = String::with_capacity(out.len());
    let mut blank = 0u8;
    for line in out.lines() {
        let t = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        if !res.is_empty() {
            res.push('\n');
        }
        res.push_str(&t);
    }
    res.trim().to_string()
}

/// 技能按需加载（票 09）：catalog 在系统提示里是一行式指针，
/// 全文从这里取。每次调用重扫目录——会话中新建的技能也能取到。
pub struct LoadSkill;
impl Tool for LoadSkill {
    fn name(&self) -> &str {
        "load_skill"
    }
    fn description(&self) -> &str {
        r#"Load a skill's full instructions by name.
- Use when: a skill in the available-skills list, or one granted to your role, is relevant to the task — load it before acting on it.
- Do not use: to guess skills that are not listed.
- Pass the name exactly as listed. An unknown name returns the list of available skills."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"name":{"type":"string","description":"skill name exactly as listed"}},"required":["name"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, _db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let requested = str_arg(input, "name")?;
        let role: String = _db
            .conn()
            .query_row(
                "SELECT role FROM agents WHERE id=?1",
                [&ctx.agent_id],
                |r| r.get(0),
            )
            .unwrap_or_default();
        // 目录是「经验-<角色>」。单独的「经验」按当前角色找；目录里两行并存时
        // 点名用「经验（角色）」。
        let aliased = crate::skills::experience_load_name(requested, &role);
        let name = crate::skills::validate_name(&aliased).map_err(ToolError::BadInput)?;
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
