//! Test3/15 (spec 03, Q5/Q10, ADR 0084): host-controlled public dependency
//! fetch, then an offline project install. Probed on real processes:
//! .scratch/test3-integrated-2026-10-09/15-fetch-probe.md.
//!
//! - Fetch side: an empty host scratch with a host-generated manifest, a fixed
//!   public registry, empty npm configs and `--ignore-scripts`. Its sandbox can
//!   read only runtimes, the scratch and the cache — never project source,
//!   credentials or project configuration — and is the only side with network.
//! - Outbound metadata is package names, exact versions and the registry host
//!   (plus npm's own user agent); never project content. Not "zero egress".
//! - Install side: the project install runs offline under the 14 deletion fence
//!   (only `node_modules` may be removed). npm leaves unapproved install
//!   scripts unrun and lists them; we report that instead of guessing.
//! - Unsupported (git/file/tarball URLs, ranges, tags, aliases, private
//!   registries, global installs) is refused explicitly — never a silent
//!   fallback to a bare shell or `net: true`.
use super::*;

pub const REGISTRY: &str = "https://registry.npmjs.org/";
const REGISTRY_HOST: &str = "registry.npmjs.org";
const MAX_PACKAGES: usize = 20;
const MAX_FETCH_BYTES: u64 = 512 * 1024 * 1024;
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Validate one `name@exact-version` request. Cost model: a false refusal
/// costs one owner question; a false accept could smuggle text (URLs, paths,
/// queries) into the outbound request, so only the npm name grammar and an
/// exact semver pass.
pub(crate) fn parse_spec(spec: &str) -> Result<(String, String), &'static str> {
    let at = spec
        .rfind('@')
        .filter(|i| *i > 0)
        .ok_or("exact_version_required")?;
    let (name, version) = (&spec[..at], &spec[at + 1..]);
    let segment = |s: &str| {
        !s.is_empty()
            && s.len() <= 214
            && s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-._~".contains(c))
    };
    let name_ok = match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, pkg)| segment(scope) && segment(pkg)),
        None => segment(name),
    };
    if !name_ok {
        return Err("invalid_package_name");
    }
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    let exact = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c));
    if !exact {
        return Err("exact_version_required");
    }
    Ok((name.to_string(), version.to_string()))
}

pub struct DependencyFetch;

