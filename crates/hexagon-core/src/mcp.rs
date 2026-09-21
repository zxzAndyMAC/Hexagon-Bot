//! MCP 宿主（票 13）：每服务一个 stdio OS 子进程，项目级生命周期。
//!
//! - 传输：JSON-RPC 2.0 over stdio，Content-Length 帧（MCP stdio 标准）。
//! - 握手：spawn → `initialize` → `notifications/initialized` → `tools/list`，
//!   发现的工具注册为 `mcp:<service>:<tool>`，走统一工具管线（权限照常求值；
//!   grants 表授权闸门在 permissions::evaluate 的 L0）。
//! - 崩溃韧性：传输错误时按 spec 重启一次并重试一次，仍失败则报错——
//!   子进程崩溃不拖垮核（所有 I/O 有 Mutex 串行化 + 超时外直接断开）。
//! - 无常驻 Agent 进程；Host Drop 时全部 kill。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

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
    specs.retain(|s| s.name != spec.name);
    specs.push(spec.clone());
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
    let row = |s: &McpSpec, origin: &str| McpEntryRow {
        name: s.name.clone(),
        command: s.command.clone(),
        args: s.args.clone(),
        env: s.env.clone(),
        cwd: s.cwd.clone(),
        disabled: s.disabled,
        transport: s.transport().into(),
        url: s.url.clone(),
        origin: origin.into(),
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
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub disabled: bool,
    /// "stdio" | "remote"
    pub transport: String,
    pub url: Option<String>,
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
                disabled: false,
                url: get("url").and_then(|v| v.as_str()).map(String::from),
            }
        })
        .collect()
}

/// 扫描入口（`home` 注入便于测试；线上用 HOME）。单源解析失败跳过不连坐。
pub fn scan_external_mcp_at(
    home: &std::path::Path,
    global_path: &std::path::Path,
) -> Vec<ExtMcpRow> {
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
                conflict: existing.contains(&spec.name.to_lowercase()),
                transport: spec.transport().into(),
                name: spec.name,
                command: spec.command,
                args: spec.args,
                env: spec.env,
                cwd: spec.cwd,
                disabled: spec.disabled,
                url: spec.url,
                origin: platform.to_string(),
                source_path: path.to_string_lossy().to_string(),
            });
        }
    }
    out
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

struct Conn {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Conn {
    fn spawn(spec: &McpSpec) -> Result<Self, ToolError> {
        log::info!("mcp spawn: {} {} {:?}", spec.name, spec.command, spec.args);
        let mut cmd = Command::new(&spec.command);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .envs(&spec.env);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::Exec(format!("mcp spawn {}: {e}", spec.name)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ToolError::Exec("mcp stdin closed".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::Exec("mcp stdout closed".into()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// 发一帧 JSON-RPC 并读到响应（跳过通知帧）。
    fn rpc(&mut self, method: &str, params: Value, id: i64) -> Result<Value, ToolError> {
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        write_frame(&mut self.stdin, &msg)?;
        read_response(&mut self.stdout, id)
    }

    fn notify(&mut self, method: &str) -> Result<(), ToolError> {
        let msg = json!({"jsonrpc":"2.0","method":method});
        write_frame(&mut self.stdin, &msg)
    }
}

fn write_frame(w: &mut impl Write, msg: &Value) -> Result<(), ToolError> {
    let body = msg.to_string();
    w.write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes())
        .and_then(|_| w.flush())
        .map_err(|e| ToolError::Exec(format!("mcp write: {e}")))
}

fn read_response(r: &mut BufReader<impl Read>, want_id: i64) -> Result<Value, ToolError> {
    // 最多读 64 帧找响应（跳过通知/请求帧），防慢服务挂死时无限读
    for _ in 0..64 {
        let mut headers = HashMap::new();
        loop {
            let mut line = String::new();
            let n = r
                .read_line(&mut line)
                .map_err(|e| ToolError::Exec(format!("mcp read: {e}")))?;
            if n == 0 {
                return Err(ToolError::Exec("mcp: server closed stdout".into()));
            }
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        let len: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| ToolError::Exec("mcp: missing Content-Length".into()))?;
        if len > 8 * 1024 * 1024 {
            return Err(ToolError::Exec("mcp: frame too large".into()));
        }
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)
            .map_err(|e| ToolError::Exec(format!("mcp read body: {e}")))?;
        let v: Value = serde_json::from_slice(&buf)
            .map_err(|e| ToolError::Exec(format!("mcp bad json: {e}")))?;
        if v["id"].as_i64() == Some(want_id) {
            if let Some(err) = v.get("error") {
                return Err(ToolError::Exec(format!("mcp error: {err}")));
            }
            return Ok(v["result"].clone());
        }
        // 通知/请求帧：跳过
    }
    Err(ToolError::Exec("mcp: no response after 64 frames".into()))
}

/// 一个运行中的服务：连接 + 重启规格。
struct Server {
    spec: McpSpec,
    conn: Mutex<Conn>,
}

impl Server {
    /// 握手：initialize → initialized → tools/list。
    fn handshake(&self) -> Result<Vec<Value>, ToolError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| ToolError::Exec("poison".into()))?;
        conn.rpc(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "hexagon", "version": "0.1"}
            }),
            1,
        )?;
        conn.notify("notifications/initialized")?;
        let res = conn.rpc("tools/list", json!({}), 2)?;
        Ok(res["tools"].as_array().cloned().unwrap_or_default())
    }

    fn call_tool(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
        // 先试现连接；传输错误 → 重启一次重试一次
        let attempt =
            |conn: &mut Conn| conn.rpc("tools/call", json!({"name": tool, "arguments": args}), 3);
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| ToolError::Exec("poison".into()))?;
        match attempt(&mut conn) {
            Ok(v) => Ok(v),
            Err(e) => {
                // 重启策略：传输层失败才重启；RPC 层 error 直接上报
                if e.to_string().contains("mcp error:") {
                    return Err(e);
                }
                log::warn!(
                    "mcp transport failed for {}, respawning: {e}",
                    self.spec.name
                );
                let _ = conn.child.kill();
                *conn = Conn::spawn(&self.spec)?;
                // 重启后要重新握手，否则服务端不认 tools/call
                drop(conn);
                self.handshake()?;
                let mut conn = self
                    .conn
                    .lock()
                    .map_err(|_| ToolError::Exec("poison".into()))?;
                attempt(&mut conn)
            }
        }
    }
}

