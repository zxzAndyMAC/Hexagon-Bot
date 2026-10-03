//! Owner Q6/Q11/Q15 (2026-10-01): broad host access remains an explicit
//! project capability, never a reason to turn off OS isolation or role ownership.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, Write};

const CONTENT_CAP: usize = 8 * 1024 * 1024;
fn reject(message: &str) -> ToolError {
    ToolError::NotExecuted(message.into())
}
fn mode(db: &Db, ctx: &ToolContext) -> Result<(), ToolError> {
    if ctx.subagent.is_some()
        || ctx.native_effect.is_some()
        || ctx.repo_root.join(".hexagon/evaluation-worker").exists()
    {
        return Err(reject(
            "host access is not delegated to a subagent or evaluation worker",
        ));
    }
    if crate::approval_mode::read(db, &ctx.project_id)?.mode
        != crate::approval_mode::ApprovalMode::Broad
    {
        return Err(reject(
            "host access requires this project's owner-selected Broad access mode",
        ));
    }
    Ok(())
}

/// Resolve through the existing canonical path primitive, then keep external
/// tools away from all project/policy roots. False negatives require opening the
/// correct project; false positives bypass a role or expose host credentials.
fn checked_path(ctx: &ToolContext, raw: &str, directory: bool) -> Result<PathBuf, ToolError> {
    if !Path::new(raw).is_absolute() {
        return Err(reject("host path must be absolute"));
    }
    let resolved = repo_path(Path::new("/"), raw)?;
    let lower = resolved.to_string_lossy().to_ascii_lowercase();
    if sensitive_file_path(&resolved)
        || is_permission_rule_path(&lower)
        || lower.split('/').any(|part| {
            let part = part
                .strip_suffix("-wal")
                .or_else(|| part.strip_suffix("-shm"))
                .unwrap_or(part);
            matches!(
                part,
                ".hexagon"
                    | ".agents"
                    | ".codex"
                    | ".claude"
                    | "agents.md"
                    | "claude.md"
                    | "skill.md"
                    | "cookies"
                    | "cookies.sqlite"
                    | "login data"
                    | "logins.json"
                    | "key4.db"
                    | "auth.json"
                    | ".git-credentials"
                    | ".docker"
                    | ".kube"
                    | ".azure"
                    | "gcloud"
            )
        })
    {
        return Err(reject(
            "host credentials, policy and agent state are protected",
        ));
    }
    let project = ctx.repo_root.canonicalize()?;
    if resolved.starts_with(&project) || (directory && project.starts_with(&resolved)) {
        return Err(reject(
            "use project tools for project paths; host cwd cannot enclose the project",
        ));
    }
    for parent in resolved.ancestors().skip(usize::from(!directory)) {
        if parent.join(".hexagon").exists() {
            return Err(reject(
                "another Hexagon project must be accessed through its own roles",
            ));
        }
    }
    if directory && !resolved.is_dir() {
        return Err(reject("host cwd must be an existing directory"));
    }
    if !directory && !resolved.parent().is_some_and(Path::is_dir) {
        return Err(reject("host file parent must already exist"));
    }
    Ok(resolved)
}

fn path(ctx: &ToolContext, raw: &str, directory: bool) -> Result<PathBuf, ToolError> {
    let started = std::time::Instant::now();
    let result = checked_path(ctx, raw, directory);
    if result.is_err() {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "host_path",
            "protected_or_invalid_scope",
            started,
        );
    }
    result
}

