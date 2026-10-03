//! MCP 宿主（票 13）：每服务一个 stdio OS 子进程，项目级生命周期。
//!
//! - 传输：JSON-RPC 2.0 over stdio，UTF-8 JSON 单行消息（MCP 标准）。
//! - 握手：spawn → `initialize` → `notifications/initialized` → `tools/list`，
//!   发现的工具注册为 `mcp:<service>:<tool>`，走统一工具管线（权限照常求值；
//!   grants 表授权闸门在 permissions::evaluate 的 L0）。
//! - 传输中断不重发工具动作：响应丢失不代表副作用没有发生（可靠性 08/09）。
//! - 无常驻 Agent 进程；Host Drop 时全部 kill。

#[cfg(target_os = "macos")]
use crate::sandbox::PROCESS_GROUP_RULES;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::db::Db;
use crate::tools::{Tool, ToolContext, ToolError};

/// 一个 MCP 服务的启动规格（项目 `.hexagon/mcp.json` ∪ 全局 `~/.hexagon/mcp.json`；
/// global-config 票 05，ADR 0057：同名项目覆盖全局）。
///
/// 向后兼容：旧格式只有 name/command/args，其余字段 serde default。
/// `url` 非空 = 远程传输（sse/http）——一期不 spawn（传输层只有 stdio），
/// 但清单行照常显示（ticket 06 裁决：可见置灰 > 静默隐藏）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct McpSpec {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// 注入子进程的环境变量（可含密钥——本机明文文件，与 cursor/claude 惯例一致）。
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// 远程传输的请求标头（Authorization 等，可含密钥——与 env 同约定本机明文）。
    /// stdio 条目存而不用：外部配置里的 headers 原样保留，http 传输落地时消费。
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub url: Option<String>,
}

impl McpSpec {
    /// 可 spawn 的 stdio 服务（启用 + 有 command + 无 url）。
    fn spawnable(&self) -> bool {
        !self.disabled && self.url.is_none() && !self.command.is_empty()
    }
    /// 传输类标签（清单行展示用）。
    pub fn transport(&self) -> &'static str {
        if self.url.is_some() {
            "remote"
        } else {
            "stdio"
        }
    }
}

/// 全局 MCP 清单路径（~/.hexagon/mcp.json；HEXAGON_MCP_PATH 覆盖）。
/// cfg(not(test))：test build 下 runtime_global_path 走死路径，本函数会成
/// dead code——单测一律走 `*_at` 注入缝。
#[cfg(not(test))]
fn global_mcp_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("HEXAGON_MCP_PATH") {
        return std::path::PathBuf::from(p);
    }
    // 无 HOME = 永不存在的路径：读面按空清单处理（容忍式），写面会在
    // create_dir_all 处报系统错——与 roles/providers 的 NoHome 语义一致。
    std::env::var_os("HOME")
        .map(|h| std::path::Path::new(&h).join(".hexagon/mcp.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("/nonexistent/.hexagon/mcp.json"))
}

/// 运行时全局路径：cfg(test) 下指向不存在路径——并行 cargo test 无法靠 env
/// 做进程级隔离，api 测试 open 时绝不能 spawn 开发者本机 ~/.hexagon/mcp.json
/// 里的真服务（起进程 ≠ 读目录，技能扫描那只读没这问题）。单测覆盖全走
/// `*_at` 注入缝；生产路径不变。
fn runtime_global_path() -> std::path::PathBuf {
    #[cfg(test)]
    {
        std::path::PathBuf::from("/nonexistent/hexagon-test/mcp.json")
    }
    #[cfg(not(test))]
    {
        global_mcp_path()
    }
}

/// 容忍式解析：缺席/损坏/非数组 = 空清单（MCP 是可选项）。
fn read_specs_at(path: &std::path::Path) -> Vec<McpSpec> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return vec![];
    };
    serde_json::from_str::<Vec<McpSpec>>(&text).unwrap_or_default()
}

fn write_specs_at(path: &std::path::Path, specs: &[McpSpec]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(specs).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 服务名校验：进工具名 `mcp:<name>:<tool>`——禁冒号/空白/路径分隔/点号开头。
fn valid_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.starts_with('.')
}

// Reliability 22: configuration is host-held. All free-form launch arguments,
// environment/header values and paths may carry secrets, so do not guess keys.
fn display_spec(raw: &McpSpec) -> McpSpec {
    let mut s = raw.clone();
    s.command = crate::data_boundary::hidden(&s.command);
    s.args = s
        .args
        .iter()
        .map(|v| crate::data_boundary::hidden(v))
        .collect();
    for v in s.env.values_mut().chain(s.headers.values_mut()) {
        *v = crate::data_boundary::hidden(v);
    }
    s.cwd = s.cwd.as_deref().map(crate::data_boundary::hidden);
    s.url = s.url.as_deref().map(crate::data_boundary::hidden_url);
    s
}
fn restore_spec(input: &McpSpec, current: Option<&McpSpec>) -> Result<McpSpec, String> {
    use crate::data_boundary::restore;
    let mut s = input.clone();
    s.command = restore(&input.command, current.map(|c| c.command.as_str()), false)?;
    s.args = input
        .args
        .iter()
        .enumerate()
        .map(|(i, v)| {
            restore(
                v,
                current.and_then(|c| c.args.get(i)).map(String::as_str),
                false,
            )
        })
        .collect::<Result<_, _>>()?;
    for (key, value) in &mut s.env {
        *value = restore(
            value,
            current.and_then(|c| c.env.get(key)).map(String::as_str),
            false,
        )?;
    }
    for (key, value) in &mut s.headers {
        *value = restore(
            value,
            current.and_then(|c| c.headers.get(key)).map(String::as_str),
            false,
        )?;
    }
    s.cwd = input
        .cwd
        .as_deref()
        .map(|v| restore(v, current.and_then(|c| c.cwd.as_deref()), false))
        .transpose()?;
    s.url = input
        .url
        .as_deref()
        .map(|v| restore(v, current.and_then(|c| c.url.as_deref()), true))
        .transpose()?;
    Ok(s)
}

/// 新建/覆写全局 MCP 服务（upsert 按名；写 ~/.hexagon/mcp.json）。
/// 校验：名合法；stdio 必有 command，远程必有 url，二选一。
pub fn save_global_mcp(spec: &McpSpec) -> Result<(), String> {
    save_global_mcp_at(spec, &runtime_global_path())
}

fn save_global_mcp_at(spec: &McpSpec, path: &std::path::Path) -> Result<(), String> {
    if !valid_mcp_name(&spec.name) {
        return Err(format!("invalid mcp service name: {}", spec.name));
    }
    match (spec.command.is_empty(), spec.url.is_some()) {
        (true, false) => return Err("mcp service needs command or url".into()),
        (false, true) => return Err("mcp service: command and url are exclusive".into()),
        _ => {}
    }
    let mut specs = read_specs_at(path);
    let spec = restore_spec(spec, specs.iter().find(|s| s.name == spec.name))?;
    specs.retain(|s| s.name != spec.name);
    specs.push(spec);
    write_specs_at(path, &specs)
}

/// 删全局服务（幂等）。
pub fn delete_global_mcp(name: &str) -> Result<(), String> {
    delete_global_mcp_at(name, &runtime_global_path())
}

fn delete_global_mcp_at(name: &str, path: &std::path::Path) -> Result<(), String> {
    let mut specs = read_specs_at(path);
    specs.retain(|s| s.name != name);
    write_specs_at(path, &specs)
}

/// 运行时装载：全局 ∪ 项目（同名项目胜），只回可 spawn 的 stdio 服务。
/// 文件缺席/损坏 = 无服务（不报错——MCP 是可选项）。
pub fn load_specs(repo_root: &std::path::Path) -> Vec<McpSpec> {
    merged_specs_at(&runtime_global_path(), Some(repo_root))
        .into_iter()
        .filter(|s| s.spawnable())
        .collect()
}

