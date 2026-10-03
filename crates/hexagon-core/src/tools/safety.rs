//! 内置 deny 表与路径安全/spill——结构性护栏层（内置 deny 最高优先不可覆盖）。

use super::*;

// ---------- 内置 deny 与路径安全 ----------

/// 凭据类路径：正文永远不进上下文（内置 deny，最高优先）。
/// reliability 03: MCP/package-manager configuration can store inline keys;
/// protecting only keychain/.env left those known credential containers readable.
pub(crate) fn is_credential_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.split('/').any(|base| {
        matches!(
            base,
            ".ssh"
                | ".aws"
                | "credentials"
                | "credentials.json"
                | "netrc"
                | ".netrc"
                | "mcp.json"
                | ".npmrc"
                | ".pypirc"
        ) || base.contains("keychain")
            || base.starts_with(".env")
            || base.ends_with(".pem")
            || base.ends_with(".key")
            || base.starts_with("id_rsa")
            || base.starts_with("id_ed25519")
    })
}

/// reliability 03 / D03: apply before reading bytes, including resolved aliases.
/// False negative refuses one read; false positive leaks a secret into durable
/// context. Hard-linked files have ambiguous identities and are denied too.
pub(crate) fn sensitive_file_path(path: &Path) -> bool {
    if is_credential_path(&path.to_string_lossy()) {
        return true;
    }
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // Desktop ticket 07: ordinary file/search tools must not bypass revocation
    // of screenshot-sharing consent by rereading the clearable private trail.
    let normalized = resolved.to_string_lossy().replace('\\', "/").to_lowercase();
    if normalized.contains("/.hexagon/computer-use/")
        || normalized.ends_with("/.hexagon/computer-use")
    {
        return true;
    }
    if is_credential_path(&resolved.to_string_lossy()) {
        return true;
    }
    if crate::credentials::active_file_path()
        .is_some_and(|p| p.canonicalize().unwrap_or(p) == resolved)
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if resolved
            .metadata()
            .is_ok_and(|m| m.is_file() && m.nlink() > 1)
        {
            return true;
        }
    }
    false
}

pub(crate) fn record_sensitive_read_rejection(started: std::time::Instant) {
    // Background index expansion has no agent identity. Do not log paths or
    // contents just to fill identity fields; the tool envelope carries IDs.
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        None,
        None,
        None,
        None,
        "content_read",
        "sensitive_content",
        started,
    );
}

pub(crate) fn readable_repo_path(root: &Path, rel: &str) -> Result<PathBuf, ToolError> {
    let started = std::time::Instant::now();
    let path = repo_path(root, rel)?;
    if is_credential_path(rel) || sensitive_file_path(&path) {
        record_sensitive_read_rejection(started);
        return Err(ToolError::BadInput(
            "sensitive content is not readable by agents".into(),
        ));
    }
    Ok(path)
}

/// Generic file/search tools in evaluation workers cannot read host state.
/// 2026-09-28 eval-2 read pack/control/lock files successfully. False negative
/// costs an artifact_read; false positive exposes host protocol state. Registered
/// artifacts retain the separate host-validated artifact_read path.
pub(crate) fn agent_readable_repo_path(root: &Path, rel: &str) -> Result<PathBuf, ToolError> {
    let started = std::time::Instant::now();
    let path = readable_repo_path(root, rel)?;
    if root.join(".hexagon/evaluation-worker").is_file()
        && path
            .strip_prefix(root.canonicalize()?)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".hexagon")
            })
    {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "content_read",
            "evaluation_private_state",
            started,
        );
        return Err(ToolError::BadInput(
            "evaluation host state requires a registered artifact".into(),
        ));
    }
    Ok(path)
}

/// 权限规则文件：Agent 不得自改规则（内置 deny）。
pub(crate) fn is_permission_rule_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    lower.contains("permission_rules") || lower.ends_with(".hexagon/permissions.toml")
}

/// agent 自授策略面（票 09）：skills 定义与本地会话偏好是负责人管理的
/// 提示词内容——写进 SKILL.md = 给自己追加指令，与 permission_rules 同类。
/// reliability 02: structure writes check resolved paths; terminal isolation
/// denies direct writes to the host state tree. Approval is not isolation.
pub fn is_agent_policy_path(p: &str) -> bool {
    let lower = p.to_lowercase();
    let Some(rel) = lower.strip_prefix(".hexagon/") else {
        return false;
    };
    let first = rel.split('/').next().unwrap_or("");
    matches!(
        first,
        "skills"
            | "local"
            | "roles"
            | "roles.json"
            | "mcp.json"
            | "permissions.toml"
            | "prices.json"
            | "request-locks"
            | "write-locks"
            | "skill-mutes.json"
            | "ui.json"
            | "computer-use"
    ) || first.starts_with("state.db")
        // Evaluation 08 probe reproduced a structured overwrite of the
        // isolation marker. These host protocol files are never Agent policy.
        || first.starts_with("evaluation-")
        || first.starts_with("pack.")
        || first == "pack"
        || first == "pack-permission.json"
}