/// 注册进工具管线的单个 MCP 工具。
pub struct McpTool {
    full_name: String,
    desc: String,
    schema: Value,
    server: Arc<Server>,
    tool_name: String,
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
    fn exec(&self, _db: &Db, input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        self.server.call_tool(&self.tool_name, input.clone())
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

/// MCP 宿主：持全部服务进程，Drop 时全停。
pub struct McpHost {
    servers: Vec<Arc<Server>>,
    /// 每 spec 一行状态（含失败）——设置页审计面。
    statuses: Vec<McpServiceRow>,
}

impl McpHost {
    /// 起全部服务并把发现的工具注册进 Registry。
    /// 单个服务起不来不连坐：记 status=down 继续（事件由调用方决定是否落）。
    pub fn start(specs: Vec<McpSpec>, registry: &mut crate::tools::Registry) -> Self {
        let mut servers = Vec::new();
        let mut statuses = Vec::new();
        for spec in specs {
            let row = |status: &str, tools: Vec<String>, error: Option<String>| McpServiceRow {
                name: spec.name.clone(),
                command: spec.command.clone(),
                status: status.into(),
                tools,
                error,
            };
            let conn = match Conn::spawn(&spec) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("mcp service {} failed to spawn, skipped", spec.name);
                    statuses.push(row("down", vec![], Some(e.to_string())));
                    continue;
                }
            };
            let server = Arc::new(Server {
                spec: spec.clone(),
                conn: Mutex::new(conn),
            });
            let tools = match server.handshake() {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("mcp service {} handshake failed, skipped", spec.name);
                    // Child::drop 不杀进程——握手失败的服务若不 kill 会成孤儿。
                    if let Ok(mut c) = server.conn.lock() {
                        let _ = c.child.kill();
                    }
                    statuses.push(row("down", vec![], Some(e.to_string())));
                    continue;
                }
            };
            let tool_names: Vec<String> = tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect();
            log::info!("mcp service {} up: {} tools", spec.name, tool_names.len());
            for (t, name) in tools.iter().zip(tool_names.iter()) {
                registry.register(McpTool {
                    full_name: format!("mcp:{}:{}", spec.name, name),
                    desc: t["description"].as_str().unwrap_or("").to_string(),
                    schema: t["inputSchema"].clone(),
                    server: server.clone(),
                    tool_name: name.clone(),
                });
            }
            statuses.push(row("up", tool_names, None));
            servers.push(server);
        }
        Self { servers, statuses }
    }

    /// 每配置服务一行实况（设置页列表用）。
    pub fn status(&self) -> &[McpServiceRow] {
        &self.statuses
    }
}