impl Tool for DependencyFetch {
    fn name(&self) -> &str {
        "dependency_fetch"
    }
    fn description(&self) -> &str {
        r#"Fetch public npm packages through the host's controlled channel, then install them into this project offline.
- Use when: the project needs npm dependencies.
- Do not use: `npm install` with network in bash, git/file/tarball sources, private registries or global installs.
- Input: exact `name@version` specs only (e.g. "chalk@4.1.2"). Ranges, tags, git/file/tarball URLs, private registries and global installs are refused.
- Only package names, exact versions and the public registry host leave the machine; the fetch side cannot read project files.
- The install runs offline in the project. Install scripts npm does not run are listed in the result."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "packages":{"type":"array","items":{"type":"string"},"description":"exact npm specs, e.g. [\"chalk@4.1.2\"]"}},
            "required":["packages"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Egress
    }
    fn builtin_deny(&self, input: &Value, _ctx: &ToolContext) -> Option<String> {
        let specs = input["packages"].as_array()?;
        if specs.is_empty() || specs.len() > MAX_PACKAGES {
            return Some(format!("dependency_fetch takes 1–{MAX_PACKAGES} packages"));
        }
        specs.iter().find_map(|spec| match spec.as_str().map(parse_spec) {
            Some(Ok(_)) => None,
            Some(Err(code)) => Some(format!("unsupported package spec ({code}); use name@exact-version from the public npm registry")),
            None => Some("package specs must be strings".into()),
        })
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let specs: Vec<String> = input["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str())
            .map(|s| parse_spec(s).map(|(n, v)| format!("{n}@{v}")))
            .collect::<Result<_, _>>()
            .map_err(|code| ToolError::BadInput(format!("unsupported package spec: {code}")))?;
        let cache = dependency_cache()?;
        let scratch = std::env::temp_dir().join("hexagon-fetch").join(format!(
            "{}-{}",
            ctx.project_id,
            crate::tools::run_nonce()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)
            .map_err(|e| ToolError::NotExecuted(format!("scratch: {e}")))?;
        let fetched = fetch(&scratch, &cache, &specs);
        let _cleanup = scopeguard(&scratch);
        let fetched = fetched?;
        let install = install(db, ctx, &cache, &specs)?;
        Ok(json!({
            "ecosystem": "npm",
            "registry": REGISTRY,
            "outbound_metadata": ["package names", "exact versions", "registry host", "npm user agent"],
            "fetched": fetched,
            "install": install,
        }))
    }
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scopeguard(path: &std::path::Path) -> Cleanup {
    Cleanup(path.to_path_buf())
}

/// Host-wide cache of public packages (content-addressed by npm).
fn dependency_cache() -> Result<std::path::PathBuf, ToolError> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| ToolError::NotExecuted("HOME unavailable".into()))?;
    let cache = std::path::Path::new(&home).join(".hexagon/cache/npm");
    std::fs::create_dir_all(&cache).map_err(|e| ToolError::NotExecuted(format!("cache: {e}")))?;
    cache
        .canonicalize()
        .map_err(|e| ToolError::NotExecuted(format!("cache: {e}")))
}

fn dir_bytes(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match entry.metadata() {
                Ok(m) if m.is_dir() => stack.push(entry.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// Fetch side: resolve and download into the cache with no project access.
fn fetch(
    scratch: &std::path::Path,
    cache: &std::path::Path,
    specs: &[String],
) -> Result<Vec<Value>, ToolError> {
    let scratch = scratch
        .canonicalize()
        .map_err(|e| ToolError::NotExecuted(format!("scratch: {e}")))?;
    std::fs::write(
        scratch.join("package.json"),
        r#"{"name":"hexagon-fetch","version":"0.0.0","private":true}"#,
    )
    .map_err(|e| ToolError::NotExecuted(format!("manifest: {e}")))?;
    for name in [".npmrc-user", ".npmrc-global"] {
        std::fs::write(scratch.join(name), "")
            .map_err(|e| ToolError::NotExecuted(format!("npmrc: {e}")))?;
    }
    let spec = crate::sandbox::fetch_spec(&scratch, cache);
    let before = dir_bytes(cache);
    let mut cmd = std::process::Command::new("npm");
    cmd.current_dir(&scratch)
        .args(["install", "--ignore-scripts", "--no-audit", "--no-fund"])
        .args(specs);
    let mut cmd = crate::sandbox::wrap_command(&mut cmd, &spec)
        .map_err(|e| ToolError::NotExecuted(format!("fetch isolation unavailable: {e}")))?;
    cmd.env("HOME", &scratch)
        .env("npm_config_cache", cache)
        .env("npm_config_userconfig", scratch.join(".npmrc-user"))
        .env("npm_config_globalconfig", scratch.join(".npmrc-global"))
        .env("npm_config_registry", REGISTRY)
        .env("npm_config_update_notifier", "false");
    let output = run_bounded(cmd)?;
    if !output.status.success() {
        return Err(ToolError::Exec(format!(
            "dependency fetch failed: {}",
            tail(&String::from_utf8_lossy(&output.stderr))
        )));
    }
    if dir_bytes(cache).saturating_sub(before) > MAX_FETCH_BYTES {
        return Err(ToolError::Exec(
            "dependency fetch exceeded the size limit".into(),
        ));
    }
    verify_lock(&scratch.join("package-lock.json"))
}

/// Every resolved entry must come from the public registry with integrity.
fn verify_lock(path: &std::path::Path) -> Result<Vec<Value>, ToolError> {
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(path).map_err(|e| ToolError::Exec(format!("lockfile: {e}")))?,
    )
    .map_err(|e| ToolError::Exec(format!("lockfile: {e}")))?;
    let mut fetched = vec![];
    for (key, entry) in lock["packages"].as_object().into_iter().flatten() {
        if key.is_empty() {
            continue;
        }
        let resolved = entry["resolved"].as_str().unwrap_or_default();
        let from_registry = resolved
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            == Some(REGISTRY_HOST);
        if !from_registry || entry["integrity"].as_str().is_none_or(str::is_empty) {
            return Err(ToolError::Exec(format!(
                "rejected {key}: not a public registry package with integrity"
            )));
        }
        fetched.push(json!({
            "name": key.trim_start_matches("node_modules/"),
            "version": entry["version"],
            "integrity": entry["integrity"],
            "optional": entry["optional"] == true,
        }));
    }
    Ok(fetched)
}

/// Install side: offline project install under the deletion fence.
fn install(
    db: &Db,
    ctx: &ToolContext,
    cache: &std::path::Path,
    specs: &[String],
) -> Result<Value, ToolError> {
    let scratch = std::env::temp_dir().join("hexagon-exec").join(format!(
        "{}-install-{}",
        ctx.project_id,
        crate::tools::run_nonce()
    ));
    std::fs::create_dir_all(&scratch)
        .map_err(|e| ToolError::NotExecuted(format!("scratch: {e}")))?;
    let _cleanup = scopeguard(&scratch);
    // npm refuses one file loaded as both user and global config.
    for name in [".npmrc-user", ".npmrc-global"] {
        std::fs::write(scratch.join(name), "")
            .map_err(|e| ToolError::NotExecuted(format!("npmrc: {e}")))?;
    }
    let mut fenced = ctx.clone();
    fenced.exec_scratch = Some(scratch.clone());
    fenced.install_cache = Some(cache.to_path_buf());
    let quoted: Vec<String> = specs.iter().map(|s| format!("'{s}'")).collect();
    let cmd = format!(
        "npm_config_cache='{}' npm_config_offline=true npm_config_userconfig='{}' npm_config_globalconfig='{}' npm_config_update_notifier=false npm install --no-audit --no-fund {}",
        cache.display(),
        scratch.join(".npmrc-user").display(),
        scratch.join(".npmrc-global").display(),
        quoted.join(" ")
    );
    let out = ctx
        .sessions
        .run_oneshot(db, &fenced, &cmd, FETCH_TIMEOUT, false)?;
    let stderr = out["stderr"].as_str().unwrap_or_default();
    let skipped: Vec<String> = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("npm warn allow-scripts"))
        .map(str::trim)
        .filter(|l| l.contains('@') && l.contains('('))
        .map(str::to_string)
        .collect();
    Ok(json!({
        "exit_code": out["exit_code"],
        "offline": true,
        "skipped_install_scripts": skipped,
        "stderr_tail": tail(stderr),
    }))
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().rev().take(12).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Fetch runs outside the project shell table: bound it and own its group.
fn run_bounded(mut cmd: std::process::Command) -> Result<std::process::Output, ToolError> {
    use std::io::Read;
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::NotExecuted(format!("spawn npm: {e}")))?;
    let deadline = std::time::Instant::now() + FETCH_TIMEOUT;
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let mut stdout = child.stdout.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| ToolError::Exec(e.to_string()))?
        {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Exec("dependency fetch timed out".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    Ok(std::process::Output {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: reader.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn exact_public_specs_pass_and_everything_else_is_refused() {
        assert_eq!(
            parse_spec("chalk@4.1.2"),
            Ok(("chalk".into(), "4.1.2".into()))
        );
        assert!(parse_spec("@types/node@20.11.5").is_ok());
        assert!(parse_spec("esbuild@0.20.2-beta.1").is_ok());
        for bad in [
            "chalk",
            "chalk@^4.1.2",
            "chalk@latest",
            "chalk@4.x",
            "chalk@>=4 <5",
            "git+https://github.com/x/y.git",
            "file:../secret",
            "https://evil.test/a.tgz",
            "chalk@npm:evil@1.0.0",
            "Chalk@4.1.2",
            "../x@1.0.0",
            "chalk@4.1.2?q=src",
            "@scope@1.0.0",
            "a b@1.0.0",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad}");
        }
    }

    proptest! {
        // Test3/15: no accepted spec can carry URL, path, query or space
        // characters into the outbound request.
        #[test]
        fn accepted_specs_carry_no_request_syntax(s in "\\PC{0,40}") {
            if let Ok((name, version)) = parse_spec(&s) {
                for forbidden in [':', '?', '#', ' ', '\\', '%', '&', '='] {
                    prop_assert!(!name.contains(forbidden) && !version.contains(forbidden));
                }
                prop_assert!(!name.starts_with('.') && !name.starts_with('/'));
            }
        }
    }
}