/// bash 命令里的凭据探测模式。
pub(crate) fn bash_hits_credentials(cmd: &str) -> bool {
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

pub(crate) fn scrub_input(tool: &str, input: &Value) -> Value {
    match tool {
        "fs_write" | "artifact_write" => json!({
            "path": input["path"],
            "bytes": input["content"].as_str().map(|s| s.len()).unwrap_or(0),
        }),
        // exec-cards 票 01（spec D5，owner 裁决）：old/new 是模型自产的
        // 变更片段而非文件全文——各截 4KB 放行，给执行卡的 diff 体供数据。
        // 文件体纪律不松：fs_write/artifact_write 的 content 仍只留 bytes。
        "fs_patch" => json!({
            "path": input["path"],
            "old": clip4k(input["old"].as_str()),
            "new": clip4k(input["new"].as_str()),
        }),
        _ => redact(input),
    }
}

/// 4KB 截断 + marker（票 01）：超界说明这段 patch 本身就大到不该
/// 进事件日志，marker 让 UI 知道体是残本不是全文。
fn clip4k(s: Option<&str>) -> Value {
    match s {
        None => Value::Null,
        Some(s) if s.len() <= 4096 => json!(s),
        Some(s) => {
            let mut end = 4096;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            json!(format!("{}\n…[truncated]", &s[..end]))
        }
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
    // 2026-09-28 eval-2: caching output created .gitignore outside task scope.
    // ponytail: isolated evaluations use the existing bounded-output fallback;
    // add a host cache read capability only if truncation blocks real tasks.
    // Do not exempt .gitignore or expose host state to make the cache readable.
    if ctx.repo_root.join(".hexagon/evaluation-worker").is_file() {
        return None;
    }
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
    // reliability 02: the old lexical OR accepted a symlink inside the repo
    // even when its target escaped. Resolve the nearest existing ancestor too,
    // so creating a new file under an escaping directory symlink is rejected.
    let mut existing = norm.as_path();
    let mut tail = Vec::new();
    while !existing.exists() {
        if std::fs::symlink_metadata(existing).is_ok() {
            return Err(ToolError::PathEscape(rel.into()));
        }
        let Some(name) = existing.file_name() else {
            return Err(ToolError::PathEscape(rel.into()));
        };
        tail.push(name.to_os_string());
        let Some(parent) = existing.parent() else {
            return Err(ToolError::PathEscape(rel.into()));
        };
        existing = parent;
    }
    let mut check = existing.canonicalize()?;
    for part in tail.iter().rev() {
        check.push(part);
    }
    let root_c = root.canonicalize()?;
    if !check.starts_with(&root_c) {
        return Err(ToolError::PathEscape(rel.into()));
    }
    Ok(check)
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

pub(crate) fn str_arg<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input[key]
        .as_str()
        .ok_or_else(|| ToolError::BadInput(format!("missing string arg: {key}")))
}

#[cfg(test)]
mod evaluation_policy_tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn evaluation_private_reads_reject_normalized_paths_and_aliases(name in "[a-z]{1,20}", alias in any::<bool>()) {
            let dir = tempfile::tempdir().unwrap();
            let private = dir.path().join(".hexagon");
            std::fs::create_dir(&private).unwrap();
            std::fs::write(private.join("evaluation-worker"), "private-state-v1").unwrap();
            std::fs::write(private.join(&name), "private").unwrap();
            let input = if alias {
                std::os::unix::fs::symlink(private.join(&name), dir.path().join("alias")).unwrap();
                "alias".into()
            } else { format!("./.hexagon/../.hexagon/{name}") };
            prop_assert!(agent_readable_repo_path(dir.path(), &input).is_err());
            std::fs::write(dir.path().join("public.txt"), "public").unwrap();
            prop_assert!(agent_readable_repo_path(dir.path(), "public.txt").is_ok());
        }

        #[test]
        fn evaluation_protocol_paths_are_always_owner_managed(suffix in "[a-z0-9/-]{0,30}",upper in any::<bool>()) {
            let mut path=format!(".hexagon/evaluation-{suffix}");
            if upper {path=path.to_uppercase();}
            prop_assert!(is_agent_policy_path(&path));
        }
    }
}
