//! 自建文件溯源 + 激活 known-world 快照（openworker-borrow 票 03）。
//!
//! 出处：OpenWorker `coworker/provenance.py` + `coworker/session_facts.py`。
//! 必问卡（和将来的 reviewer）看不到文件内容，判不了 `python scripts/setup.py`
//! 的效果——但引擎知道一件双方都看不到的事：这文件是不是 agent 本激活刚写的/
//! 刚下载的。本模块从 trace 事件**机械派生**这份记录（不建新表），渲染成
//! 固定词表的一行事实。
//!
//! 不变量：
//! - 只给事实不给内容——渲染词表固定，永不输出文件正文；
//! - 缺失不增益信任：匹配不到就什么都不说（不是声称「安全」）；
//! - 只记成功调用：失败的写入没落盘，没有可执行的东西。

use crate::db::Db;
use crate::tools::ToolContext;
use crate::trace::EventKind;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

const WRITTEN: &str = "written";
const DOWNLOADED: &str = "downloaded";

/// 真正输入是命令行上从不点名的文件：没有这张表 `make deploy` 看起来什么都没碰。
/// 出处照抄 provenance.py `_IMPLICIT_TARGETS`。
const IMPLICIT_TARGETS: &[(&str, &[&str])] = &[
    ("make", &["Makefile", "makefile", "GNUmakefile"]),
    ("npm", &["package.json"]),
    ("pnpm", &["package.json"]),
    ("yarn", &["package.json"]),
    ("bun", &["package.json"]),
    ("pytest", &["conftest.py"]),
    ("tox", &["tox.ini"]),
    ("nox", &["noxfile.py"]),
    (
        "docker-compose",
        &[
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yaml",
            "compose.yml",
        ],
    ),
];

/// 裸 token（无分隔符）值得按文件解析的扩展名。
const SCRIPT_SUFFIXES: &[&str] = &[
    ".py", ".sh", ".bash", ".zsh", ".js", ".mjs", ".cjs", ".ts", ".rb", ".pl", ".php", ".ps1",
    ".bat", ".cmd", ".jar", ".exe", ".json", ".yml", ".yaml", ".ini", ".toml", ".cfg", ".mk",
];

/// shell 拉取器：flag 的值指向输出路径。`curl -O`（无值、按 URL basename 存）单列。
const FETCHER_OUTPUT_FLAGS: &[(&str, &[&str])] = &[
    ("curl", &["-o", "--output"]),
    ("wget", &["-O", "--output-document"]),
    ("invoke-webrequest", &["-outfile"]),
    ("iwr", &["-outfile"]),
];
/// 这两者的 flag 名大小写不敏感（PowerShell 传统）。
const CASE_FOLDED_FETCHERS: &[&str] = &["invoke-webrequest", "iwr"];

fn program(argv: &[String]) -> String {
    let name = argv[0]
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&argv[0])
        .to_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

