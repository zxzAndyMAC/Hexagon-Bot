//! Test3/16 (spec 04, ADR 0082): a role verifies its real delivery entry in a
//! real browser. test3 failed three ways: Chrome was blocked by the outer
//! sandbox, a trailing `ls` turned that failure into exit 0, and a Node canvas
//! self-check was read as a browser check. Here the host runs the verified
//! Playwright Firefox runtime inside the outer terminal sandbox on the actual
//! entry, and each stage (launch, navigation, checks, typing) is recorded
//! separately; only all-pass is "verified". Missing evidence is "failed".
//! Chrome or a missing runtime is honestly "unverified" — never substituted.
use super::*;

const WATCHDOG_MS: u64 = 20_000;
const MARK: &str = "HEXAGON_VERIFY ";

/// The verified runtime: a directory with `node_modules/playwright` and
/// `browsers/` (the ADR 0082 fixture shape). Host configuration only.
pub(crate) fn browser_runtime() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("HEXAGON_BROWSER_RUNTIME")?);
    let dir = dir.canonicalize().ok()?;
    (dir.join("node_modules/playwright/index.mjs").is_file() && dir.join("browsers").is_dir())
        .then_some(dir)
}

/// Where the page is. File entries stay inside the project; HTTP entries are
/// loopback only (the project's own dev server). Cost model: a false refusal
/// costs one owner question; a false accept opens arbitrary local files or
/// lets the sandboxed browser reach the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Entry {
    File(std::path::PathBuf),
    Loopback(String),
}

pub(crate) fn parse_entry(root: &std::path::Path, raw: &str) -> Result<Entry, &'static str> {
    let lower = raw.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
    {
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = host.rsplit_once(':').map_or(host, |(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host
            }
        });
        return if matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            Ok(Entry::Loopback(raw.to_string()))
        } else {
            Err("http_entry_must_be_loopback")
        };
    }
    if lower.contains("://") || lower.starts_with("file:") {
        return Err("unsupported_scheme");
    }
    let path = std::path::Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("entry_outside_project");
    }
    if !matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html" | "htm")
    ) {
        return Err("entry_not_html");
    }
    let root = root.canonicalize().map_err(|_| "project_unavailable")?;
    let full = root
        .join(path)
        .canonicalize()
        .map_err(|_| "entry_missing")?;
    if !full.starts_with(&root) || full.starts_with(root.join(".hexagon")) {
        return Err("entry_outside_project");
    }
    Ok(Entry::File(full))
}

pub struct BrowserVerify;

impl Tool for BrowserVerify {
    fn name(&self) -> &str {
        "browser_verify"
    }
    fn description(&self) -> &str {
        r#"Verify the real delivery entry in a real browser (Firefox) run by the host inside the project sandbox.
- Use when: you must prove a page actually loads and behaves — never conclude this from a Node self-check, `ls`, or a script that only builds.
- entry: a project HTML file path (e.g. "ux/prototype.html") or a loopback URL (http://127.0.0.1:PORT/...).
- checks: [{selector, text?}] — each selector must exist (and contain text if given). type: [{selector, text}] types with the keyboard. keys: ["ArrowLeft", ...] presses keys on the page.
- Result status is "verified" only when launch, navigation and every check pass; otherwise "failed" or "unverified" with the reason. Chrome is not available in the sandbox and is reported as unverified."#
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "entry":{"type":"string"},
            "browser":{"type":"string","enum":["firefox","chrome"]},
            "checks":{"type":"array","items":{"type":"object","properties":{"selector":{"type":"string"},"text":{"type":"string"}},"required":["selector"]}},
            "type":{"type":"array","items":{"type":"object","properties":{"selector":{"type":"string"},"text":{"type":"string"}},"required":["selector","text"]}},
            "keys":{"type":"array","items":{"type":"string"}}},
            "required":["entry"]})
    }
    fn risk(&self) -> RiskClass {
        RiskClass::Exec
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let entry = parse_entry(&ctx.repo_root, str_arg(input, "entry")?)
            .map_err(|code| ToolError::BadInput(format!("browser entry refused: {code}")))?;
        if input["browser"] == "chrome" {
            return Ok(unverified("chrome_not_supported_in_sandbox", "Chrome cannot run inside the terminal sandbox; no Chrome result exists. Firefox is the verified browser."));
        }
        let Some(runtime) = browser_runtime() else {
            return Ok(unverified(
                "browser_runtime_unavailable",
                "No host-verified browser runtime is configured; the page was not opened.",
            ));
        };
        let scratch = std::env::temp_dir().join("hexagon-exec").join(format!(
            "{}-browser-{}",
            ctx.project_id,
            crate::tools::run_nonce()
        ));
        std::fs::create_dir_all(&scratch)
            .map_err(|e| ToolError::NotExecuted(format!("scratch: {e}")))?;
        let _cleanup = Cleanup(scratch.clone());
        // Firefox resolves its profile under TMPDIR; give it the canonical
        // path the sandbox rules name (/var is a symlink on macOS).
        let scratch = scratch
            .canonicalize()
            .map_err(|e| ToolError::NotExecuted(format!("scratch: {e}")))?;
        let url = match &entry {
            Entry::File(path) => format!("file://{}", path.display()),
            Entry::Loopback(url) => url.clone(),
        };
        let plan = json!({"url": url, "checks": input["checks"], "type": input["type"], "keys": input["keys"],
            "runtime": runtime, "watchdog_ms": WATCHDOG_MS});
        std::fs::write(scratch.join("verify.mjs"), script(&plan))
            .map_err(|e| ToolError::NotExecuted(format!("script: {e}")))?;
        let mut fenced = ctx.clone();
        fenced.exec_scratch = Some(scratch.clone());
        fenced.browser_runtime = Some((runtime, matches!(entry, Entry::Loopback(_))));
        let cmd = format!("node '{}'", scratch.join("verify.mjs").display());
        let out = ctx.sessions.run_oneshot(
            db,
            &fenced,
            &cmd,
            std::time::Duration::from_millis(WATCHDOG_MS + 10_000),
            false,
        )?;
        let reaped = reap_descendants(&scratch);
        let mut result = evidence(&out);
        result["descendants_reaped"] = json!(reaped);
        Ok(result)
    }
}

