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
    // Test3/17 (Q19–Q21): project delegation also covers external files for
    // reading/creating/overwriting; these tools have no delete operation and
    // checked_path keeps other projects' policy/state and credentials closed.
    if crate::approval_mode::read(db, &ctx.project_id)?.mode
        != crate::approval_mode::ApprovalMode::Broad
        && crate::delegation::current(db, &ctx.project_id).is_none()
    {
        return Err(reject(
            "host access requires this project's owner-selected Broad access mode or project delegation",
        ));
    }
    Ok(())
}

/// One lower-cased path component naming credentials, policy or agent state.
fn protected_part(part: &str) -> bool {
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
        || lower.split('/').any(protected_part)
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
    if !matches!(
        name,
        "host_fs_read" | "host_fs_write" | "host_fs_trash" | "host_bash"
    ) {
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
        if name == "host_fs_trash" {
            trash_target(ctx, str_arg(input, "path")?)?;
        } else {
            path(
                ctx,
                str_arg(input, if name == "host_bash" { "cwd" } else { "path" })?,
                name == "host_bash",
            )?;
        }
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
    fn supports_reconcile(&self) -> bool {
        true
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

/// Fixed guidance appended to a failing delegated host shell result: the fence
/// silently refuses unlink/rename-away/rmdir, and a model that is not told why
/// burns correction rounds retrying `rm` (ticket 02 / spec T2).
pub(crate) const TRASH_GUIDANCE: &str = "Under project delegation this shell cannot delete, rename away or remove directories outside the project (deletion fence). To delete a local file or folder, call host_fs_trash with its absolute path; it moves the item to the system Trash where the owner can restore it.";

/// Entries walked before a directory trash gives up verifying (same bound as
/// the host shell's nested-project inventory): an unverifiable tree fails
/// closed rather than being trashed unchecked.
const TRASH_WALK_LIMIT: usize = 20000;

/// Top-level and system locations the trash tool never touches, beyond the
/// shared credential/policy/project rules of `checked_path`.
const SYSTEM_ROOTS: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/private/etc",
    "/Library",
    "/Applications",
    "/opt",
    "/cores",
    "/dev",
    "/Volumes",
];

/// Managed-followups ticket 02 (ADR0087 本机删除只进废纸篓): validate one
/// trash target. Same canonical rules as host_fs_write (absolute, outside the
/// current and any other Hexagon project, no credential/policy/agent state),
/// plus: it must exist and not be a symlink (the link, not its target, would
/// be the effect), it may not be a system root, the home directory, its
/// Library or the Trash itself, and a directory may not contain protected
/// state or a nested project anywhere below it. Cost model: a false refusal
/// costs one owner action in Finder; a false accept trashes credentials or a
/// whole project — the walk is bounded and fails closed when it is too large.
pub(crate) fn trash_target(ctx: &ToolContext, raw: &str) -> Result<PathBuf, ToolError> {
    let started = std::time::Instant::now();
    let result = (|| {
        if !Path::new(raw).is_absolute() {
            return Err(reject("host path must be absolute"));
        }
        let meta =
            std::fs::symlink_metadata(raw).map_err(|_| reject("trash target does not exist"))?;
        if meta.file_type().is_symlink() {
            return Err(reject(
                "symlinks are not moved to the Trash; trash the real path",
            ));
        }
        let resolved = checked_path(ctx, raw, false)?;
        let project = ctx.repo_root.canonicalize()?;
        if project.starts_with(&resolved) {
            return Err(reject(
                "the trash target cannot enclose the current project",
            ));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let home = home.as_deref();
        let forbidden = resolved.components().count() <= 2
            || SYSTEM_ROOTS.iter().any(|root| resolved.starts_with(root))
            || home.is_some_and(|home| {
                home.starts_with(&resolved)
                    || resolved.starts_with(home.join("Library"))
                    || resolved.starts_with(home.join(".Trash"))
            });
        if forbidden {
            return Err(reject("system, home and Trash locations are protected"));
        }
        if meta.is_dir() {
            let mut queue = vec![resolved.clone()];
            let mut seen = 0usize;
            while let Some(dir) = queue.pop() {
                for entry in std::fs::read_dir(&dir)? {
                    let entry = entry?;
                    seen += 1;
                    if seen > TRASH_WALK_LIMIT {
                        return Err(reject(
                            "trash target is too large to verify; trash narrower paths",
                        ));
                    }
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if protected_part(&name) || sensitive_file_path(&entry.path()) {
                        return Err(reject(
                            "trash target contains credentials, policy, agent state or a project",
                        ));
                    }
                    if entry.file_type()?.is_dir() {
                        queue.push(entry.path());
                    }
                }
            }
        }
        Ok(resolved)
    })();
    if result.is_err() {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "host_trash",
            "protected_or_invalid_target",
            started,
        );
    }
    result
}

/// The system Trash API keeps the original name and "Put Back" information.
/// It is a single rename on the item's own volume: on failure (missing
/// target, permission, a volume without Trash) nothing moved.
#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path) -> Result<PathBuf, ToolError> {
    use objc2_foundation::{NSFileManager, NSURL};
    let url = NSURL::from_file_path(path).ok_or_else(|| reject("invalid host path"))?;
    let mut out = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut out))
        .map_err(|error| {
            ToolError::NotExecuted(format!(
                "system Trash refused the item: {}",
                error.localizedDescription()
            ))
        })?;
    out.and_then(|url| url.to_file_path())
        .ok_or_else(|| ToolError::Exec("moved to the Trash but its new location is unknown".into()))
}

