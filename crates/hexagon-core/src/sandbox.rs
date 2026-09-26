//! 按风险类的 OS 沙箱（agent-senses 票 06 / ADR 0058-4）。
//!
//! 出处与代价模型：
//! - **双后端**：macOS `sandbox-exec`（seatbelt profile）、Linux `bwrap`。
//!   reliability 02 / Q3: missing or inexpressible isolation stops execution.
//!   False negative costs a manual intervention; false positive costs an unaudited
//!   side effect. Never fall back to an unsandboxed command.
//! - **seatbelt 的诚实边界**（ADR 0058-4）：macOS 沙箱网络是开关式
//!   （`(allow network*)`），没有 per-domain 粒度——域名级出网裁决永远
//!   在应用层 `permissions.rs` 做，OS 沙箱只是额外围笼。bash 的 `net`
//!   入参是唯一开关：默认关网，Agent 声明 `net:true` 才带网，且该声明
//!   进必问卡 raw_input 给负责人看。
//! - **终端漏斗**：终端命令的 `Command::new` 唯一出口是
//!   `sessions::sh_command(dir, spec)`——wrap 在里面发生，grep 可证。
//!   kill 辅助里的 `sh -c kill` 是宿主侧工具进程，不经漏斗（本就不该
//!   被沙箱）。reliability 05 的子代理 MCP 另由 Conn::spawn_with 调同一
//!   wrap_command，禁止写入/联网，不复用父 MCP 会话。
//! - owned_globs 仅支持精确字面路径或 directory/**；无法精确表达
//!   的通配和符号链接前缀停止执行，不扩大授权。

// reliability 10: group killing alone misses setsid/double-fork descendants.
// Darwin SDK syscall.h: setpgid=82, setsid=147, posix_spawn=244. The latter
// can set a session in-kernel without executing syscall147 (XNU kern_exec.c).
// ENOSYS makes supported runtimes use fork/exec; those without a fallback fail
// explicitly rather than launching an uncontained service. Never drop these
// restrictions just to make a launcher work. Ordinary fork/exec remains allowed.
#[cfg(target_os = "macos")]
pub(crate) const PROCESS_GROUP_RULES: &str = "(deny syscall-unix (syscall-number 82) (syscall-number 147))\n(deny syscall-unix (syscall-number 244) (with errno 78))";

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一次执行要套的沙箱规格（由 `spec_for` 生成，`sessions` 消费）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxSpec {
    /// macOS seatbelt profile 文本（sandbox-exec -p）。
    Seatbelt(String),
    /// bwrap 参数向量（`--` 之前的部分）。
    Bwrap(Vec<String>),
    /// No usable backend or the requested scope cannot be enforced: execution denied.
    Unavailable,
}

/// IPC 三态（TopBar 徽章）：mode = seatbelt|bwrap|none。
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct SandboxStatus {
    pub mode: String,
    pub available: bool,
    pub note: String,
}

/// 状态探测（结果缓存：可用性在进程生命周期内视为不变）。
pub fn status() -> SandboxStatus {
    use std::sync::OnceLock;
    static S: OnceLock<SandboxStatus> = OnceLock::new();
    S.get_or_init(detect_status).clone()
}

fn detect_status() -> SandboxStatus {
    if cfg!(target_os = "macos") {
        if Path::new("/usr/bin/sandbox-exec").exists() {
            return SandboxStatus {
                mode: "seatbelt".into(),
                available: true,
                note: "writes confined to effective scope; protected paths denied; network gated per call".into(),
            };
        }
        return SandboxStatus {
            mode: "none".into(),
            available: false,
            note: "sandbox-exec missing — terminal execution blocked".into(),
        };
    }
    if cfg!(target_os = "linux") {
        let ok = Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        return if ok {
            SandboxStatus {
                mode: "bwrap".into(),
                // reliability 03 / Q3: root read-only binds still expose secrets.
                // Stop until this backend enforces the same sensitive-read policy.
                available: false,
                note: "bwrap detected, but sensitive-read isolation is not enforceable by the current backend; terminal execution blocked".into(),
            }
        } else {
            SandboxStatus {
                mode: "none".into(),
                available: false,
                note: "bwrap not found — terminal execution blocked".into(),
            }
        };
    }
    SandboxStatus {
        mode: "none".into(),
        available: false,
        note: "no sandbox backend on this platform".into(),
    }
}