/// Host guarantee (spec 04: no browser descendants after cancel/timeout):
/// every process still carrying this run's unique scratch (the Firefox
/// profile lives there) is killed. Returns how many had to be reaped.
fn reap_descendants(scratch: &std::path::Path) -> usize {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
    else {
        return 0;
    };
    let needle = scratch.to_string_lossy().into_owned();
    let mut reaped = 0;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let line = line.trim_start();
        let Some((pid, cmd)) = line.split_once(' ') else {
            continue;
        };
        let Ok(pid) = pid.parse::<i32>() else {
            continue;
        };
        if pid as u32 != std::process::id() && cmd.contains(&needle) {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            reaped += 1;
        }
    }
    reaped
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unverified(reason: &str, message: &str) -> Value {
    json!({"status":"unverified","reason":reason,"message":message,"stages":{}})
}

/// The evidence line decides the result; exit code and stderr are kept as
/// diagnostics and can never turn a failed stage into success.
pub(crate) fn evidence(out: &Value) -> Value {
    let stdout = out["stdout"].as_str().unwrap_or_default();
    let line = stdout.lines().rev().find_map(|l| l.strip_prefix(MARK));
    let Some(stages) = line.and_then(|l| serde_json::from_str::<Value>(l).ok()) else {
        return json!({"status":"failed","reason":"no_browser_evidence","exit_code":out["exit_code"],
            "stderr_tail": out["stderr"].as_str().unwrap_or_default().lines().rev().take(8).collect::<Vec<_>>()});
    };
    let all_ok = stages["launch"]["ok"] == true
        && stages["navigate"]["ok"] == true
        && stages["checks"]
            .as_array()
            .is_none_or(|c| c.iter().all(|c| c["ok"] == true))
        && stages["type"]
            .as_array()
            .is_none_or(|c| c.iter().all(|c| c["ok"] == true))
        && stages["keys"]["ok"] != false;
    let status = if all_ok && out["exit_code"] == 0 {
        "verified"
    } else {
        "failed"
    };
    json!({"status":status,"browser":"firefox","stages":stages,"exit_code":out["exit_code"]})
}