fn looks_like_path(t: &str) -> bool {
    if t.is_empty() || t.starts_with('-') || t.contains("://") {
        return false;
    }
    if t.contains('/') || t.contains('\\') {
        return true;
    }
    let lower = t.to_lowercase();
    SCRIPT_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

fn sub_commands(cmd: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for part in crate::permissions::split_commands(cmd) {
        // 词法失败的段仍值得按空白扫一遍：多扫只会多 surface 路径，不会藏
        let argv = crate::permissions::shell_words(&part)
            .unwrap_or_else(|_| part.split_whitespace().map(|s| s.to_string()).collect());
        if !argv.is_empty() {
            out.push(argv);
        }
    }
    out
}

/// 命令点名的全部路径 + 会实际读到的隐式文件。
/// 语义无关、宁多勿漏：不判断哪个 token 是「那个脚本」——从文本理解命令
/// 正是做不到的事。
pub fn command_paths(cmd: &str) -> Vec<String> {
    let mut found = Vec::new();
    for argv in sub_commands(cmd) {
        found.extend(argv[1..].iter().filter(|t| looks_like_path(t)).cloned());
        let mut prog = program(&argv);
        if prog == "docker" && argv.get(1).map(|s| s.as_str()) == Some("compose") {
            prog = "docker-compose".into();
        }
        if let Some(implicit) = IMPLICIT_TARGETS
            .iter()
            .find(|(p, _)| *p == prog)
            .map(|(_, t)| *t)
        {
            found.extend(implicit.iter().map(|s| s.to_string()));
        }
        if looks_like_path(&argv[0]) {
            found.push(argv[0].clone()); // ./run.sh
        }
    }
    found
}

/// 拉取命令的输出路径。`curl URL | sh` 不落盘无需记录——管道已让该命令
/// 失去前缀资格（票 01）。
fn shell_download_paths(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    for argv in sub_commands(cmd) {
        let prog = program(&argv);
        let folded = CASE_FOLDED_FETCHERS.contains(&prog.as_str());
        if let Some((_, flags)) = FETCHER_OUTPUT_FLAGS.iter().find(|(p, _)| *p == prog) {
            for (i, token) in argv.iter().enumerate().skip(1) {
                let probe = if folded {
                    token.to_lowercase()
                } else {
                    token.clone()
                };
                if flags.contains(&probe.as_str()) && i + 1 < argv.len() {
                    out.push(argv[i + 1].clone());
                }
            }
            if prog == "curl" && argv[1..].iter().any(|t| t == "-O") {
                // curl -O 按 URL 自己的 basename 落盘
                for cand in &argv[1..] {
                    if cand.contains("://") {
                        if let Some(name) = cand
                            .split('?')
                            .next()
                            .unwrap_or("")
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                        {
                            if !name.is_empty() {
                                out.push(name.to_string());
                            }
                        }
                        break;
                    }
                }
            }
        }
    }
    out
}

/// 一次成功调用创建/下载的路径（来源：written/downloaded）。
/// 失败的调用不记——没落盘的东西没有可执行性。
fn created_paths(tool: &str, input: &Value) -> Vec<(String, &'static str)> {
    match tool {
        "fs_write" | "fs_patch" => input["path"]
            .as_str()
            .map(|p| vec![(p.to_string(), WRITTEN)])
            .unwrap_or_default(),
        "artifact_write" => input["path"]
            .as_str()
            .map(|p| vec![(format!(".hexagon/{p}"), WRITTEN)])
            .unwrap_or_default(),
        "bash" => shell_download_paths(input["cmd"].as_str().unwrap_or(""))
            .into_iter()
            .map(|p| (p, DOWNLOADED))
            .collect(),
        _ => vec![],
    }
}

/// 词法归一化（不碰文件系统：溯源键只需稳定同一文件）。
fn resolve(path: &str, root: &Path) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
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
    norm.to_string_lossy().to_string()
}