fn open(path: &Path, write: bool, new: bool) -> Result<std::fs::File, ToolError> {
    // Open every canonical path component relative to a pinned directory fd.
    // O_NOFOLLOW on just the final file left a parent-directory swap window.
    #[cfg(unix)]
    let file = {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let mut parent = std::fs::File::open("/")?;
        let components: Vec<_> = path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        for (index, name) in components.iter().enumerate() {
            let name = std::ffi::CString::new(name.as_bytes())
                .map_err(|_| reject("invalid host filename"))?;
            let last = index + 1 == components.len();
            let flags = libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | if last {
                    if write {
                        libc::O_RDWR
                    } else {
                        libc::O_RDONLY
                    }
                } else {
                    libc::O_RDONLY | libc::O_DIRECTORY
                }
                | if last && new {
                    libc::O_CREAT | libc::O_EXCL
                } else {
                    0
                };
            // The pinned directory fd and NUL-terminated component live through
            // openat; ownership of a successful returned fd transfers to File.
            let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            parent = unsafe { std::fs::File::from_raw_fd(fd) };
        }
        parent
    };
    #[cfg(not(unix))]
    let file: std::fs::File = return Err(reject(
        "host file isolation is unavailable on this platform",
    ));
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(reject("host hardlink aliases are protected"));
        }
    }
    if !metadata.is_file() || metadata.len() > CONTENT_CAP as u64 {
        return Err(reject(
            "host file must be a regular file no larger than 8 MiB",
        ));
    }
    Ok(file)
}
fn fingerprint(file: &mut std::fs::File) -> Result<String, ToolError> {
    file.rewind()?;
    let mut hash = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        hash.update(&chunk[..count]);
    }
    file.rewind()?;
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn guard(
    db: &Db,
    ctx: &ToolContext,
    name: &str,
    input: &Value,
) -> Result<Option<String>, ToolError> {
    if !matches!(name, "host_fs_read" | "host_fs_write" | "host_bash") {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let checked = (|| {
        mode(db, ctx)?;
        if name != "host_fs_read"
            && crate::permissions::agent_role(db, ctx).as_deref() == Some(crate::pm_route::PM_ROLE)
        {
            return Err(reject(
                "project manager cannot use host write/terminal capabilities",
            ));
        }
        path(
            ctx,
            str_arg(input, if name == "host_bash" { "cwd" } else { "path" })?,
            name == "host_bash",
        )?;
        if name == "host_bash" && bash_hits_credentials(str_arg(input, "cmd")?) {
            return Err(reject("command touches credential material"));
        }
        Ok(())
    })();
    crate::diag::note(
        if checked.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        checked.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "host_scope",
        if checked.is_err() {
            "protected_or_ungranted"
        } else {
            "bounded_external_scope"
        },
        started,
    );
    Ok(checked.err().map(|error| error.to_string()))
}

pub struct HostRead;
impl Tool for HostRead {
    fn name(&self) -> &str {
        "host_fs_read"
    }
    fn description(&self) -> &str {
        "Use when broad access is enabled to read an ordinary external absolute file (up to 8 MiB). Returns bounded text plus sha256 for a possible explicit replacement. Current/other Hexagon project paths, credentials and agent policy are forbidden; use project tools for owned project source. Do not use for project source, secrets, policy files or subagent tasks."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Absolute path to an ordinary external file outside Hexagon projects and protected locations."}},"required":["path"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Read
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        mode(db, ctx)?;
        let path = path(ctx, str_arg(input, "path")?, false)?;
        let mut file = open(&path, false, false)?;
        let hash = fingerprint(&mut file)?;
        let mut bytes = Vec::new();
        file.take(FS_READ_CAP as u64).read_to_end(&mut bytes)?;
        Ok(
            json!({"path":path,"sha256":hash,"content":String::from_utf8_lossy(&bytes),"truncated":path.metadata()?.len()>bytes.len() as u64}),
        )
    }
}
pub struct HostWrite;
impl Tool for HostWrite {
    fn name(&self) -> &str {
        "host_fs_write"
    }
    fn description(&self) -> &str {
        "Use when broad access is enabled to create an ordinary external absolute file in an existing directory, or replace an existing file with expected_sha256 from host_fs_read. Existing-file replacements always require owner approval and fail if the bytes changed. Do not use for project source, credentials or agent policy. No delete or directory creation."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Absolute path to an ordinary external file outside Hexagon projects and protected locations."},"content":{"type":"string","maxLength":8388608,"description":"Complete UTF-8 content to create or replace after authorization."},"expected_sha256":{"type":"string","pattern":"^[a-f0-9]{64}$","description":"Hash from host_fs_read, required when replacing an existing file."}},"required":["path","content"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn precondition(&self, input: &Value, ctx: &ToolContext) -> Result<(), ToolError> {
        let path = path(ctx, str_arg(input, "path")?, false)?;
        if str_arg(input, "content")?.len() > CONTENT_CAP {
            return Err(reject("host content exceeds 8 MiB"));
        }
        if path.exists() && input["expected_sha256"].as_str().is_none() {
            return Err(reject(
                "read the external file and provide expected_sha256 before replacing it",
            ));
        }
        Ok(())
    }
    fn reconcile(
        &self,
        db: &Db,
        input: &Value,
        ctx: &ToolContext,
        _id: &str,
    ) -> Result<Reconciliation, ToolError> {
        mode(db, ctx)?;
        let path = path(ctx, str_arg(input, "path")?, false)?;
        let mut file = open(&path, false, false)?;
        if fingerprint(&mut file)?
            == format!(
                "{:x}",
                Sha256::digest(str_arg(input, "content")?.as_bytes())
            )
        {
            Ok(Reconciliation::Succeeded {
                output: json!({"written":path}),
                evidence: "external file bytes match the recorded replacement".into(),
            })
        } else {
            Ok(Reconciliation::Unresolved {
                evidence: "external file no longer proves the recorded outcome".into(),
            })
        }
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        mode(db, ctx)?;
        let path = path(ctx, str_arg(input, "path")?, false)?;
        self.precondition(input, ctx)?;
        let expected = input["expected_sha256"].as_str();
        let mut file = open(&path, true, expected.is_none())?;
        file.try_lock().map_err(|_| {
            if expected.is_none() {
                // create_new already produced an empty file: never call a later
                // lock failure "not executed" and silently retry the creation.
                ToolError::Exec("new external file exists but could not be locked".into())
            } else {
                reject("external file is busy")
            }
        })?;
        if let Some(expected) = expected {
            if fingerprint(&mut file)? != expected {
                return Err(reject(
                    "external file changed since the approved read; reread and ask again",
                ));
            }
        }
        file.set_len(0)?;
        file.write_all(str_arg(input, "content")?.as_bytes())?;
        file.sync_all()?;
        Ok(
            json!({"written":path,"sha256":format!("{:x}",Sha256::digest(str_arg(input,"content")?.as_bytes()))}),
        )
    }
}

pub struct HostBash;
impl Tool for HostBash {
    fn name(&self) -> &str {
        "host_bash"
    }
    fn description(&self) -> &str {
        "Use when broad access is enabled to run one foreground command in an explicit external absolute cwd after owner approval. OS sandbox confines writes to that cwd and protects credentials, policy and nested Hexagon projects. cwd cannot contain the current project. Do not use for persistent/background sessions or commands inside a Hexagon project. Network is off unless net:true is explicitly approved."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"cwd":{"type":"string","description":"Existing absolute external working directory; cannot encompass the current project."},"cmd":{"type":"string","description":"One foreground shell command for explicit owner review."},"timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"description":"Maximum command runtime in milliseconds."},"net":{"type":"boolean","description":"Request network access in the owner approval; defaults to false."}},"required":["cwd","cmd"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        mode(db, ctx)?;
        let cwd = path(ctx, str_arg(input, "cwd")?, true)?;
        let net = input["net"].as_bool().unwrap_or(false);
        let mut scoped = ctx.clone();
        scoped.repo_root = cwd;
        scoped.owned_globs.clear();
        // Keep bookkeeping/lease in the real project; external cwd must not gain
        // a .hexagon directory that would masquerade as another project later.
        scoped.write_lease = Some(Arc::new(writeguard::repository_lock(ctx)?));
        let spec = host_spec(&scoped.repo_root, net)?;
        let spec = crate::design::guard_shell(db, &scoped, spec);
        let timeout = std::time::Duration::from_millis(
            input["timeout_ms"]
                .as_u64()
                .unwrap_or(30000)
                .clamp(1, 600000),
        );
        ctx.sessions.run_oneshot_with_spec(
            db,
            &scoped,
            str_arg(input, "cmd")?,
            timeout,
            (net, Some(spec), Some(ctx.repo_root.clone())),
        )
    }
}

fn host_spec(cwd: &Path, net: bool) -> Result<crate::sandbox::SandboxSpec, ToolError> {
    let crate::sandbox::SandboxSpec::Seatbelt(mut profile) =
        crate::sandbox::spec_for(cwd, &[], net)
    else {
        return Ok(crate::sandbox::SandboxSpec::Unavailable);
    };
    // Whole nested projects, not merely their state directories, retain ownership.
    // Bounded inventory fails closed; do not silently skip a large source tree.
    let mut queue = vec![cwd.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = queue.pop() {
        seen += 1;
        if seen > 20000 {
            return Err(reject(
                "host cwd inventory exceeds scope limit; choose a narrower directory",
            ));
        }
        if dir != cwd && dir.join(".hexagon").exists() {
            let text = dir.to_string_lossy();
            if text.contains(['"', '\\', '\n', '\r']) {
                return Err(reject(
                    "host cwd contains an unrepresentable protected project",
                ));
            }
            profile.push_str(&format!("(deny file-write* (subpath \"{text}\"))\n"));
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                queue.push(entry.path());
            }
        }
    }
    for (operations, pattern) in [
        (
            "file-read-data file-write*",
            r"/(\.hexagon|\.agents|\.codex|\.claude)(/|$)",
        ),
        ("file-write*", r"/(agents\.md|claude\.md|skill\.md)$"),
        (
            "file-read-data file-write*",
            r"/(cookies(\.sqlite)?|login data|logins\.json|key4\.db|auth\.json|\.git-credentials)(-wal|-shm)?$",
        ),
        (
            "file-read-data file-write*",
            r"/(\.docker|\.kube|\.azure|gcloud)(/|$)",
        ),
    ] {
        let insensitive: String = pattern
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() {
                    format!("[{c}{}]", c.to_ascii_uppercase())
                } else {
                    c.to_string()
                }
            })
            .collect();
        profile.push_str(&format!("(deny {operations} (regex #\"{insensitive}\"))\n"));
    }
    if profile.len() > 64 * 1024 {
        return Err(reject("host sandbox profile exceeds bounds"));
    }
    Ok(crate::sandbox::SandboxSpec::Seatbelt(profile))
}