/// 为一次执行生成沙箱规格。`net` = 本条命令的出网开关（bash `net`
/// 入参）；非空 owned_globs 收窄可写范围，不能叠加在整仓授权上。
pub fn spec_for(repo_root: &Path, owned_globs: &[String], net: bool) -> SandboxSpec {
    scoped_spec(repo_root, owned_globs, net, false)
}

pub fn read_only_spec(repo_root: &Path, net: bool) -> SandboxSpec {
    scoped_spec(repo_root, &[], net, true)
}

fn scoped_spec(
    repo_root: &Path,
    owned_globs: &[String],
    net: bool,
    read_only: bool,
) -> SandboxSpec {
    // reliability 02: a partial glob prefix used to grant more than the glob.
    // Reject scopes we cannot represent exactly rather than widening them.
    if owned_globs
        .iter()
        .any(|g| glob_write_prefix(repo_root, g).is_none())
    {
        return SandboxSpec::Unavailable;
    }
    let st = status();
    if !st.available {
        return SandboxSpec::Unavailable;
    }
    // canonicalize：seatbelt/bwrap 按真实路径判定——macOS 上 /var→/private/var
    // 等符号链接不解析会围错目录。原路径也保留在授权里（防链接形态写入）。
    let canon = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    for g in owned_globs {
        let Some(path) = glob_write_prefix(&canon, g) else {
            return SandboxSpec::Unavailable;
        };
        if !crate::tools::repo_path(&canon, path.to_string_lossy().as_ref())
            .is_ok_and(|resolved| resolved == path)
        {
            return SandboxSpec::Unavailable;
        }
    }
    // reliability 02 / Q3: a directory bind cannot exclude future protected
    // filenames. Reject this unrepresentable scope instead of claiming that
    // mounting today's credentials read-only also protects tomorrow's files.
    // Exact existing ordinary files remain representable with a read-only parent.
    if st.mode == "bwrap"
        && !read_only
        && (owned_globs.is_empty()
            || owned_globs.iter().any(|g| {
                crate::tools::is_credential_path(g)
                    || crate::tools::is_permission_rule_path(g)
                    || g.starts_with(".hexagon/")
                    || glob_write_prefix(&canon, g).is_none_or(|p| !p.is_file())
            }))
    {
        return SandboxSpec::Unavailable;
    }
    match st.mode.as_str() {
        "seatbelt" => {
            let Ok(aliases) = hardlink_paths(&read_roots(&canon)) else {
                return SandboxSpec::Unavailable;
            };
            let mut profile = seatbelt_profile(repo_root, &canon, owned_globs, net, read_only);
            for path in aliases {
                profile.push_str(&format!(
                    "(deny file-read-data file-write* (literal \"{}\"))\n",
                    sbq(&path)
                ));
            }
            if profile.len() > 64 * 1024 {
                return SandboxSpec::Unavailable;
            }
            SandboxSpec::Seatbelt(profile)
        }
        "bwrap" => SandboxSpec::Bwrap(bwrap_args(repo_root, &canon, owned_globs, net, read_only)),
        _ => SandboxSpec::Unavailable,
    }
}