fn script(plan: &Value) -> String {
    format!(
        r#"const plan = {plan};
process.env.PLAYWRIGHT_BROWSERS_PATH = plan.runtime + '/browsers';
const stages = {{ launch: {{ ok: false }}, navigate: {{ ok: false }}, checks: [], type: [], keys: {{ ok: null }} }};
const report = (code) => {{ console.log('{MARK}' + JSON.stringify(stages)); process.exit(code); }};
let browser;
// Close the browser before exiting on timeout; a hard stop follows in 3 s.
const watchdog = setTimeout(() => {{
  stages.timeout = true;
  setTimeout(() => report(3), 3000).unref();
  (browser ? browser.close().catch(() => {{}}) : Promise.resolve()).finally(() => report(3));
}}, plan.watchdog_ms);
try {{
  const {{ firefox }} = await import(plan.runtime + '/node_modules/playwright/index.mjs');
  browser = await firefox.launch({{ headless: true, timeout: 10000 }});
  stages.launch = {{ ok: true }};
  const page = await browser.newPage();
  const response = await page.goto(plan.url, {{ timeout: 10000 }});
  stages.navigate = {{ ok: true, status: response ? response.status() : null, title: await page.title() }};
  for (const step of plan.type || []) {{
    try {{ await page.locator(step.selector).first().fill(''); await page.locator(step.selector).first().focus(); await page.keyboard.type(step.text);
      const value = await page.locator(step.selector).first().inputValue().catch(() => null);
      stages.type.push({{ selector: step.selector, ok: value === null || value.includes(step.text), value }});
    }} catch (e) {{ stages.type.push({{ selector: step.selector, ok: false, error: String(e.message || e).slice(0, 200) }}); }}
  }}
  if ((plan.keys || []).length) {{
    try {{ for (const key of plan.keys) await page.keyboard.press(key); stages.keys = {{ ok: true, pressed: plan.keys.length }}; }}
    catch (e) {{ stages.keys = {{ ok: false, error: String(e.message || e).slice(0, 200) }}; }}
  }}
  for (const check of plan.checks || []) {{
    const node = page.locator(check.selector).first();
    const count = await page.locator(check.selector).count();
    const text = count ? (await node.textContent()) || '' : null;
    stages.checks.push({{ selector: check.selector, ok: count > 0 && (check.text == null || (text || '').includes(check.text)), found: count, text: text && text.slice(0, 200) }});
  }}
}} catch (e) {{
  const stage = stages.launch.ok ? 'navigate' : 'launch';
  stages[stage] = {{ ok: false, error: String(e.message || e).slice(0, 1500) }};
}} finally {{
  clearTimeout(watchdog);
  if (browser) await browser.close().catch(() => {{}});
}}
const failed = !stages.launch.ok || !stages.navigate.ok || stages.checks.some(c => !c.ok) || stages.type.some(t => !t.ok) || stages.keys.ok === false;
report(failed ? 1 : 0);
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn entries_stay_in_the_project_or_on_loopback() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("ux")).unwrap();
        std::fs::write(dir.path().join("ux/page.html"), "<p>x</p>").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert!(matches!(
            parse_entry(dir.path(), "ux/page.html"),
            Ok(Entry::File(_))
        ));
        assert!(matches!(
            parse_entry(dir.path(), "http://127.0.0.1:5173/"),
            Ok(Entry::Loopback(_))
        ));
        assert!(matches!(
            parse_entry(dir.path(), "http://localhost/app"),
            Ok(Entry::Loopback(_))
        ));
        for (bad, code) in [
            ("../outside.html", "entry_outside_project"),
            ("/etc/passwd.html", "entry_outside_project"),
            ("file:///etc/passwd", "unsupported_scheme"),
            ("https://example.com/", "http_entry_must_be_loopback"),
            ("http://127.0.0.1.evil.test/", "http_entry_must_be_loopback"),
            ("notes.txt", "entry_not_html"),
            ("missing.html", "entry_missing"),
        ] {
            assert_eq!(parse_entry(dir.path(), bad), Err(code), "{bad}");
        }
    }

    #[test]
    fn evidence_decides_and_trailing_success_cannot_mask_failure() {
        let failed = json!({"exit_code":0,"stdout":format!("{MARK}{}\nls ok", json!({"launch":{"ok":false,"error":"blocked"},"navigate":{"ok":false},"checks":[],"type":[],"keys":{"ok":null}}))});
        assert_eq!(evidence(&failed)["status"], "failed");
        let missing = json!({"exit_code":0,"stdout":"index.html\n"});
        assert_eq!(evidence(&missing)["status"], "failed");
        assert_eq!(evidence(&missing)["reason"], "no_browser_evidence");
        let ok = json!({"exit_code":0,"stdout":format!("{MARK}{}", json!({"launch":{"ok":true},"navigate":{"ok":true},"checks":[{"ok":true}],"type":[],"keys":{"ok":null}}))});
        assert_eq!(evidence(&ok)["status"], "verified");
    }

    proptest! {
        // Test3/16: any failed stage, or a non-zero exit, is never "verified".
        #[test]
        fn verified_needs_every_stage(launch in any::<bool>(), nav in any::<bool>(), check in any::<bool>(), typed in any::<bool>(), exit in 0i64..2) {
            let stages = json!({"launch":{"ok":launch},"navigate":{"ok":nav},"checks":[{"ok":check}],"type":[{"ok":typed}],"keys":{"ok":null}});
            let out = json!({"exit_code":exit,"stdout":format!("{MARK}{stages}")});
            let verified = evidence(&out)["status"] == "verified";
            prop_assert_eq!(verified, launch && nav && check && typed && exit == 0);
        }
    }
}