/// 全局 ∪ 项目合并（项目同名覆盖全局）。`repo_root=None` = 无项目只看全局。
fn merged_specs_at(
    global_path: &std::path::Path,
    repo_root: Option<&std::path::Path>,
) -> Vec<McpSpec> {
    let mut merged: Vec<McpSpec> = read_specs_at(global_path);
    if let Some(root) = repo_root {
        for spec in read_specs_at(&root.join(".hexagon/mcp.json")) {
            merged.retain(|s| s.name != spec.name);
            merged.push(spec);
        }
    }
    merged
}

/// 设置页清单行（global-config 票 05）：全量条目含禁用/远程，带来源标。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct McpEntryRow {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub disabled: bool,
    /// "stdio" | "remote"（url 端点，一期不 spawn）
    pub transport: String,
    pub url: Option<String>,
    /// 远程标头值使用不透明保留标记；原文不返回 UI。
    pub headers: std::collections::BTreeMap<String, String>,
    /// "global" | "project"（同名项目覆盖全局——全局行被吞后不显示）
    pub origin: String,
}

/// 清单视图：全局 ∪ 项目（项目优先），含禁用与远程条目。
/// `repo_root=None` 时只列全局——设置分区无项目可用。
pub fn list_mcp_entries(repo_root: Option<&std::path::Path>) -> Vec<McpEntryRow> {
    list_mcp_entries_at(&runtime_global_path(), repo_root)
}

fn list_mcp_entries_at(
    global_path: &std::path::Path,
    repo_root: Option<&std::path::Path>,
) -> Vec<McpEntryRow> {
    let globals = read_specs_at(global_path);
    let projects: Vec<McpSpec> = repo_root
        .map(|r| read_specs_at(&r.join(".hexagon/mcp.json")))
        .unwrap_or_default();
    let shadowed: std::collections::HashSet<String> =
        projects.iter().map(|s| s.name.clone()).collect();
    let row = |raw: &McpSpec, origin: &str| {
        let s = display_spec(raw);
        McpEntryRow {
            name: s.name.clone(),
            command: s.command.clone(),
            args: s.args.clone(),
            env: s.env.clone(),
            cwd: s.cwd.clone(),
            disabled: s.disabled,
            transport: s.transport().into(),
            url: s.url.clone(),
            headers: s.headers.clone(),
            origin: origin.into(),
        }
    };
    globals
        .iter()
        .filter(|s| !shadowed.contains(&s.name))
        .map(|s| row(s, "global"))
        .chain(projects.iter().map(|s| row(s, "project")))
        .collect()
}

// ---------- 外部 MCP 扫描/导入（global-config 票 06） ----------

/// 主流平台 MCP 配置源表（数据驱动；平台文件缺席=跳过，不硬凑）。
/// fmt "json" = `{"mcpServers": {name: {…}}}`；"toml" = codex 的
/// `[mcp_servers.<name>]` 段。顺序=去重优先级（先扫到的赢）。
const EXT_MCP_SOURCES: &[(&str, &str, &str)] = &[
    ("cursor", ".cursor/mcp.json", "json"),
    ("claude", ".claude.json", "json"),
    (
        "claude-desktop",
        "Library/Application Support/Claude/claude_desktop_config.json",
        "json",
    ),
    ("windsurf", ".codeium/windsurf/mcp_config.json", "json"),
    ("gemini", ".gemini/settings.json", "json"),
    ("devin", ".config/devin/mcp.json", "json"),
    ("codex", ".codex/config.toml", "toml"),
];

/// 外部 MCP 扫描行。去重键 = 小写名 + 传输签名（command 或 url）——
/// 同名不同实现是两个服务都列出；同名同实现只列先扫到的。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExtMcpRow {
    /// Opaque binding to the scanned host-side configuration, used for import.
    pub reference: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub disabled: bool,
    /// "stdio" | "remote"
    pub transport: String,
    pub url: Option<String>,
    /// 远程标头值脱敏，导入通过 reference 在宿主重新读取。
    pub headers: std::collections::BTreeMap<String, String>,
    /// 来源平台（cursor/claude/codex/…）
    pub origin: String,
    /// 来源配置文件路径（透明性：让用户知道从哪读到的）
    pub source_path: String,
    /// 与本机全局清单同名——导入时跳过不覆盖
    pub conflict: bool,
}

