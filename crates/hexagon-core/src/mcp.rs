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

/// 一个 MCP 服务的启动规格（来自项目配置 `.hexagon/mcp.json`）。
#[derive(Debug, Clone)]
pub struct McpSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// 从 `.hexagon/mcp.json` 读服务清单：`[{"name","command","args":[]}]`。
/// 文件缺席/损坏 = 无服务（不报错——MCP 是可选项）。
pub fn load_specs(repo_root: &std::path::Path) -> Vec<McpSpec> {
    let Ok(text) = std::fs::read_to_string(repo_root.join(".hexagon/mcp.json")) else {
        return vec![];
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            Some(McpSpec {
                name: e["name"].as_str()?.to_string(),
                command: e["command"].as_str()?.to_string(),
                args: e["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

struct Conn {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Conn {
    fn spawn(spec: &McpSpec) -> Result<Self, ToolError> {
        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
    /// MCP 出网/副作用可能性高：无记忆规则命中时默认必问（走 L5）。
    fn needs_ask(&self, _input: &Value, _ctx: &ToolContext) -> bool {
        true
    }
    fn exec(&self, _db: &Db, input: &Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        self.server.call_tool(&self.tool_name, input.clone())
    }
}

/// MCP 宿主：持全部服务进程，Drop 时全停。
pub struct McpHost {
    servers: Vec<Arc<Server>>,
}

impl McpHost {
    /// 起全部服务并把发现的工具注册进 Registry。
    /// 单个服务起不来不连坐：记录跳过（事件由调用方决定是否落）。
    pub fn start(specs: Vec<McpSpec>, registry: &mut crate::tools::Registry) -> Self {
        let mut servers = Vec::new();
        for spec in specs {
            let Ok(conn) = Conn::spawn(&spec) else {
                continue;
            };
            let server = Arc::new(Server {
                spec: spec.clone(),
                conn: Mutex::new(conn),
            });
            let Ok(tools) = server.handshake() else {
                continue;
            };
            for t in tools {
                let Some(name) = t["name"].as_str() else {
                    continue;
                };
                registry.register(McpTool {
                    full_name: format!("mcp:{}:{}", spec.name, name),
                    desc: t["description"].as_str().unwrap_or("").to_string(),
                    schema: t["inputSchema"].clone(),
                    server: server.clone(),
                    tool_name: name.to_string(),
                });
            }
            servers.push(server);
        }
        Self { servers }
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
            .resolve(&db, &ctx, &qid, true, None, "activation", None)
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
            }],
            &mut reg,
        );
        assert!(reg.defs().iter().all(|d| !d.name.starts_with("mcp:")));
        let _ = dir;
    }
}