/// 把已构造的 Command 包进沙箱。Unavailable 拒绝启动。重建 Command 会丢 process_group——这里统一补回（本模块
/// 所有消费者都要进程组语义做 kill_tree）。
pub fn wrap_command(cmd: &mut Command, spec: &SandboxSpec) -> std::io::Result<Command> {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cwd = cmd.get_current_dir().map(PathBuf::from);
    let envs: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|x| x.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let mut c = match spec {
        SandboxSpec::Seatbelt(profile) => {
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(profile).arg(&prog).args(&args);
            c
        }
        SandboxSpec::Bwrap(pre) => {
            let mut c = Command::new("bwrap");
            c.args(pre).arg("--").arg(&prog).args(&args);
            c
        }
        SandboxSpec::Unavailable => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "terminal execution blocked: isolation unavailable or scope unsupported",
            ));
        }
    };
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    // reliability 03: terminal children must not inherit the host's model keys,
    // startup injection variables, or credential-provider sockets.
    const SAFE_ENV: &[&str] = &[
        "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "TMPDIR",
    ];
    c.env_clear();
    for key in SAFE_ENV {
        if let Some(value) = std::env::var_os(key) {
            c.env(key, value);
        }
    }
    for (k, v) in envs
        .into_iter()
        .filter(|(k, _)| SAFE_ENV.contains(&k.as_str()))
    {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    #[cfg(target_os = "macos")]
    if let Some(developer) = developer_directory() {
        // reliability 03: Apple's /usr/bin/git shim invokes xcrun, which writes
        // host caches and loads unrelated Xcode frameworks. Use the selected
        // toolchain's actual executables inside the existing read/write scope.
        let current = c
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
            .unwrap_or_default();
        let paths =
            std::iter::once(developer.join("usr/bin")).chain(std::env::split_paths(&current));
        c.env(
            "PATH",
            std::env::join_paths(paths).map_err(std::io::Error::other)?,
        );
    }
    // reliability 03: global Git configuration may carry credential material.
    // Local repository configuration stays readable; do not expand host reads
    // merely to satisfy Git's optional global configuration lookup.
    c.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    Ok(c)
}

#[cfg(target_os = "macos")]
fn developer_directory() -> Option<&'static PathBuf> {
    static DEVELOPER: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DEVELOPER
        .get_or_init(|| {
            let output = Command::new("/usr/bin/xcode-select")
                .arg("-p")
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            PathBuf::from(String::from_utf8(output.stdout).ok()?.trim())
                .canonicalize()
                .ok()
        })
        .as_ref()
}

fn read_roots(repo: &Path) -> Vec<PathBuf> {
    let mut roots = vec![repo.to_path_buf()];
    roots.extend(
        [
            "/System/Library",
            "/usr/lib",
            "/usr/bin",
            "/bin",
            "/sbin",
            "/dev",
            "/opt/homebrew",
            "/usr/local",
            // Cargo links the system TLS runtime, which reads openssl.cnf at startup.
            "/private/etc/ssl",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    #[cfg(target_os = "macos")]
    if let Some(developer) = developer_directory() {
        roots.push(developer.join("usr"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.extend(
            [".rustup", ".cargo/bin", ".cargo/registry", ".cargo/git"]
                .into_iter()
                .map(|rel| Path::new(&home).join(rel)),
        );
    }
    roots
}

// reliability 03: a credential can already have an innocuous hardlink name.
// Deny every ambiguous regular file, including ignored/hidden entries; refusing
// an incomplete inventory costs one terminal run, missing an alias leaks bytes.
fn hardlink_paths(roots: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    // System-volume runtime trees are immutable. Data-volume runtimes must be
    // inventoried too: otherwise ~/.cargo/registry is an external alias bypass.
    let mut pending: Vec<_> = roots
        .iter()
        .filter(|p| {
            ![
                "/System/Library",
                "/usr/lib",
                "/usr/bin",
                "/bin",
                "/sbin",
                "/dev",
            ]
            .iter()
            .any(|fixed| p == &Path::new(fixed))
                && p.exists()
        })
        .cloned()
        .collect();
    let mut aliases = Vec::new();
    let mut visited = 0;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            visited += 1;
            if visited > 500_000 {
                return Err(std::io::Error::other("read isolation inventory limit"));
            }
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() > 1 {
                        aliases.push(path);
                    }
                }
            }
        }
    }
    Ok(aliases)
}

// ---------- seatbelt ----------