/// 从单个 JSON 配置抽出 `mcpServers` map 条目。
fn specs_from_mcp_json(text: &str) -> Vec<McpSpec> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return vec![];
    };
    let Some(map) = v["mcpServers"].as_object() else {
        return vec![];
    };
    map.iter()
        .filter_map(|(name, e)| {
            if !e.is_object() {
                return None;
            }
            let url = e["url"]
                .as_str()
                .or_else(|| e["serverUrl"].as_str())
                .map(String::from);
            let command = e["command"].as_str().unwrap_or_default().to_string();
            // 命令与 url 双空 = 无效条目（数字/半截配置），滤掉不列
            if command.is_empty() && url.is_none() {
                return None;
            }
            Some(McpSpec {
                command,
                name: name.clone(),
                args: e["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                cwd: e["cwd"].as_str().map(String::from),
                env: e["env"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default(),
                headers: e["headers"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default(),
                disabled: e["disabled"].as_bool().unwrap_or(false),
                url,
            })
        })
        .collect()
}

/// codex `.codex/config.toml`：`[mcp_servers.<name>] command=… args=[…]`。
fn specs_from_mcp_toml(text: &str) -> Vec<McpSpec> {
    let Ok(v) = text.parse::<toml::Value>() else {
        return vec![];
    };
    let Some(map) = v.get("mcp_servers").and_then(|m| m.as_table()) else {
        return vec![];
    };
    map.iter()
        .map(|(name, e)| {
            let get = |k: &str| e.get(k);
            McpSpec {
                name: name.clone(),
                command: get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                args: get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                cwd: get("cwd").and_then(|v| v.as_str()).map(String::from),
                env: get("env")
                    .and_then(|v| v.as_table())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default(),
                headers: get("headers")
                    .and_then(|v| v.as_table())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default(),
                disabled: false,
                url: get("url").and_then(|v| v.as_str()).map(String::from),
            }
        })
        .collect()
}

/// 扫描入口（`home` 注入便于测试；线上用 HOME）。单源解析失败跳过不连坐。
fn scan_external_raw_at(home: &std::path::Path, global_path: &std::path::Path) -> Vec<ExtMcpRow> {
    let existing: std::collections::HashSet<String> = read_specs_at(global_path)
        .iter()
        .map(|s| s.name.to_lowercase())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (platform, rel, fmt) in EXT_MCP_SOURCES {
        let path = home.join(rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let specs = match *fmt {
            "toml" => specs_from_mcp_toml(&text),
            _ => specs_from_mcp_json(&text),
        };
        for spec in specs {
            let sig = format!(
                "{}|{}",
                spec.name.to_lowercase(),
                spec.url.clone().unwrap_or_else(|| spec.command.clone())
            );
            if !seen.insert(sig) {
                continue; // 同名同实现去重：表序先到的赢
            }
            out.push(ExtMcpRow {
                reference: String::new(),
                conflict: existing.contains(&spec.name.to_lowercase()),
                transport: spec.transport().into(),
                name: spec.name,
                command: spec.command,
                args: spec.args,
                env: spec.env,
                cwd: spec.cwd,
                disabled: spec.disabled,
                url: spec.url,
                headers: spec.headers,
                origin: platform.to_string(),
                source_path: path.to_string_lossy().to_string(),
            });
        }
    }
    out
}

fn external_spec(row: &ExtMcpRow) -> McpSpec {
    McpSpec {
        name: row.name.clone(),
        command: row.command.clone(),
        args: row.args.clone(),
        env: row.env.clone(),
        headers: row.headers.clone(),
        cwd: row.cwd.clone(),
        disabled: row.disabled,
        url: row.url.clone(),
    }
}
fn external_reference(row: &ExtMcpRow) -> String {
    crate::data_boundary::hidden(&serde_json::to_string(row).unwrap_or_default())
}
pub fn scan_external_mcp_at(home: &std::path::Path, global: &std::path::Path) -> Vec<ExtMcpRow> {
    scan_external_raw_at(home, global)
        .into_iter()
        .map(|mut row| {
            row.reference = external_reference(&row);
            let safe = display_spec(&external_spec(&row));
            row.command = safe.command;
            row.args = safe.args;
            row.env = safe.env;
            row.headers = safe.headers;
            row.cwd = safe.cwd;
            row.url = safe.url;
            row
        })
        .collect()
}

/// Re-read only known platform configuration sources. No secret passes through
/// UI import payloads; an edited source no longer matches its scan reference.
pub fn import_mcp_references_at(
    home: &std::path::Path,
    global: &std::path::Path,
    references: &[String],
) -> crate::skills::ImportReport {
    let rows = scan_external_raw_at(home, global);
    let mut specs = Vec::new();
    let mut stale = Vec::new();
    for reference in references {
        if let Some(row) = rows.iter().find(|r| {
            crate::data_boundary::matches_reference(
                reference,
                &serde_json::to_string(r).unwrap_or_default(),
            )
        }) {
            let mut spec = external_spec(row);
            spec.disabled |= spec.url.is_some();
            specs.push(spec);
        } else {
            stale.push("configuration changed; scan again".into());
        }
    }
    let mut report = import_mcp_at(global, &specs);
    report.skipped.extend(stale);
    report
}
pub fn import_mcp_references(references: &[String]) -> crate::skills::ImportReport {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return crate::skills::ImportReport {
            imported: 0,
            skipped: vec!["home unavailable".into()],
        };
    };
    import_mcp_references_at(&home, &runtime_global_path(), references)
}

pub fn scan_external_mcp() -> Vec<ExtMcpRow> {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return vec![];
    };
    scan_external_mcp_at(&home, &runtime_global_path())
}

/// 批量导入外部 MCP 服务 → 全局清单。同名冲突跳过不覆盖
/// （owner 已有配置优先——与技能导入同策略）。
pub fn import_mcp_at(
    global_path: &std::path::Path,
    specs: &[McpSpec],
) -> crate::skills::ImportReport {
    let mut rep = crate::skills::ImportReport {
        imported: 0,
        skipped: vec![],
    };
    let mut existing = read_specs_at(global_path);
    for spec in specs {
        if !valid_mcp_name(&spec.name) {
            rep.skipped.push(format!("{}: invalid name", spec.name));
            continue;
        }
        if existing.iter().any(|s| s.name == spec.name) {
            rep.skipped.push(format!("{}: conflict", spec.name));
            continue;
        }
        existing.push(spec.clone());
        rep.imported += 1;
    }
    if rep.imported > 0 {
        if let Err(e) = write_specs_at(global_path, &existing) {
            rep.skipped.push(format!("write failed: {e}"));
            rep.imported = 0;
        }
    }
    rep
}

pub fn import_mcp(specs: &[McpSpec]) -> crate::skills::ImportReport {
    import_mcp_at(&runtime_global_path(), specs)
}

// reliability 10: this is the sole owner/reaper of Child. Keep the unreaped
// handle until termination, so an exited child's PID cannot be recycled before
// its process group is killed. Never retain a naked PID for later shutdown.
struct ManagedProcess {
    child: Mutex<Option<Child>>,
}
impl ManagedProcess {
    fn terminate(&self) {
        let Some(mut child) = self.child.lock().unwrap().take() else {
            return;
        };
        crate::sessions::kill_pid_group(child.id());
        let _ = child.kill();
        let grace = Instant::now() + Duration::from_millis(200);
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if Instant::now() < grace => std::thread::sleep(Duration::from_millis(5)),
                _ => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return;
                }
            }
        }
    }
}
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn lifecycle_command(
    spec: &McpSpec,
    isolation: Option<&crate::sandbox::SandboxSpec>,
) -> Result<Command, ToolError> {
    let mut original = Command::new(&spec.command);
    original.args(&spec.args).envs(&spec.env);
    if let Some(cwd) = &spec.cwd {
        original.current_dir(cwd);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(isolation) = isolation {
            let crate::sandbox::SandboxSpec::Seatbelt(profile) = isolation else {
                return Err(ToolError::NotExecuted(
                    "mcp: process lifetime isolation unavailable".into(),
                ));
            };
            return crate::sandbox::wrap_command(
                &mut original,
                &crate::sandbox::SandboxSpec::Seatbelt(format!("{profile}\n{PROCESS_GROUP_RULES}")),
            )
            .map_err(|e| ToolError::NotExecuted(format!("mcp isolation: {e}")));
        }
        // Parent sessions keep their configured environment and external rights;
        // this additional boundary controls process lifetime only.
        let mut cmd = Command::new("/usr/bin/sandbox-exec");
        cmd.args([
            "-p",
            &format!("(version 1)(allow default)\n{PROCESS_GROUP_RULES}"),
            "--",
        ])
        .arg(&spec.command)
        .args(&spec.args)
        .envs(&spec.env);
        let node_options = spec
            .env
            .get("NODE_OPTIONS")
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("NODE_OPTIONS"));
        cmd.env(
            "NODE_OPTIONS",
            crate::sandbox::node_options(node_options.as_deref()),
        );
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        Ok(cmd)
    }
    #[cfg(target_os = "linux")]
    {
        if isolation.is_some() {
            return Err(ToolError::NotExecuted(
                "mcp: readonly isolation unavailable".into(),
            ));
        }
        // PID namespace descendants cannot escape into the host namespace;
        // namespace init death reaps detached sessions too. No unsandboxed retry.
        let mut cmd = Command::new("bwrap");
        cmd.args([
            "--unshare-pid",
            "--die-with-parent",
            "--bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--",
        ])
        .arg(&spec.command)
        .args(&spec.args)
        .envs(&spec.env);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        Ok(cmd)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (original, isolation);
        Err(ToolError::NotExecuted(
            "mcp: process lifetime isolation unavailable".into(),
        ))
    }
}

#[cfg(test)]
pub(crate) const TEST_PEER: &str = r#"
const rl=require('readline').createInterface({input:process.stdin});
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
rl.on('line',line=>{const r=JSON.parse(line);if(r.id===undefined)return;
if(r.method==='initialize')send(r.id,{protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'fake',version:'1'}});
else if(r.method==='tools/list')send(r.id,{tools:[{name:'echo',description:'echo args',inputSchema:{type:'object'}}]});
else if(r.method==='tools/call')send(r.id,{content:[{type:'text',text:JSON.stringify(r.params.arguments)}]});
});
"#;

struct Conn {
    process: Arc<ManagedProcess>,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: i64,
    broken: bool,
}

impl Conn {
    fn spawn(spec: &McpSpec) -> Result<Self, ToolError> {
        Self::spawn_with(spec, None)
    }

    fn spawn_with(
        spec: &McpSpec,
        isolation: Option<&crate::sandbox::SandboxSpec>,
    ) -> Result<Self, ToolError> {
        // reliability 05: command arguments may contain service credentials.
        log::info!("mcp spawn: {}", spec.name);
        let mut cmd = lifecycle_command(spec, isolation)?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::NotExecuted(format!("mcp spawn {}: {e}", spec.name)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ToolError::Exec("mcp stdin closed".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::Exec("mcp stdout closed".into()))?;
        Ok(Self {
            process: Arc::new(ManagedProcess {
                child: Mutex::new(Some(child)),
            }),
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            broken: false,
        })
    }

    fn handshake(&mut self) -> Result<Vec<Value>, ToolError> {
        let started = std::time::Instant::now();
        let init = self.rpc(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name":"hexagon", "version":"0.1"}
            }),
        )?;
        if let Err(reason) = validate_initialize(&init) {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                None,
                None,
                None,
                None,
                "mcp_initialize",
                reason,
                started,
            );
            return Err(ToolError::Exec(reason.into()));
        }
        self.notify("notifications/initialized")?;
        let mut tools = Vec::new();
        let mut cursor = None;
        for _ in 0..64 {
            let res = self.rpc(
                "tools/list",
                cursor.as_ref().map_or(json!({}), |c| json!({"cursor":c})),
            )?;
            let page = res["tools"]
                .as_array()
                .ok_or_else(|| ToolError::Exec("mcp: invalid tools list".into()))?;
            for tool in page {
                if tool["name"].as_str().is_none_or(str::is_empty)
                    || !tool["inputSchema"].is_object()
                    || tools
                        .iter()
                        .any(|existing: &Value| existing["name"] == tool["name"])
                {
                    return Err(ToolError::Exec(
                        "mcp: invalid or duplicate tool definition".into(),
                    ));
                }
                tools.push(tool.clone());
                if tools.len() > 10_000 {
                    return Err(ToolError::Exec("mcp: too many tools".into()));
                }
            }
            match res.get("nextCursor") {
                None => return Ok(tools),
                Some(Value::String(next)) if !next.is_empty() && cursor.as_ref() != Some(next) => {
                    cursor = Some(next.clone())
                }
                _ => return Err(ToolError::Exec("mcp: invalid pagination cursor".into())),
            }
        }
        Err(ToolError::Exec("mcp: too many tools pages".into()))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        if self.broken {
            return Err(ToolError::NotExecuted("mcp: session unavailable".into()));
        }
        self.next_id += 1;
        let msg = json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params});
        let result = write_frame(&mut self.stdin, &msg)
            .and_then(|_| read_response(&mut self.stdout, &mut self.stdin, self.next_id))
            .and_then(|result| {
                if method == "tools/call" && !valid_tool_result(&result) {
                    return Err(ToolError::Exec("mcp: malformed tool result".into()));
                }
                Ok(result)
            });
        // reliability 09: don't consume a stale response as a later call's result.
        // A refusal costs a new session; false success can hide an unreviewed effect.
        if result.is_err() {
            self.broken = true;
        }
        result
    }

    fn notify(&mut self, method: &str) -> Result<(), ToolError> {
        let msg = json!({"jsonrpc":"2.0","method":method});
        write_frame(&mut self.stdin, &msg)
    }
}