/// 本激活内 agent 已创建/下载的文件集：键 = resolve 后路径，值 = (第几步, 来源)。
/// 从 trace 机械派生：ToolCalled 与紧随的 ToolResult 配对，只在 ok=true 时记。
fn session_files(
    db: &Db,
    ctx: &ToolContext,
) -> Result<std::collections::HashMap<String, (usize, &'static str)>, crate::tools::ToolError> {
    let mut st = db.conn().prepare(
        "SELECT kind, payload FROM events
         WHERE project_id=?1 AND agent_id=?2 AND (?3 IS NULL OR stage_run_id=?3)
         AND kind IN ('tool_called','tool_result') ORDER BY id",
    )?;
    let rows = st
        .query_map(
            rusqlite::params![ctx.project_id, ctx.agent_id, ctx.stage_run_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut files = std::collections::HashMap::new();
    let mut pending: std::collections::VecDeque<(String, Value)> =
        std::collections::VecDeque::new();
    let mut step = 0usize;
    for (kind, payload) in rows {
        let payload: Value = serde_json::from_str(&payload).unwrap_or_default();
        if kind == "tool_called" {
            step += 1;
            pending.push_back((
                payload["tool"].as_str().unwrap_or("").to_string(),
                payload["input"].clone(),
            ));
            continue;
        }
        // tool_result：与最早的同名未配对调用配对
        if payload["ok"].as_bool() != Some(true) {
            // 失败结果也消费掉配对位，防止错位
            let tool = payload["tool"].as_str().unwrap_or("");
            if let Some(pos) = pending.iter().position(|(t, _)| t == tool) {
                pending.remove(pos);
            }
            continue;
        }
        let tool = payload["tool"].as_str().unwrap_or("");
        let Some(pos) = pending.iter().position(|(t, _)| t == tool) else {
            continue;
        };
        let (_, input) = pending.remove(pos).unwrap();
        for (path, origin) in created_paths(tool, &input) {
            // 后写/后下载覆盖先者——会执行的是新字节
            files.insert(resolve(&path, &ctx.repo_root), (step, origin));
        }
    }
    Ok(files)
}

/// 提议的 bash 命令里最新被本激活自建的路径。最新者优先——
/// 那是 agent 最近控制过内容的那个。
pub fn match_command(
    db: &Db,
    ctx: &ToolContext,
    cmd: &str,
) -> Result<Option<ProvenanceHit>, crate::tools::ToolError> {
    let files = session_files(db, ctx)?;
    if files.is_empty() {
        return Ok(None);
    }
    // 当前步 = 已发生的工具调用总数
    let step: usize = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE project_id=?1 AND agent_id=?2
             AND (?3 IS NULL OR stage_run_id=?3) AND kind='tool_called'",
            rusqlite::params![ctx.project_id, ctx.agent_id, ctx.stage_run_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .unwrap_or(0);
    let mut best: Option<ProvenanceHit> = None;
    for path in command_paths(cmd) {
        let Some((origin_step, kind)) = files.get(&resolve(&path, &ctx.repo_root)) else {
            continue;
        };
        let hit = ProvenanceHit {
            path: path.clone(),
            downloaded: *kind == DOWNLOADED,
            steps_ago: step.saturating_sub(*origin_step),
            step: *origin_step,
        };
        if best.as_ref().map(|b| hit.step > b.step).unwrap_or(true) {
            best = Some(hit);
        }
    }
    Ok(best)
}

#[derive(Debug, Clone)]
pub struct ProvenanceHit {
    /// 调用里原样的路径（给人看的行用原文）
    pub path: String,
    pub downloaded: bool,
    pub steps_ago: usize,
    step: usize,
}

impl ProvenanceHit {
    /// 固定词表一行：永不带文件内容。
    pub fn render(&self) -> String {
        let verb = if self.downloaded { "下载" } else { "创建" };
        let when = match self.steps_ago {
            0 => "刚刚".to_string(),
            1 => "1 步前".to_string(),
            n => format!("{n} 步前"),
        };
        format!("{} 由本 agent {} {}", self.path, when, verb)
    }
}

/// 工具调用的溯源注记（目前只有 bash——写后执行链都落在 shell 命令文本里）。
pub fn note(db: &Db, ctx: &ToolContext, tool: &str, input: &Value) -> Option<String> {
    if tool != "bash" {
        return None;
    }
    match_command(db, ctx, input["cmd"].as_str().unwrap_or(""))
        .ok()
        .flatten()
        .map(|h| h.render())
}

// ---------- 机械状态块（openworker-borrow 票 06）----------
//
// 字段全部从 trace 事件确定性派生，零模型参与。
// 票 07 / ADR 0066：不许再把它塞进回合上下文代替被移走的轮次——那是摘要，
// 会盖住负责人和角色原文。撞限只删工具记录（turn::context）。

/// 本激活的机械状态块：写过/下载的文件、已交付产物、最近 bash+exit。
pub fn state_block(db: &Db, ctx: &ToolContext) -> String {
    let mut lines = vec!["## 机械状态（trace 事件派生，非模型摘要）".to_string()];
    if let Ok(files) = session_files(db, ctx) {
        let mut written: Vec<&str> = Vec::new();
        let mut downloaded: Vec<&str> = Vec::new();
        for (path, (_, kind)) in &files {
            match *kind {
                WRITTEN => written.push(path),
                _ => downloaded.push(path),
            }
        }
        written.sort();
        downloaded.sort();
        let rel = |p: &str| {
            Path::new(p)
                .strip_prefix(&ctx.repo_root)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_else(|_| p.to_string())
        };
        if !written.is_empty() {
            lines.push(format!(
                "本激活写过的文件：{}",
                written
                    .iter()
                    .map(|p| rel(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !downloaded.is_empty() {
            lines.push(format!(
                "本激活下载的文件：{}",
                downloaded
                    .iter()
                    .map(|p| rel(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    // 已交付产物（本激活）
    if let Ok(mut st) = db.conn().prepare(
        "SELECT path, kind FROM artifacts WHERE project_id=?1
         AND (?2 IS NULL OR stage_run_id=?2) ORDER BY created_at",
    ) {
        if let Ok(rows) = st.query_map(rusqlite::params![ctx.project_id, ctx.stage_run_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            let arts: Vec<String> = rows
                .filter_map(|r| r.ok())
                .map(|(p, k)| format!("{p}({k})"))
                .collect();
            if !arts.is_empty() {
                lines.push(format!("已交付产物：{}", arts.join(", ")));
            }
        }
    }
    // 最近一次 bash 命令 + exit code
    if let Ok(mut st) = db.conn().prepare(
        "SELECT payload FROM events WHERE project_id=?1 AND agent_id=?2
         AND (?3 IS NULL OR stage_run_id=?3) AND kind IN ('tool_called','tool_result')
         ORDER BY id DESC LIMIT 60",
    ) {
        if let Ok(rows) = st.query_map(
            rusqlite::params![ctx.project_id, ctx.agent_id, ctx.stage_run_id],
            |r| r.get::<_, String>(0),
        ) {
            let mut pending_cmd: Option<String> = None;
            for payload in rows.filter_map(|r| r.ok()) {
                let Ok(v) = serde_json::from_str::<Value>(&payload) else {
                    continue;
                };
                if v["tool"].as_str() != Some("bash") {
                    continue;
                }
                // 倒序扫：先遇 tool_result(exit_code)，再遇其 tool_called(cmd)
                if pending_cmd.is_none() && v["ok"].is_boolean() {
                    let code = v["result"]["output"]["exit_code"]
                        .as_i64()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "?".into());
                    pending_cmd = Some(format!("__EXIT__{code}"));
                    continue;
                }
                if let Some(exit) = pending_cmd.take() {
                    if let Some(cmd) = v["input"]["cmd"].as_str() {
                        let exit = exit.trim_start_matches("__EXIT__");
                        lines.push(format!("最近命令：`{cmd}` → exit {exit}"));
                        break;
                    }
                }
            }
        }
    }
    lines.join("\n")
}

// ---------- known world 快照 ----------

/// 激活冻结：`git remote -v` + repo_root 落 System 事件（open_stage 调用）。
/// 必问卡/reviewer 拿「会话开始时」的基线比对——防 agent 先 `remote add`
/// 再 push，让目的地显得本来就熟悉。
pub fn snapshot_known_world(db: &Db, project_id: &str, stage_run_id: &str) {
    let dir: Option<String> = db
        .conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project_id], |r| {
            r.get(0)
        })
        .ok();
    let remotes = dir
        .as_deref()
        .and_then(|d| crate::git::run(Path::new(d), &["remote", "-v"]).ok())
        .map(|out| parse_remotes(&out))
        .unwrap_or_default();
    if let Err(e) = db.append_event(
        project_id,
        EventKind::System,
        json!({
            "kind": "known_world",
            "remotes": remotes,
            "repo_root": dir,
        }),
        None,
        Some(stage_run_id),
    ) {
        log::warn!("known_world snapshot failed: {e}");
    }
}

/// `git remote -v` 输出 → [{name,url}]（按名去重；fetch/push 两行同名）。
fn parse_remotes(out: &str) -> Vec<Value> {
    let mut seen = std::collections::HashMap::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        if let (Some(name), Some(url)) = (it.next(), it.next()) {
            seen.entry(name.to_string())
                .or_insert_with(|| url.to_string());
        }
    }
    seen.into_iter()
        .map(|(name, url)| json!({"name": name, "url": url}))
        .collect()
}

/// 最近一次 known_world 快照（本激活优先，回落项目最近一条）。
pub fn known_world(db: &Db, project_id: &str, stage_run_id: Option<&str>) -> Option<Value> {
    let mut st = db
        .conn()
        .prepare(
            "SELECT payload FROM events WHERE project_id=?1
             AND kind='system' AND json_extract(payload,'$.kind')='known_world'
             AND (?2 IS NULL OR stage_run_id=?2)
             ORDER BY id DESC LIMIT 1",
        )
        .ok()?;
    st.query_row(rusqlite::params![project_id, stage_run_id], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
}

/// `git push` 的 remote 参数与初始列表比对：不在即返回注记行。
/// 快照缺失 → None（无基线不给结论，缺失不增益信任）。
pub fn remote_delta(tool: &str, input: &Value, world: Option<&Value>) -> Option<String> {
    if tool != "bash" {
        return None;
    }
    let cmd = input["cmd"].as_str().unwrap_or("");
    let remotes: Vec<String> = world?["remotes"]
        .as_array()?
        .iter()
        .filter_map(|r| r["name"].as_str().map(|s| s.to_string()))
        .collect();
    for argv in sub_commands(cmd) {
        if argv.len() >= 3 && program(&argv) == "git" && argv[1] == "push" {
            let remote = argv[2..].iter().find(|t| !t.starts_with('-'));
            if let Some(r) = remote {
                if !remotes.iter().any(|n| n == r) {
                    return Some(format!("remote '{r}' 不在激活开始时的 remote 列表"));
                }
            }
        }
    }
    None
}

/// taint（票 10）：外部内容源（subagent/旧 research 结论、mcp:* 结果）回喂过
/// 该 agent 后，其后续产出的事件打 `after_external`——下游 agent 与负责人
/// 看得出「这段话是在读了外部内容之后写的」。纯机械派生自 trace，不改消息
/// 结构。subagent 回执含网页摘要——与旧 research 同档外部内容（
/// code-search 票 04 更名迁移，旧轨迹里的 research 行仍算）。
/// 不对称性：漏标 = 下游误信来源纯度；多标 = 多一行元数据——宁可多标。
pub fn tainted(db: &Db, agent_id: &str, stage_run_id: Option<&str>) -> bool {
    db.conn()
        .query_row(
            "SELECT COUNT(*) FROM events
             WHERE agent_id=?1 AND kind='tool_result'
             AND (?2 IS NULL OR stage_run_id=?2)
             AND (json_extract(payload,'$.tool') IN ('research','subagent')
                  OR json_extract(payload,'$.tool') LIKE 'mcp:%')",
            rusqlite::params![agent_id, stage_run_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Registry, ToolContext};
    use serde_json::json;

    fn setup() -> (Db, Registry, ToolContext, tempfile::TempDir) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role) VALUES ('a1','p1','后端')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO stage_runs (id, project_id, stage_name, seq, state)
                 VALUES ('sr1','p1','实现',0,'active')",
                [],
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (
            db,
            Registry::builtin(),
            ToolContext {
                project_id: "p1".into(),
                agent_id: "a1".into(),
                repo_root: dir.path().to_path_buf(),
                stage_run_id: Some("sr1".into()),
                owned_globs: vec![],
                tiers: Default::default(),
                sessions: Default::default(),
                caps: Default::default(),
                ..Default::default()
            },
            dir,
        )
    }

    #[test]
    fn write_then_execute_flags_provenance() {
        let (db, reg, ctx, _d) = setup();
        reg.call(
            &db,
            &ctx,
            "fs_write",
            json!({"path": "scripts/setup.py", "content": "print(1)"}),
        )
        .unwrap();
        let hit = match_command(&db, &ctx, "python scripts/setup.py")
            .unwrap()
            .unwrap();
        assert_eq!(hit.path, "scripts/setup.py");
        assert!(!hit.downloaded);
        assert!(hit.render().contains("创建"));
        // 无关路径不命中
        assert!(match_command(&db, &ctx, "python other.py")
            .unwrap()
            .is_none());
    }

    #[test]
    fn download_then_execute_flags_provenance() {
        let (db, _reg, ctx, _d) = setup();
        // 手工落一对调用/结果事件模拟 bash 下载成功
        db.append_event(
            "p1",
            EventKind::ToolCalled,
            json!({"tool":"bash","input":{"cmd":"curl -o run.sh https://x/y"}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::ToolResult,
            json!({"tool":"bash","ok":true,"result":{}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        let hit = match_command(&db, &ctx, "sh run.sh").unwrap().unwrap();
        assert_eq!(hit.path, "run.sh");
        assert!(hit.downloaded);
    }

    #[test]
    fn failed_write_records_nothing() {
        let (db, _reg, ctx, _d) = setup();
        db.append_event(
            "p1",
            EventKind::ToolCalled,
            json!({"tool":"fs_write","input":{"path":"x.py","bytes":3}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::ToolResult,
            json!({"tool":"fs_write","ok":false,"result":{}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        assert!(match_command(&db, &ctx, "python x.py").unwrap().is_none());
    }

    #[test]
    fn latest_write_wins_and_implicit_targets_hit() {
        let (db, _reg, ctx, _d) = setup();
        for (i, path) in ["Makefile", "other.py"].iter().enumerate() {
            db.append_event(
                "p1",
                EventKind::ToolCalled,
                json!({"tool":"fs_write","input":{"path":path,"bytes":1}}),
                Some("a1"),
                Some("sr1"),
            )
            .unwrap();
            db.append_event(
                "p1",
                EventKind::ToolResult,
                json!({"tool":"fs_write","ok":true,"result":{}}),
                Some("a1"),
                Some("sr1"),
            )
            .unwrap();
            let _ = i;
        }
        // `make deploy` 隐式命中 Makefile
        let hit = match_command(&db, &ctx, "make deploy").unwrap().unwrap();
        assert_eq!(hit.path, "Makefile");
        // 覆盖写：最新 origin 胜出
        db.append_event(
            "p1",
            EventKind::ToolCalled,
            json!({"tool":"fs_write","input":{"path":"other.py","bytes":1}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        db.append_event(
            "p1",
            EventKind::ToolResult,
            json!({"tool":"fs_write","ok":true,"result":{}}),
            Some("a1"),
            Some("sr1"),
        )
        .unwrap();
        let hit = match_command(&db, &ctx, "python other.py")
            .unwrap()
            .unwrap();
        assert!(hit.steps_ago <= 1);
    }

    #[test]
    fn push_to_new_remote_flagged() {
        let (_db, _reg, _ctx, _d) = setup();
        let world = json!({"kind":"known_world","remotes":[{"name":"origin","url":"https://x"}]});
        assert_eq!(
            remote_delta("bash", &json!({"cmd":"git push origin main"}), Some(&world)),
            None
        );
        let d = remote_delta(
            "bash",
            &json!({"cmd":"git push -u myevil main"}),
            Some(&world),
        );
        assert!(d.unwrap().contains("myevil"));
        // 快照缺失 → 无结论
        assert!(remote_delta("bash", &json!({"cmd":"git push x main"}), None).is_none());
    }
}