/// seatbelt 授权串的引号转义（路径进 "…" 字面量）。
fn sbq(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Exact literal or directory/** scope only; unsupported patterns must fail closed.
fn glob_write_prefix(repo: &Path, glob: &str) -> Option<PathBuf> {
    let literal = glob.strip_suffix("/**").unwrap_or(glob);
    if literal.is_empty()
        || Path::new(literal).is_absolute()
        || literal
            .split('/')
            .any(|s| s == ".." || s == "." || s.is_empty())
        || literal.contains(['*', '?', '[', ']'])
    {
        return None;
    }
    Some(repo.join(literal))
}

/// macOS profile: writable roots are exact scopes, never their temporary ancestors.
/// reliability 03: credential name and configured-store rules also deny reads.
fn seatbelt_profile(
    repo: &Path,
    canon: &Path,
    globs: &[String],
    net: bool,
    read_only: bool,
) -> String {
    // reliability 05: unrestricted signals let a claimed read-only MCP kill
    // host/parent processes. Only processes in the same sandbox may be signalled.
    let mut prof = String::from(
        "(version 1)\n(deny default)\n(allow process*)\n(allow signal (target same-sandbox))\n(allow sysctl-read)\n(allow mach-lookup)\n(allow ipc-posix-shm)\n(allow file-read-metadata)\n(allow file-ioctl)\n(allow file-write* (literal \"/dev/null\") (literal \"/dev/tty\") (literal \"/dev/ptmx\"))\n",
    );
    // reliability 03: global data reads let an external ordinary-name hardlink
    // bypass the repository inventory. Only project data and runtime locations
    // are readable; metadata remains available for ordinary process startup.
    prof.push_str("(allow file-read-data (literal \"/\"))\n");
    for root in read_roots(canon) {
        prof.push_str(&format!(
            "(allow file-read-data (subpath \"{}\"))\n",
            sbq(&root)
        ));
    }
    // reliability 03: inventory covers existing aliases; prohibit creating new
    // hardlinks after that inventory instead of relying on filename filtering.
    prof.push_str("(deny file-link)\n");
    // reliability 02: global /tmp write grants also granted temporary repositories
    // in full. No ancestor of a repository may be a blanket writable exception.
    let mut subs: Vec<String> = Vec::new();
    if globs.is_empty() && !read_only {
        subs.push(sbq(repo));
        if canon != repo {
            subs.push(sbq(canon));
        }
    }
    for g in globs.iter().filter(|_| !read_only) {
        if let Some(p) = glob_write_prefix(repo, g) {
            let s = sbq(&p);
            if !subs.contains(&s) {
                subs.push(s);
            }
        }
        if canon != repo {
            if let Some(p) = glob_write_prefix(canon, g) {
                let s = sbq(&p);
                if !subs.contains(&s) {
                    subs.push(s);
                }
            }
        }
    }
    for s in subs {
        let tree = globs.is_empty()
            || globs.iter().any(|g| {
                g.ends_with("/**")
                    && [repo, canon]
                        .iter()
                        .any(|r| glob_write_prefix(r, g).is_some_and(|p| sbq(&p) == s))
            });
        let filter = if tree { "subpath" } else { "literal" };
        prof.push_str(&format!("(allow file-write* ({filter} \"{s}\"))\n"));
    }
    for root in [repo, canon] {
        prof.push_str(&format!(
            "(deny file-write* (subpath \"{}\"))\n",
            sbq(&root.join(".hexagon"))
        ));
    }
    // reliability 02 / D02: deny names even if created after profile generation;
    // enumerating existing files alone permits replacing policy via a new path.
    // Character classes keep the deny case-insensitive like structured tools.
    for pattern in [
        r"/(\.env[^/]*|[^/]*\.(pem|key)|id_rsa[^/]*|id_ed25519[^/]*|credentials(\.json)?|\.?netrc|mcp\.json|\.npmrc|\.pypirc)(/|$)",
        r"/(\.ssh|\.aws)(/|$)",
        r"[^/]*(permission_rules|keychain)[^/]*(/|$)",
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
        prof.push_str(&format!("(deny file-write* (regex #\"{insensitive}\"))\n"));
        if !pattern.contains("permission_rules") {
            prof.push_str(&format!(
                "(deny file-read-data (regex #\"{insensitive}\"))\n"
            ));
        } else {
            prof.push_str("(deny file-read-data (regex #\"[kK][eE][yY][cC][hH][aA][iI][nN]\"))\n");
        }
    }
    if let Some(path) = crate::credentials::active_file_path() {
        let path = path.canonicalize().unwrap_or(path);
        prof.push_str(&format!(
            "(deny file-read-data file-write* (literal \"{}\"))\n",
            sbq(&path)
        ));
    }
    // macOS shipped application.sb names these keychain IPC endpoints. Denying
    // files alone does not prevent security(1) from asking the credential daemon.
    // 2026-09-26 regression: denying self process-info makes a clean-environment
    // child trap during startup. Deny inspection of other processes (including
    // host environment), while preserving the child's own runtime queries.
    prof.push_str("(deny mach-lookup (global-name \"com.apple.securityd.xpc\") (global-name \"com.apple.SecurityServer\"))\n(deny process-info* (target others))\n");
    if net {
        // ADR 0058-4：seatbelt 无 per-domain——开就是全网。
        // 域名裁决在 permissions.rs；这里只是围笼开关。
        prof.push_str("\n(allow network*)\n");
    }
    prof
}

// ---------- bwrap ----------

fn bwrap_args(
    repo: &Path,
    canon: &Path,
    globs: &[String],
    net: bool,
    read_only: bool,
) -> Vec<String> {
    let mut a: Vec<String> = [
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/",
        "/",
        "--proc",
        "/proc",
        "--dev-bind",
        "/dev",
        "/dev",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rw = |a: &mut Vec<String>, p: &Path| {
        a.push("--bind".into());
        a.push(p.to_string_lossy().into_owned());
        a.push(p.to_string_lossy().into_owned());
    };
    if globs.is_empty() && !read_only {
        rw(&mut a, canon);
    }
    for g in globs.iter().filter(|_| !read_only) {
        if let Some(p) = glob_write_prefix(repo, g) {
            rw(&mut a, &p);
        }
    }
    let protected = canon.join(".hexagon");
    if protected.exists() {
        a.extend([
            "--ro-bind".into(),
            protected.to_string_lossy().into_owned(),
            protected.to_string_lossy().into_owned(),
        ]);
    }
    if !net {
        a.push("--unshare-net".into());
    }
    a.push("--chdir".into());
    a.push(repo.to_string_lossy().into_owned());
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn subs_of(profile: &str) -> Vec<String> {
        profile
            // reliability 03: runtime directories grant data reads, not writes.
            .lines()
            .filter(|line| line.starts_with("(allow file-write*"))
            .flat_map(|line| line.split("(subpath \"").skip(1))
            .filter_map(|s| s.split('"').next().map(String::from))
            .collect()
    }

    proptest! {
        // reliability 03: a runtime tree cannot be a less protected data source
        // than the repository. An ordinary alias in either tree stays denied.
        #[cfg(unix)]
        #[test]
        fn inventory_covers_every_mutable_read_root(name in "[a-z]{1,12}") {
            let repo = tempfile::tempdir().unwrap();
            let runtime = tempfile::tempdir().unwrap();
            let secret = repo.path().join(".env");
            let alias = runtime.path().join(name);
            std::fs::write(&secret, "synthetic").unwrap();
            std::fs::hard_link(&secret, &alias).unwrap();
            let roots = [repo.path().to_path_buf(), runtime.path().to_path_buf()];
            let denied = hardlink_paths(&roots).unwrap();
            prop_assert!(denied.contains(&secret));
            prop_assert!(denied.contains(&alias));
        }

        /// D12 不变量一：任意 owned_globs 生成的 profile，subpath 授权
        /// 只落在 repo 内或固定临时/设备白名单——永不授权 repo 外路径。
        #[test]
        fn profile_never_grants_outside_repo(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..6),
            net in any::<bool>(),
        ) {
            let repo = Path::new("/repo/proj");
            let p = seatbelt_profile(repo, repo, &globs, net, false);
            for s in subs_of(&p) {
                let ok = s.starts_with("/repo/proj")
                    || ["/tmp", "/private/tmp", "/private/var/folders"]
                        .iter()
                        .any(|w| s.starts_with(w));
                prop_assert!(ok, "grant outside repo: {s}\n{p}");
            }
        }

        /// D12 不变量二：net=false 时 profile 不含 network 放行；
        /// deny default 在 ⇒ 网络被围死。
        #[test]
        fn no_net_means_no_network_allow(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..4),
        ) {
            let repo = Path::new("/repo/proj");
            let p = seatbelt_profile(repo, repo, &globs, false, false);
            prop_assert!(!p.contains("network"), "net=false leaked network:\n{p}");
            prop_assert!(p.contains("(deny default)"));
            let p2 = seatbelt_profile(repo, repo, &globs, true, false);
            prop_assert!(p2.contains("(allow network*)"));
        }

        /// bwrap 同口径：net=false 必有 --unshare-net。
        #[test]
        fn bwrap_net_toggle(
            globs in proptest::collection::vec("[a-zA-Z0-9._/*?-]{0,40}", 0..4),
            net in any::<bool>(),
        ) {
            let repo = Path::new("/repo/proj");
            let a = bwrap_args(repo, repo, &globs, net, false);
            prop_assert_eq!(a.contains(&"--unshare-net".to_string()), !net);
            // reliability 02: full repo write is only valid for empty ownership.
            prop_assert_eq!(a.windows(3).any(|w| w[0] == "--bind" && w[1] == "/repo/proj"), globs.is_empty());
        }
    }

    #[test]
    fn unavailable_never_returns_an_executable_command() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        assert!(wrap_command(&mut command, &SandboxSpec::Unavailable).is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn terminal_environment_does_not_forward_secret_variables() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(dir.path(), &[], false);
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s' \"$HX_SYNTHETIC_SECRET\"; printf ordinary",
            ])
            .current_dir(dir.path())
            .env("HX_SYNTHETIC_SECRET", "SYNTHETIC_HOST_SECRET");
        let out = wrap_command(&mut command, &spec).unwrap().output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"ordinary");
    }

    proptest! {
        // reliability 02: a narrower declared scope cannot inherit a writable
        // ancestor (especially the system temp directory containing the repo).
        #[test]
        fn explicit_scope_has_no_writable_repo_ancestor(name in "[a-z]{1,12}") {
            let repo = Path::new("/private/tmp/project");
            let profile = seatbelt_profile(repo, repo, &[format!("{name}/**")], false, false);
            for line in profile.lines().filter(|line| line.starts_with("(allow file-write*")) {
                prop_assert!(!line.contains("(subpath \"/private/tmp\")"));
                prop_assert!(!line.contains("(subpath \"/private/tmp/project\")"));
            }
        }
    }

    #[test]
    fn glob_prefix_rejects_escapes() {
        let repo = Path::new("/r");
        assert_eq!(
            glob_write_prefix(repo, "src/**"),
            Some(PathBuf::from("/r/src"))
        );
        assert_eq!(glob_write_prefix(repo, "docs/a/*.md"), None);
        assert_eq!(glob_write_prefix(repo, "/abs/**"), None);
        assert_eq!(glob_write_prefix(repo, "../out/**"), None);
        assert_eq!(glob_write_prefix(repo, "**"), None);
    }

    /// macOS 实测门控（票 06 验收）：沙箱内 repo 写成功、$HOME 写被
    /// 内核拒。只在有 sandbox-exec 的机器上跑。
    #[test]
    #[cfg(target_os = "macos")]
    fn seatbelt_confines_writes() {
        if !status().available {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let spec = spec_for(repo, &[], false);
        let inside = repo.join("ok.txt");
        let mut base = Command::new("sh");
        base.arg("-c")
            .arg(
                "echo hi > ok.txt; echo x > \"$HOME/sb_escape.txt\" 2>&1; echo code=$?; cat ok.txt",
            )
            .current_dir(repo);
        let out = wrap_command(&mut base, &spec).unwrap().output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(inside.exists(), "repo write blocked: {out:?}");
        assert!(
            stdout.contains("code=1"),
            "escape write not denied: {stdout}"
        );
        assert!(!Path::new(&format!("{}/sb_escape.txt", std::env::var("HOME").unwrap())).exists());
        // 关网探测：curl 应失败（不真发请求——连不存在的端口也是网络动作）
        let mut c2 = Command::new("sh");
        c2.arg("-c")
            .arg("nc -z -w1 127.0.0.1 1 2>/dev/null; echo net=$?")
            .current_dir(repo);
        let o2 = wrap_command(&mut c2, &spec).unwrap().output().unwrap();
        let s2 = String::from_utf8_lossy(&o2.stdout);
        assert!(!s2.contains("net=0"), "net open under net=false: {s2}");
    }
}