// reliability 09 / MCP 2025-11-25 stdio: bounded UTF-8 JSON-lines, no private
// Content-Length wrapper. Read limit includes the delimiter, before allocating.
const MAX_FRAME_BYTES: u64 = 8 * 1024 * 1024;

// reliability 09: a matching response ID proves correlation, not success.
// False rejection needs reconciliation; false acceptance hides an unknown effect.
fn valid_tool_result(value: &Value) -> bool {
    let Some(content) = value["content"].as_array() else {
        return false;
    };
    if value.get("isError").is_some_and(|v| !v.is_boolean())
        || value
            .get("structuredContent")
            .is_some_and(|v| !v.is_object())
    {
        return false;
    }
    content.iter().all(|block| match block["type"].as_str() {
        Some("text") => block["text"].is_string(),
        Some("image" | "audio") => block["data"].is_string() && block["mimeType"].is_string(),
        Some("resource_link") => block["uri"].is_string() && block["name"].is_string(),
        Some("resource") => {
            let resource = &block["resource"];
            resource["uri"].is_string()
                && (resource["text"].is_string() || resource["blob"].is_string())
        }
        _ => false,
    })
}

fn validate_initialize(init: &Value) -> Result<(), &'static str> {
    if !matches!(
        init["protocolVersion"].as_str(),
        Some("2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25")
    ) {
        return Err("mcp: unsupported protocol version");
    }
    if !init["capabilities"]["tools"].is_object()
        || !init["serverInfo"]["name"].is_string()
        || !init["serverInfo"]["version"].is_string()
    {
        return Err("mcp: incompatible server capabilities");
    }
    Ok(())
}

fn write_frame(w: &mut impl Write, msg: &Value) -> Result<(), ToolError> {
    let body = serde_json::to_vec(msg).map_err(|e| ToolError::Exec(format!("mcp encode: {e}")))?;
    if body.len() as u64 >= MAX_FRAME_BYTES {
        return Err(ToolError::NotExecuted("mcp: frame too large".into()));
    }
    w.write_all(&body)
        .and_then(|_| w.write_all(b"\n"))
        .and_then(|_| w.flush())
        .map_err(|e| ToolError::Exec(format!("mcp write: {e}")))
}

fn read_response(
    r: &mut BufReader<impl Read>,
    w: &mut impl Write,
    want_id: i64,
) -> Result<Value, ToolError> {
    for _ in 0..64 {
        let mut buf = Vec::new();
        (&mut *r)
            .take(MAX_FRAME_BYTES + 1)
            .read_until(b'\n', &mut buf)
            .map_err(|e| ToolError::Exec(format!("mcp read: {e}")))?;
        if buf.len() as u64 > MAX_FRAME_BYTES {
            return Err(ToolError::Exec("mcp: frame too large".into()));
        }
        if buf.last() != Some(&b'\n') {
            return Err(ToolError::Exec(
                "mcp: incomplete message or closed stdout".into(),
            ));
        }
        let v: Value = serde_json::from_slice(&buf)
            .map_err(|_| ToolError::Exec("mcp: invalid JSON message".into()))?;
        if v["jsonrpc"] != "2.0" {
            return Err(ToolError::Exec("mcp: invalid JSON-RPC version".into()));
        }
        if let Some(method) = v.get("method") {
            if !method.is_string() || v.get("result").is_some() || v.get("error").is_some() {
                return Err(ToolError::Exec("mcp: malformed method message".into()));
            }
            if let Some(id) = v.get("id") {
                if !id.is_string() && !id.is_i64() {
                    return Err(ToolError::Exec(
                        "mcp: invalid server request identity".into(),
                    ));
                }
                // MCP ping works in both directions, including during initialization.
                // No sampling/roots capability was advertised: explicitly reject others.
                let reply = if method == "ping" {
                    json!({"jsonrpc":"2.0","id":id,"result":{}})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not supported"}})
                };
                write_frame(w, &reply)?;
            }
            continue;
        }
        if v["id"].as_i64() != Some(want_id)
            || v.get("result").is_some() == v.get("error").is_some()
        {
            return Err(ToolError::Exec(
                "mcp: invalid response identity or shape".into(),
            ));
        }
        if let Some(error) = v.get("error") {
            // Do not reflect untrusted service error bodies into diagnostics.
            let code = error["code"]
                .as_i64()
                .ok_or_else(|| ToolError::Exec("mcp: invalid error code".into()))?;
            return Err(ToolError::Exec(format!("mcp: remote error {code}")));
        }
        return Ok(v["result"].clone());
    }
    Err(ToolError::Exec("mcp: no response after 64 messages".into()))
}

/// Connection ownership moves to one I/O worker per running request. The slot
/// mutex never covers reads/writes; waiting callers can still cancel or expire.
struct Server {
    spec: McpSpec,
    conn: Mutex<Option<Conn>>,
    process: Arc<ManagedProcess>,
    failed: AtomicBool,
}

impl Server {
    fn new(spec: McpSpec, conn: Conn) -> Arc<Self> {
        Arc::new(Self {
            spec,
            process: conn.process.clone(),
            conn: Mutex::new(Some(conn)),
            failed: AtomicBool::new(false),
        })
    }

    fn run<T: Send + 'static>(
        self: &Arc<Self>,
        timeout: Duration,
        upper: Option<Instant>,
        mut cancelled: impl FnMut() -> bool,
        operation: impl FnOnce(&mut Conn) -> Result<T, ToolError> + Send + 'static,
    ) -> Result<T, ToolError> {
        let deadline = upper.map_or(Instant::now() + timeout, |d| {
            d.min(Instant::now() + timeout)
        });
        let mut conn = loop {
            if self.failed.load(Ordering::Acquire) {
                return Err(ToolError::NotExecuted("mcp: session unavailable".into()));
            }
            if cancelled() || Instant::now() >= deadline {
                return Err(ToolError::NotExecuted(
                    "mcp: cancelled or expired before sending".into(),
                ));
            }
            if let Some(conn) = self.conn.lock().unwrap().take() {
                break conn;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let worker = self.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = operation(&mut conn);
            if result.is_err() {
                worker.failed.store(true, Ordering::Release);
                worker.process.terminate();
            } else if !worker.failed.load(Ordering::Acquire) {
                *worker.conn.lock().unwrap() = Some(conn);
            }
            let _ = tx.send(result);
        });
        loop {
            if cancelled() || Instant::now() >= deadline {
                // Intent has reached the I/O worker. Killing a process is not
                // evidence that its effect was rolled back: caller records unknown.
                self.failed.store(true, Ordering::Release);
                self.process.terminate();
                return Err(ToolError::Exec(
                    "mcp: deadline or cancellation after dispatch".into(),
                ));
            }
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(10));
            match rx.recv_timeout(remaining) {
                Ok(result) => return result,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    self.failed.store(true, Ordering::Release);
                    self.process.terminate();
                    return Err(ToolError::Exec("mcp: I/O worker disconnected".into()));
                }
            }
        }
    }

    fn handshake(self: &Arc<Self>, upper: Option<Instant>) -> Result<Vec<Value>, ToolError> {
        self.run(Duration::from_secs(8), upper, || false, Conn::handshake)
    }

    fn call_tool(
        self: &Arc<Self>,
        tool: &str,
        args: Value,
        db: &Db,
        ctx: &ToolContext,
    ) -> Result<Value, ToolError> {
        let tool = tool.to_owned();
        self.run(
            ctx.mcp_timeout.min(Duration::from_secs(120)),
            ctx.deadline,
            || crate::turn::halted(db, ctx),
            move |conn| conn.rpc("tools/call", json!({"name":tool,"arguments":args})),
        )
    }
}