#[cfg(not(target_os = "macos"))]
fn move_to_trash(_: &Path) -> Result<PathBuf, ToolError> {
    // Spec T2: Windows Recycle Bin and Linux desktop trash are out of scope.
    Err(reject("host_fs_trash is only available on macOS"))
}

pub struct HostTrash;
impl Tool for HostTrash {
    fn name(&self) -> &str {
        "host_fs_trash"
    }
    fn description(&self) -> &str {
        "Use when a local file or folder outside the project must be deleted: moves it to the system Trash (macOS), where the owner can restore it. The only allowed way to delete local files under project delegation; shell deletion is fenced. Always asks for approval. Current/other Hexagon projects, credentials, agent policy, system and home locations are refused. Do not use for project files (use project tools) or symlinks."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Absolute path of an existing external file or folder to move to the system Trash."}},"required":["path"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::WriteLocal
    }
    fn precondition(&self, input: &Value, ctx: &ToolContext) -> Result<(), ToolError> {
        trash_target(ctx, str_arg(input, "path")?).map(|_| ())
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        mode(db, ctx)?;
        let target = trash_target(ctx, str_arg(input, "path")?)?;
        let trashed = move_to_trash(&target)?;
        // Ticket 02/05: the trace fact is the report's source — paths and
        // time only, never file contents.
        db.append_event(
            &ctx.project_id,
            crate::trace::EventKind::System,
            json!({"kind":"host_trashed","path":target,"trash_path":trashed,
                "action_id":ctx.action_key,
                "managed_actor":crate::delegation::current(db, &ctx.project_id).map(|grant| grant.actor())}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )
        .map_err(|error| ToolError::Exec(format!("moved to the Trash; trace not recorded: {error}")))?;
        Ok(json!({"trashed":target,"trash_path":trashed,"recoverable":true}))
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
        // Test3/23 (Q19–Q21): delegated decisions may edit external files but
        // never delete them; the owner's own Broad approval keeps its scope.
        let fenced = crate::delegation::current(db, &ctx.project_id).is_some();
        let spec = if fenced {
            match crate::sandbox::delegated_host_fence(spec, &scoped.repo_root) {
                crate::sandbox::SandboxSpec::Unavailable => {
                    return Err(reject(
                        "project delegation cannot fence external deletion on this platform",
                    ))
                }
                fenced => fenced,
            }
        } else {
            spec
        };
        let spec = crate::design::guard_shell(db, &scoped, spec);
        let timeout = std::time::Duration::from_millis(
            input["timeout_ms"]
                .as_u64()
                .unwrap_or(30000)
                .clamp(1, 600000),
        );
        let mut out = ctx.sessions.run_oneshot_with_spec(
            db,
            &scoped,
            str_arg(input, "cmd")?,
            timeout,
            (net, Some(spec), Some(ctx.repo_root.clone())),
        )?;
        // Only a failing command can have hit the fence; successful runs
        // stay unchanged (review: the hint on every result was noise).
        if fenced && out["exit_code"].as_i64() != Some(0) {
            out["deletion_fence"] = json!(TRASH_GUIDANCE);
        }
        Ok(out)
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