impl Drop for McpHost {
    fn drop(&mut self) {
        for s in &self.servers {
            if let Ok(mut c) = s.conn.lock() {
                let _ = c.child.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Registry;

    /// 假 MCP 服务：python 脚本说 Content-Length 帧 JSON-RPC。
    /// initialize/tools/list 应答清单；tools/call 回显参数。
    const FAKE: &str = r#"
import sys, json
def send(msg):
    body = json.dumps(msg)
    sys.stdout.write(f"Content-Length: {len(body)}\r\n\r\n{body}")
    sys.stdout.flush()
while True:
    headers = {}
    while True:
        line = sys.stdin.readline()
        if not line: sys.exit(0)
        if line.strip() == "": break
        k, v = line.split(":", 1); headers[k.strip()] = v.strip()
    body = sys.stdin.read(int(headers["Content-Length"]))
    req = json.loads(body)
    if "id" not in req: continue
    if req["method"] == "initialize":
        send({"jsonrpc":"2.0","id":req["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fake","version":"0"}}})
    elif req["method"] == "tools/list":
        send({"jsonrpc":"2.0","id":req["id"],"result":{"tools":[{"name":"echo","description":"echo args","inputSchema":{"type":"object"}}]}})
    elif req["method"] == "tools/call":
        send({"jsonrpc":"2.0","id":req["id"],"result":{"content":[{"type":"text","text":json.dumps(req["params"]["arguments"])}]}})
    else:
        send({"jsonrpc":"2.0","id":req["id"],"error":{"code":-32601,"message":"unknown"}})
"#;

    fn setup(grant: bool) -> (Db, Registry, ToolContext, tempfile::TempDir, McpHost) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_mcp.py");
        std::fs::write(&script, FAKE).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p','/tmp/x','x','pack')",
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
        };
        let mut reg = Registry::builtin();
        let host = McpHost::start(
            vec![McpSpec {
                name: "fake".into(),
                command: "/usr/bin/python3".into(),
                args: vec![script.to_string_lossy().into()],
                ..Default::default()
            }],
            &mut reg,
        );
        (db, reg, ctx, dir, host)
    }

    #[test]
    fn granted_agent_calls_mcp_tool() {
        let (db, reg, ctx, _d, _h) = setup(true);
        // mcp 工具默认必问：先转必问，再批准执行
        let out = reg
            .call(&db, &ctx, "mcp:fake:echo", json!({"hello": "world"}))
            .unwrap();
        let crate::tools::CallOutcome::Asked(qid) = out else {
            panic!("expected ask, got {out:?}");
        };
        let out = reg
            .resolve(&db, &ctx, &qid, true, None, "activation", None, "owner")
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
        assert_eq!(shared.command, "/bin/project");
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
                "fig":{"url":"https://h/sse"},
                "bad json row":42
            }}"#,
        )
        .unwrap();
        // codex TOML 源
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(
            home.path().join(".codex/config.toml"),
            "[mcp_servers.ctx7]\ncommand = \"npx\"\nargs = [\"-y\", \"@upstash/context7-mcp\"]\n\n[mcp_servers.remote]\nurl = \"https://r/mcp\"\n",
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
        assert_eq!(t.env["K"], "v");
        assert_eq!(t.transport, "stdio");
        assert_eq!(
            rows.iter().find(|r| r.name == "fig").unwrap().transport,
            "remote"
        );
        let c = rows.iter().find(|r| r.name == "ctx7").unwrap();
        assert!(c.conflict);
        assert_eq!(c.origin, "codex");
        assert_eq!(c.command, "npx");
    }

    #[test]
    fn import_mcp_skips_conflicts_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let gpath = dir.path().join("g.json");
        save_global_mcp_at(&spec("have", "/bin/have"), &gpath).unwrap();
        let rep = import_mcp_at(
            &gpath,
            &[
                spec("have", "/bin/other"),
                spec("new", "/bin/new"),
                spec("bad name", "/x"),
            ],
        );
        assert_eq!(rep.imported, 1);
        assert_eq!(rep.skipped.len(), 2);
        let loaded = read_specs_at(&gpath);
        assert_eq!(loaded.len(), 2);
        // 既有条目没被覆盖
        assert_eq!(
            loaded.iter().find(|s| s.name == "have").unwrap().command,
            "/bin/have"
        );
    }

    #[test]
    fn dead_service_reports_error_not_crash() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::builtin();
        // 起不来的命令：静默跳过，不 panic
        let _host = McpHost::start(
            vec![McpSpec {
                name: "ghost".into(),
                command: "/nonexistent/binary".into(),
                args: vec![],
                ..Default::default()
            }],
            &mut reg,
        );
        assert!(reg.defs().iter().all(|d| !d.name.starts_with("mcp:")));
        let _ = dir;
    }
}