/// 注册进工具管线的单个 MCP 工具。
pub struct McpTool {
    full_name: String,
    desc: String,
    schema: Value,
    server: Arc<Server>,
    tool_name: String,
    read_only_hint: bool,
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.full_name
    }
    fn description(&self) -> &str {
        &self.desc
    }
    fn input_schema(&self) -> Value {
        self.schema.clone()
    }
    /// mcp:* 焊死 External 地板（票 08）：第三方服务器语义自定——名字叫
    /// "sync_records" 的工具能做任何事。永远逐次必问，记忆 allow 在
    /// evaluate 的 L4 对本类不生效，persist_rule 拒写 mcp 规则。
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::External
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let started = std::time::Instant::now();
        let result = self
            .server
            .call_tool(&self.tool_name, input.clone(), db, ctx);
        crate::diag::note(
            if result.is_ok() {
                crate::diag::CLASS_HOST
            } else {
                crate::diag::CLASS_REJECT
            },
            result.is_err(),
            Some(&ctx.project_id),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
            None,
            "mcp_call",
            if result.is_ok() {
                "response_received"
            } else {
                "session_interrupted"
            },
            started,
        );
        result
    }

    fn for_subagent(&self, ctx: &ToolContext) -> Result<Arc<dyn Tool>, ToolError> {
        // reliability 05 / Q4: the hint is a contract, never the proof. False
        // refusal loses one delegated tool; false acceptance permits a side
        // effect. A separate OS-confined process supplies the actual boundary.
        if !self.read_only_hint || !self.server.spec.spawnable() {
            return Err(ToolError::Exec(
                "local read-only tool contract unavailable".into(),
            ));
        }
        if !self.server.spec.env.is_empty() || !self.server.spec.headers.is_empty() {
            return Err(ToolError::Exec(
                "credential/config overrides cannot be delegated to a read-only session".into(),
            ));
        }
        let root = ctx.repo_root.canonicalize()?;
        let mut spec = self.server.spec.clone();
        let cwd = spec
            .cwd
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.clone())
            .canonicalize()?;
        if !cwd.starts_with(&root)
            || crate::sandbox::read_only_spec(&root, false)
                == crate::sandbox::SandboxSpec::Unavailable
        {
            return Err(ToolError::Exec(
                "read-only service isolation unavailable".into(),
            ));
        }
        spec.cwd = Some(cwd.to_string_lossy().into());
        Ok(Arc::new(ReadOnlyMcpTool {
            definition: crate::provider::ToolDef {
                name: self.full_name.clone(),
                description: self.desc.clone(),
                input_schema: self.schema.clone(),
            },
            spec,
            tool_name: self.tool_name.clone(),
            repo_root: root,
        }))
    }
}

/// A child never receives the parent's Server/Conn Arc. Each invocation gets
/// an isolated local session; this deliberately favors confinement over reuse.
struct ReadOnlyMcpTool {
    definition: crate::provider::ToolDef,
    spec: McpSpec,
    tool_name: String,
    repo_root: std::path::PathBuf,
}

impl Tool for ReadOnlyMcpTool {
    fn name(&self) -> &str {
        &self.definition.name
    }
    fn description(&self) -> &str {
        &self.definition.description
    }
    fn input_schema(&self) -> Value {
        self.definition.input_schema.clone()
    }
    fn risk(&self) -> crate::tools::RiskClass {
        crate::tools::RiskClass::External
    }
    fn exec(&self, db: &Db, input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let started = std::time::Instant::now();
        let project = ctx.project_id.clone();
        let agent = ctx.agent_id.clone();
        let activation = ctx.stage_run_id.clone();
        let reject = move |reason: &str| {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&project),
                Some(&agent),
                activation.as_deref(),
                None,
                "mcp_readonly",
                reason,
                started,
            );
        };
        if ctx.subagent.is_none() {
            reject("child_dispatch_required");
            return Err(ToolError::Exec(
                "read-only delegation requires a child dispatch".into(),
            ));
        }
        let isolation = crate::sandbox::read_only_spec(&self.repo_root, false);
        if isolation == crate::sandbox::SandboxSpec::Unavailable {
            reject("isolation_unavailable");
            return Err(ToolError::Exec(
                "read-only service isolation unavailable".into(),
            ));
        }
        let conn = Conn::spawn_with(&self.spec, Some(&isolation))?;
        let server = Server::new(self.spec.clone(), conn);
        let tools = server
            .run(
                Duration::from_secs(8),
                ctx.deadline,
                || crate::turn::halted(db, ctx),
                Conn::handshake,
            )
            .map_err(|error| {
                ToolError::NotExecuted(format!("mcp initialization before tool dispatch: {error}"))
            })?;
        if !tools.iter().any(|tool| {
            tool["name"] == self.tool_name && tool["annotations"]["readOnlyHint"] == true
        }) {
            reject("readonly_contract_changed");
            return Err(ToolError::NotExecuted(
                "read-only tool contract changed".into(),
            ));
        }
        let result = server.call_tool(&self.tool_name, input.clone(), db, ctx);
        if result.is_err() {
            reject("readonly_interrupted");
        }
        server.process.terminate();
        result
    }
}

/// 设置-MCP 分区行（ui-audit-2 票 06）：每个配置服务的实况——
/// 起不来不连坐但必须可见（此前失败只进 log，UI 永远"空"）。
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct McpServiceRow {
    pub name: String,
    pub command: String,
    /// up = 握手成功且工具已注册；down = spawn/握手失败
    pub status: String,
    /// 已注册工具名（裸名，完整名 = mcp:<service>:<tool>）
    pub tools: Vec<String>,
    /// down 时的失败原因（spawn/handshake 阶段）
    pub error: Option<String>,
}

struct HostInner {
    closed: bool,
    servers: Vec<Arc<Server>>,
    statuses: Vec<McpServiceRow>,
    /// 握手完成、尚未装进 Registry 的工具。
    ready: Vec<McpTool>,
}

/// MCP 宿主：持全部服务进程，Drop 时全停。
/// `begin` 立刻返回，握手在后台进行，不挡住进入工作台（2026-09-22）。
pub struct McpHost {
    inner: Arc<std::sync::Mutex<HostInner>>,
}

impl McpHost {
    /// 立刻返回。每个服务先记 `starting`，握手在后台跑。
    /// 调用方用 [`Self::take_ready`] 把成功的工具装进 Registry。
    pub fn begin(specs: Vec<McpSpec>) -> Self {
        Self::begin_with_deadline(specs, None)
    }

    /// Upper caller deadline only shortens the fixed eight-second handshake.
    pub fn begin_with_deadline(specs: Vec<McpSpec>, deadline: Option<Instant>) -> Self {
        let statuses = specs
            .iter()
            .map(|s| McpServiceRow {
                name: s.name.clone(),
                command: crate::data_boundary::hidden(&s.command),
                status: "starting".into(),
                tools: vec![],
                error: None,
            })
            .collect();
        let inner = Arc::new(std::sync::Mutex::new(HostInner {
            closed: false,
            servers: vec![],
            statuses,
            ready: vec![],
        }));
        if !specs.is_empty() {
            let shared = inner.clone();
            std::thread::spawn(move || drive_handshakes(shared, specs, deadline));
        }
        Self { inner }
    }

    /// 测试和需要「打开时工具已经在」的路径：等握手结束再注册。
    pub fn start(specs: Vec<McpSpec>, registry: &crate::tools::Registry) -> Self {
        let host = Self::begin(specs);
        // 握手超时是 8 秒。这里多等一点，让状态从 starting 写成 up/down。
        host.wait_settled(std::time::Duration::from_secs(10));
        for tool in host.take_ready() {
            registry.register(tool);
        }
        host
    }

    pub fn pending(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .statuses
            .iter()
            .any(|s| s.status == "starting")
    }

    pub fn wait_settled(&self, limit: std::time::Duration) {
        let start = std::time::Instant::now();
        while self.pending() && start.elapsed() < limit {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    pub fn take_ready(&self) -> Vec<McpTool> {
        std::mem::take(&mut self.inner.lock().unwrap().ready)
    }

    /// 每配置服务一行实况（设置页列表用）。
    pub fn status(&self) -> Vec<McpServiceRow> {
        let inner = self.inner.lock().unwrap();
        let mut statuses = inner.statuses.clone();
        for row in &mut statuses {
            if row.status == "up"
                && inner.servers.iter().any(|server| {
                    server.spec.name == row.name
                        && server.failed.load(std::sync::atomic::Ordering::Acquire)
                })
            {
                row.status = "down".into();
                row.error =
                    Some("mcp: session interrupted; check action outcome before retrying".into());
            }
        }
        statuses
    }
}

/// 后台握手。不挡住 `begin` 的调用方。
fn drive_handshakes(inner: Arc<Mutex<HostInner>>, specs: Vec<McpSpec>, deadline: Option<Instant>) {
    for spec in specs {
        if inner.lock().unwrap().closed {
            break;
        }
        let conn = match Conn::spawn(&spec) {
            Ok(conn) => conn,
            Err(error) => {
                set_mcp_status(&inner, &spec.name, "down", vec![], Some(error.to_string()));
                continue;
            }
        };
        let server = Server::new(spec.clone(), conn);
        {
            let mut state = inner.lock().unwrap();
            if state.closed {
                drop(state);
                server.process.terminate();
                break;
            }
            state.servers.push(server.clone());
        }
        let shared = inner.clone();
        std::thread::spawn(move || match server.handshake(deadline) {
            Ok(tools) => {
                let names = tools
                    .iter()
                    .map(|tool| tool["name"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                {
                    let mut state = shared.lock().unwrap();
                    if state.closed {
                        return;
                    }
                    for (tool, name) in tools.iter().zip(&names) {
                        state.ready.push(McpTool {
                            full_name: format!("mcp:{}:{}", spec.name, name),
                            desc: tool["description"].as_str().unwrap_or("").into(),
                            schema: tool["inputSchema"].clone(),
                            server: server.clone(),
                            tool_name: name.clone(),
                            read_only_hint: tool["annotations"]["readOnlyHint"]
                                .as_bool()
                                .unwrap_or(false),
                        });
                    }
                }
                set_mcp_status(&shared, &spec.name, "up", names, None);
            }
            Err(error) => {
                set_mcp_status(&shared, &spec.name, "down", vec![], Some(error.to_string()))
            }
        });
    }
}

fn set_mcp_status(
    inner: &Arc<std::sync::Mutex<HostInner>>,
    name: &str,
    status: &str,
    tools: Vec<String>,
    error: Option<String>,
) {
    let mut g = inner.lock().unwrap();
    if let Some(row) = g.statuses.iter_mut().find(|r| r.name == name) {
        row.status = status.into();
        row.tools = tools;
        // Peer error bodies can echo credentials; detailed protocol facts remain host-side.
        row.error = error.map(|e| {
            if e.contains("deadline") {
                "mcp: initialization deadline exceeded"
            } else if e.contains("unsupported protocol version") {
                "mcp: unsupported protocol version"
            } else if e.contains("incompatible server capabilities") {
                "mcp: incompatible server capabilities"
            } else {
                "mcp: initialization failed; check local configuration"
            }
            .into()
        });
    }
}

impl Drop for McpHost {
    fn drop(&mut self) {
        // reliability 10 / 2026-09-23 deadlock: never wait for a pipe or conn
        // while holding HostInner. closed also fences spawn racing with close.
        let servers = {
            let mut inner = self.inner.lock().unwrap();
            inner.closed = true;
            inner.servers.clone()
        };
        for server in servers {
            server.failed.store(true, Ordering::Release);
            server.process.terminate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Registry;

    proptest::proptest! {
        #[test]
        fn standard_stdio_tool_result_requires_typed_content(text in ".{0,80}") {
            proptest::prop_assert!(valid_tool_result(&json!({"content":[{"type":"text","text":text}]})), "valid text");
            proptest::prop_assert!(!valid_tool_result(&json!({"content":[],"isError":text})), "error flag must be boolean");
            proptest::prop_assert!(!valid_tool_result(&json!({"content":[{"type":"text","text":false}]})), "text must be string");
        }
    }

    #[test]
    fn standard_stdio_answers_server_ping_without_losing_call_response() {
        let wire = b"{\"jsonrpc\":\"2.0\",\"id\":\"health\",\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n";
        let mut replies = Vec::new();
        assert_eq!(
            read_response(&mut BufReader::with_capacity(1, &wire[..]), &mut replies, 7).unwrap(),
            json!({})
        );
        let reply: Value = serde_json::from_slice(&replies).unwrap();
        assert_eq!(reply, json!({"jsonrpc":"2.0","id":"health","result":{}}));
    }

    #[test]
    fn standard_stdio_rejects_incompatible_initialize() {
        for response in [
            json!({"protocolVersion":"future", "capabilities":{"tools":{}},"serverInfo":{"name":"peer","version":"1"}}),
            json!({"protocolVersion":"2025-11-25", "capabilities":{},"serverInfo":{"name":"peer","version":"1"}}),
            json!({"protocolVersion":"2025-11-25", "capabilities":{"tools":{}},"serverInfo":{}}),
        ] {
            assert!(validate_initialize(&response).is_err());
        }
    }

    proptest::proptest! {
        #[test]
        fn standard_stdio_only_accepts_supported_versions(version in "[a-zA-Z0-9-]{0,32}") {
            let supported = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"].contains(&version.as_str());
            let init = json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"peer","version":"1"}});
            proptest::prop_assert_eq!(validate_initialize(&init).is_ok(), supported);
        }
    }

    // reliability 09: independent JSON-lines vectors, never the production encoder.
    #[test]
    fn standard_stdio_unicode_notifications_and_fragmented_read() {
        let wire = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"text\":\"你好 🦀\\nline\"}}\n";
        let mut reader = BufReader::with_capacity(1, wire.as_bytes());
        assert_eq!(
            read_response(&mut reader, &mut Vec::new(), 7).unwrap()["text"],
            "你好 🦀\nline"
        );
        let mut out = Vec::new();
        write_frame(&mut out, &json!({"jsonrpc":"2.0","id":7,"method":"ping"})).unwrap();
        assert_eq!(out.last(), Some(&b'\n'));
        assert!(!out.starts_with(b"Content-Length"));
        let decoded: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(decoded["method"], "ping");
    }

    #[test]
    fn standard_stdio_rejects_invalid_envelopes_and_oversize() {
        for wire in [
            "{\"jsonrpc\":\"1.0\",\"id\":7,\"result\":{}}\n".to_string(),
            "{\"jsonrpc\":\"2.0\",\"id\":7}\n".to_string(),
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{},\"error\":{}}\n".to_string(),
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}".to_string(),
            "x".repeat(8 * 1024 * 1024 + 1),
        ] {
            assert!(
                read_response(&mut BufReader::new(wire.as_bytes()), &mut Vec::new(), 7).is_err()
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn standard_stdio_matches_only_requested_id(other in 8i64..i64::MAX, value in "[a-z]{0,30}") {
            let wire = format!("{}\n", json!({"jsonrpc":"2.0","id":other,"result":value}));
            proptest::prop_assert!(read_response(&mut BufReader::new(wire.as_bytes()), &mut Vec::new(), 7).is_err());
        }
    }

    #[test]
    fn mcp_deadline_handshake_honors_shorter_caller_deadline() {
        let started = Instant::now();
        let host = McpHost::begin_with_deadline(
            vec![McpSpec {
                name: "silent".into(),
                command: "sleep".into(),
                args: vec!["30".into()],
                ..Default::default()
            }],
            Some(started + Duration::from_millis(80)),
        );
        host.wait_settled(Duration::from_millis(700));
        assert_eq!(host.status()[0].status, "down");
        assert!(started.elapsed() < Duration::from_millis(700));
        // Boundary probe: the unique Child owner has reaped/retired its handle.
        assert!(host.inner.lock().unwrap().servers[0]
            .process
            .child
            .lock()
            .unwrap()
            .is_none());
    }

    #[test]
    fn standard_stdio_incompatible_peer_is_down_and_not_registered() {
        for bad in [
            TEST_PEER.replace("2024-11-05", "2099-01-01"),
            TEST_PEER.replace("capabilities:{tools:{}}", "capabilities:{}"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("incompatible.cjs");
            std::fs::write(&script, bad).unwrap();
            let registry = Registry::builtin();
            let host = McpHost::start(
                vec![McpSpec {
                    name: "incompatible".into(),
                    command: "node".into(),
                    args: vec![script.to_string_lossy().into()],
                    ..Default::default()
                }],
                &registry,
            );
            assert_eq!(host.status()[0].status, "down");
            assert!(host.status()[0].error.is_some());
            assert!(!registry
                .defs()
                .iter()
                .any(|tool| tool.name.starts_with("mcp:incompatible:")));
        }
    }

    fn setup(grant: bool) -> (Db, Registry, ToolContext, tempfile::TempDir, McpHost) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_mcp.cjs");
        std::fs::write(&script, TEST_PEER).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                // 票 03：默认 L4 会放行 External 询问。本夹具守「先问再批」，钉 L0。
                "INSERT INTO projects (id, dir, name, mode, autonomy) VALUES ('p','/tmp/x','x','pack','L0')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a0','p','后端','active')",
                [],
            )
            .unwrap();
        if grant {
            db.conn()
                .execute(
                    "INSERT INTO grants (id, agent_id, kind, name) VALUES ('g1','a0','mcp','fake')",
                    [],
                )
                .unwrap();
        }
        let ctx = ToolContext {
            project_id: "p".into(),
            agent_id: "a0".into(),
            repo_root: dir.path().to_path_buf(),
            stage_run_id: None,
            owned_globs: vec![],
            tiers: Default::default(),
            sessions: Default::default(),
            caps: Default::default(),
            ..Default::default()
        };
        let reg = Registry::builtin();
        let host = McpHost::start(
            vec![McpSpec {
                name: "fake".into(),
                command: "node".into(),
                args: vec![script.to_string_lossy().into()],
                ..Default::default()
            }],
            &reg,
        );
        (db, reg, ctx, dir, host)
    }

    #[test]
    fn granted_agent_calls_mcp_tool() {
        let (db, reg, ctx, _d, _h) = setup(true);
        // 2026-10-01 owner Q10: a service grant exposes the tool, but does
        // not authorize unknown external effects. Exercise the owner gate.
        let out = reg
            .call(&db, &ctx, "mcp:fake:echo", json!({"hello": "world"}))
            .unwrap();
        let crate::tools::CallOutcome::Asked(qid) = out else {
            panic!("expected owner review, got {out:?}");
        };
        let out = reg
            .resolve(&db, &ctx, &qid, true, None, "project", None, "owner")
            .unwrap();
        let crate::tools::CallOutcome::Done(v) = out else {
            panic!("expected done, got {out:?}");
        };
        assert!(v.to_string().contains("hello"));
    }

    #[test]
    fn ungranted_agent_hard_denied() {
        let (db, reg, ctx, _d, _h) = setup(false);
        let out = reg.call(&db, &ctx, "mcp:fake:echo", json!({})).unwrap();
        let crate::tools::CallOutcome::Denied(reason) = out else {
            panic!("expected deny, got {out:?}");
        };
        assert!(reason.contains("no grant"));
    }

    // ---------- global-config 票 05：全局清单 + 合并 + CRUD ----------

    fn spec(name: &str, command: &str) -> McpSpec {
        McpSpec {
            name: name.into(),
            command: command.into(),
            ..Default::default()
        }
    }

    /// 回归（e2e 活测实证）：spawned-but-silent 服务曾把 read_line 挂死，
    /// 连累 create_project 永久卡在确认页。`sleep` 进程从不答
    /// initialize——start 必须在握手超时内返回 down，不得无限阻塞。
    /// 8s 超时 + 测试断言 <25s 留足余量。
    #[cfg(unix)]
    #[test]
    fn silent_server_times_out_instead_of_hanging() {
        let reg = crate::tools::Registry::builtin();
        let mut s = spec("silent", "sleep");
        s.args = vec!["30".into()];
        let t0 = std::time::Instant::now();
        let host = McpHost::start(vec![s], &reg);
        let elapsed = t0.elapsed();
        assert!(elapsed.as_secs() < 25, "handshake 无限阻塞: {elapsed:?}");
        let st = &host.status()[0];
        assert_eq!(st.status, "down");
        // reliability 10: handshake uses the same bounded I/O lifecycle as calls.
        assert!(st.error.as_deref().unwrap().contains("deadline"));
    }

    #[test]
    fn global_save_load_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let mut s = spec("termius", "/usr/bin/ssh-mcp");
        s.args = vec!["--stdio".into()];
        s.env.insert("API_KEY".into(), "k".into());
        save_global_mcp_at(&s, &path).unwrap();
        // upsert 同名覆写
        save_global_mcp_at(&spec("termius", "/bin/new"), &path).unwrap();
        let loaded = read_specs_at(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].command, "/bin/new");
        // env 不丢
        let mut with_env = spec("e", "/bin/e");
        with_env.env.insert("K".into(), "v".into());
        save_global_mcp_at(&with_env, &path).unwrap();
        let loaded = read_specs_at(&path);
        assert_eq!(loaded.iter().find(|s| s.name == "e").unwrap().env["K"], "v");
        delete_global_mcp_at("termius", &path).unwrap();
        assert_eq!(read_specs_at(&path).len(), 1);
    }

    #[test]
    fn validation_rejects_bad_names_and_empty_transport() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        for bad in ["", "../x", "a:b", "a b", ".hidden", &"n".repeat(65)] {
            assert!(
                save_global_mcp_at(&spec(bad, "/bin/x"), &path).is_err(),
                "{bad}"
            );
        }
        // command 与 url 互斥；二者皆空拒收
        assert!(save_global_mcp_at(&spec("x", ""), &path).is_err());
        let mut both = spec("x", "/bin/x");
        both.url = Some("http://h/sse".into());
        assert!(save_global_mcp_at(&both, &path).is_err());
    }

    #[test]
    fn merged_project_overrides_global_and_filters_unspawnable() {
        let dir = tempfile::tempdir().unwrap();
        let gpath = dir.path().join("g.json");
        save_global_mcp_at(&spec("shared", "/bin/global"), &gpath).unwrap();
        save_global_mcp_at(&spec("only-g", "/bin/g"), &gpath).unwrap();
        let mut disabled = spec("off", "/bin/off");
        disabled.disabled = true;
        save_global_mcp_at(&disabled, &gpath).unwrap();
        let mut remote = spec("web", "");
        remote.url = Some("http://h/sse".into());
        save_global_mcp_at(&remote, &gpath).unwrap();

        // 项目层同名覆盖
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".hexagon")).unwrap();
        write_specs_at(
            &repo.path().join(".hexagon/mcp.json"),
            &[spec("shared", "/bin/project")],
        )
        .unwrap();

        // 清单：全局 3 行（shared 被遮蔽）+ 项目 1 行
        let entries = list_mcp_entries_at(&gpath, Some(repo.path()));
        assert_eq!(entries.len(), 4);
        let shared = entries.iter().find(|e| e.name == "shared").unwrap();
        assert_eq!(shared.origin, "project");
        // Reliability 22: launch configuration stays host-held; runtime assertion below remains raw.
        assert!(crate::data_boundary::matches_reference(
            &shared.command,
            "/bin/project"
        ));
        assert_eq!(
            entries.iter().find(|e| e.name == "web").unwrap().transport,
            "remote"
        );

        // 运行时：disabled/remote 不 spawn；项目版 shared 生效
        let specs = merged_specs_at(&gpath, Some(repo.path()));
        let spawnable: Vec<_> = specs.into_iter().filter(|s| s.spawnable()).collect();
        assert_eq!(spawnable.len(), 2);
        assert_eq!(
            spawnable
                .iter()
                .find(|s| s.name == "shared")
                .unwrap()
                .command,
            "/bin/project"
        );
    }

    // ---------- global-config 票 06：外部扫描/导入 ----------

    #[test]
    fn scan_mcp_sources_json_and_toml() {
        let home = tempfile::tempdir().unwrap();
        // cursor JSON 源
        std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
        std::fs::write(
            home.path().join(".cursor/mcp.json"),
            r#"{"mcpServers":{
                "termius":{"command":"ssh-mcp","args":["--stdio"],"env":{"K":"v"}},
                "fig":{"url":"https://h/sse","headers":{"Authorization":"Bearer t1"}},
                "bad json row":42
            }}"#,
        )
        .unwrap();
        // codex TOML 源
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(
            home.path().join(".codex/config.toml"),
            "[mcp_servers.ctx7]\ncommand = \"npx\"\nargs = [\"-y\", \"@upstash/context7-mcp\"]\n\n[mcp_servers.remote]\nurl = \"https://r/mcp\"\n[mcp_servers.remote.headers]\nAuthorization = \"Bearer t2\"\n",
        )
        .unwrap();
        // 另一个平台同名同实现 → 去重
        std::fs::create_dir_all(
            home.path()
                .join(".claude.json".replace(".claude.json", "x"))
                .parent()
                .unwrap(),
        )
        .ok();
        std::fs::write(
            home.path().join(".claude.json"),
            r#"{"mcpServers":{"termius":{"command":"ssh-mcp","args":[]}}}"#,
        )
        .unwrap();

        let gpath = home.path().join("g.json");
        save_global_mcp_at(&spec("ctx7", "/bin/mine"), &gpath).unwrap(); // 冲突项

        let rows = scan_external_mcp_at(home.path(), &gpath);
        // cursor 3 条中 bad row 被滤；claude 的 termius 同名同 command 去重；
        // codex ctx7/remote 进列 → termius, fig, ctx7, remote = 4
        assert_eq!(rows.len(), 4, "{rows:?}");
        let t = rows.iter().find(|r| r.name == "termius").unwrap();
        assert_eq!(t.origin, "cursor");
        // Reliability 22: scan returns retention markers; import re-reads its source.
        assert!(crate::data_boundary::matches_reference(&t.env["K"], "v"));
        assert_eq!(t.transport, "stdio");
        // 远程行标头随扫描行带出（负责人反馈 2026-09：context7 导入丢了
        // Authorization——解析只认 command/url/env，headers 整环缺失）
        let fig = rows.iter().find(|r| r.name == "fig").unwrap();
        assert_eq!(fig.transport, "remote");
        assert!(crate::data_boundary::matches_reference(
            &fig.headers["Authorization"],
            "Bearer t1"
        ));
        let r = rows.iter().find(|r| r.name == "remote").unwrap();
        assert!(crate::data_boundary::matches_reference(
            &r.headers["Authorization"],
            "Bearer t2"
        ));
        let c = rows.iter().find(|r| r.name == "ctx7").unwrap();
        assert!(c.conflict);
        assert_eq!(c.origin, "codex");
        assert!(crate::data_boundary::matches_reference(&c.command, "npx"));
    }

    #[test]
    fn concealed_mcp_edit_preserves_replaces_and_removes_host_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let mut original = spec("peer", "SECRET_COMMAND");
        original.args = vec!["SECRET_ARG".into()];
        original.env.insert("TOKEN".into(), "SECRET_ENV".into());
        original
            .headers
            .insert("Authorization".into(), "SECRET_HEADER".into());
        save_global_mcp_at(&original, &path).unwrap();
        let row = list_mcp_entries_at(&path, None).remove(0);
        assert!(!serde_json::to_string(&row).unwrap().contains("SECRET"));
        let mut edit = McpSpec {
            name: row.name,
            command: row.command,
            args: row.args,
            env: row.env,
            headers: row.headers,
            cwd: row.cwd,
            disabled: true,
            url: row.url,
        };
        save_global_mcp_at(&edit, &path).unwrap();
        let saved = read_specs_at(&path).remove(0);
        assert_eq!(saved.args, original.args);
        assert_eq!(saved.env, original.env);
        assert_eq!(saved.headers, original.headers);
        assert!(saved.disabled);
        edit.env.insert("TOKEN".into(), "REPLACED".into());
        edit.headers.clear();
        save_global_mcp_at(&edit, &path).unwrap();
        let saved = read_specs_at(&path).remove(0);
        assert_eq!(saved.env["TOKEN"], "REPLACED");
        assert!(saved.headers.is_empty());
        edit.args[0] = crate::data_boundary::hidden("foreign");
        assert!(save_global_mcp_at(&edit, &path).is_err());
        assert_eq!(read_specs_at(&path)[0].args, original.args);
    }

    #[test]
    fn import_mcp_skips_conflicts_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let gpath = dir.path().join("g.json");
        save_global_mcp_at(&spec("have", "/bin/have"), &gpath).unwrap();
        let mut remote = spec("ctx7", "");
        remote.url = Some("https://mcp.context7.com/mcp".into());
        remote
            .headers
            .insert("Authorization".into(), "Bearer k".into());
        let rep = import_mcp_at(
            &gpath,
            &[
                spec("have", "/bin/other"),
                spec("new", "/bin/new"),
                spec("bad name", "/x"),
                remote,
            ],
        );
        assert_eq!(rep.imported, 2);
        assert_eq!(rep.skipped.len(), 2);
        let loaded = read_specs_at(&gpath);
        assert_eq!(loaded.len(), 3);
        // 既有条目没被覆盖
        assert_eq!(
            loaded.iter().find(|s| s.name == "have").unwrap().command,
            "/bin/have"
        );
        // 远程条目标头随导入落盘
        let c = loaded.iter().find(|s| s.name == "ctx7").unwrap();
        assert_eq!(c.headers["Authorization"], "Bearer k");
        // Reliability 22: only an opaque host reference reaches the DTO.
        let row = list_mcp_entries_at(&gpath, None)
            .into_iter()
            .find(|e| e.name == "ctx7")
            .unwrap();
        assert!(crate::data_boundary::matches_reference(
            &row.headers["Authorization"],
            "Bearer k"
        ));
    }

    #[test]
    fn dead_service_reports_error_not_crash() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::builtin();
        // 起不来的命令：静默跳过，不 panic
        let _host = McpHost::start(
            vec![McpSpec {
                name: "ghost".into(),
                command: "/nonexistent/binary".into(),
                args: vec![],
                ..Default::default()
            }],
            &reg,
        );
        assert!(reg.defs().iter().all(|d| !d.name.starts_with("mcp:")));
        let _ = dir;
    }

    /// 2026-09-23 卡死事故回归：握手线程握着 conn 锁阻塞在子进程
    /// stdout read() 时，Drop 不得等 conn（按 pid 杀）。sleep 永不写
    /// stdout → 握手线程挂死在 read；旧写法 drop 会等 conn 锁直到 8s
    /// 握手超时杀了子进程才放行，修后是瞬时。
    #[cfg(unix)]
    #[test]
    fn drop_does_not_wait_on_blocked_handshake_conn() {
        let host = McpHost::begin(vec![McpSpec {
            name: "sleeper".into(),
            command: "sleep".into(),
            args: vec!["30".into()],
            ..Default::default()
        }]);
        // 等 drive_handshakes 把 server 注册进 inner.servers（spawn 后即推入，
        // 早于握手）。真空泡会直接拿空表 drop——用例就废了。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while host.inner.lock().unwrap().servers.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "mcp sleeper never spawned"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let t0 = std::time::Instant::now();
        drop(host);
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(4),
            "McpHost::drop blocked on conn mutex: {:?}",
            t0.elapsed()
        );
    }
}
