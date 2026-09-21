//! 文本指令与 composer token 解析（中立模块）。
//!
//! 被门面（api.rs）与回合内核（turn.rs 流中过滤控制消息）共用；
//! 本模块只依赖 db/trace，不依赖 api——arch-review 票 01 前这些函数住在
//! api.rs 里，导致 turn.rs 反向依赖门面（诊断卡 D08）。

use crate::db::Db;
use crate::trace::{MessageToken, TraceError};

/// composer 的 @/# 解析：[@名字]→mention，[#路径]→path 指针。
pub fn parse_tokens(body: &str) -> Vec<MessageToken> {
    let mut out = Vec::new();
    for word in body.split_whitespace() {
        if let Some(name) = word.strip_prefix('@') {
            if !name.is_empty() {
                out.push(MessageToken::Mention {
                    agent_role: name.to_string(),
                });
            }
        } else if let Some(p) = word.strip_prefix('#') {
            if !p.is_empty() {
                out.push(MessageToken::PathRef {
                    path: p.to_string(),
                });
            }
        }
    }
    out
}

/// composer `#` 路径补全（ui-audit-2 票 09）：仓根的有界遍历 +
/// 子串匹配，返回 ≤ limit 条相对路径（目录带尾 `/`）。
/// 边界：不越仓根（symlink 不跟随）、跳高噪目录、访问总量封顶——
/// 大仓上补全不拖 UI；权限/隐藏语义不管（这是补全提示不是授权）。
pub fn repo_paths(repo_root: &std::path::Path, query: &str, limit: usize) -> Vec<String> {
    const VISIT_CAP: usize = 20_000; // 遍历总量保险丝
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        ".hexagon",
        "dist",
        "build",
        ".next",
        "__pycache__",
    ];
    let q = query.trim_start_matches('#').to_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut visited = 0usize;
    // BFS：浅层路径先返回——补全列表要的是「src/」先于「src/deep/nested/x」
    let mut dirs = std::collections::VecDeque::from([repo_root.to_path_buf()]);
    while let Some(dir) = dirs.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            visited += 1;
            if visited > VISIT_CAP || out.len() >= limit {
                return out;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".github" {
                continue; // 隐藏目录默认跳过（.github 例外：workflow 常被引）
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue; // 不跟符号链接——防环与越仓根
            }
            let rel = e
                .path()
                .strip_prefix(repo_root)
                .unwrap_or(&e.path())
                .to_string_lossy()
                .to_string();
            if ft.is_dir() {
                if SKIP.contains(&name.as_str()) {
                    continue;
                }
                let d = format!("{rel}/");
                if q.is_empty() || d.to_lowercase().contains(&q) {
                    out.push(d);
                }
                dirs.push_back(e.path());
            } else if q.is_empty() || rel.to_lowercase().contains(&q) {
                out.push(rel);
            }
        }
    }
    out
}

/// 回合期旁路写入（turn-streaming 票 04/05）：dispatch 持 wb 锁跑回合时，
/// owner 消息经独立的第二条 Db 连接落库——不然干预永远排在回合后面，
/// steering 与流中叫停只剩空壳。指令只解析不分发，分发归调用方选通道
/// （Pause/Resume 可以走同一条旁路 db；其余指令仍该排队等 wb）。
pub fn send_message_side(
    db: &Db,
    project_id: &str,
    body: &str,
) -> Result<(i64, Option<TextCommand>), TraceError> {
    let tokens = parse_tokens(body);
    let id = db.append_message(project_id, "owner", body, &tokens, None, None)?;
    Ok((id, parse_command(body)))
}

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    Orch(#[from] crate::orchestra::OrchError),
}

/// 控制通道发消息（ADR 0052 路由表，arch-review 票 05）：消息落库 +
/// 即时指令（Pause/Resume）在控制连接就地生效——回合占着 wb 锁时
/// steering 与叫停照常落。其余指令（rewind/stamp/skip/override/install）
/// 返回给调用方走 wb 组分发。壳层因此只剩「conn 调本函数 → 余下给 wb」。
pub fn send_via_control(
    db: &Db,
    project_id: &str,
    body: &str,
) -> Result<(i64, Option<TextCommand>), RouteError> {
    let (id, cmd) = send_message_side(db, project_id, body)?;
    let rest = match cmd {
        Some(TextCommand::Pause) => {
            crate::orchestra::pause(db, project_id)?;
            None
        }
        Some(TextCommand::Resume) => {
            crate::orchestra::resume(db, project_id)?;
            None
        }
        other => other,
    };
    Ok((id, rest))
}

/// 文本指令（与按钮同权同痕）：只认整句命令，防普通语句被劫持。
/// pub：壳层旁路写入路径（票 05 send_message_side）要按变体分流。
#[derive(Debug, PartialEq)]
pub enum TextCommand {
    Rewind(Option<usize>),
    Skip,
    Stamp,
    Pause,
    Resume,
    SleepAll,
    Override(String),
    Install(String),
}

/// 整句匹配：命令词 + 可选数字参数，多余文本一律不算命令。
/// 双轨（ADR 0051）：`/verb` 规范式全语言通用；本地化斜杠别名（/退回 /盖章）也收；
/// 裸词（退回/盖章…）保留为过渡形态。pub：壳层旁路写入要复用同一解析。
pub fn parse_command(body: &str) -> Option<TextCommand> {
    let b = body.trim();
    // /verb 规范式 + 本地化斜杠别名（退回参数走同一数字尾巴规则）
    if let Some(rest) = b.strip_prefix('/') {
        let (verb, arg) = match rest.split_once(' ') {
            Some((v, a)) => (v, a.trim()),
            None => (rest, ""),
        };
        return match verb {
            "rewind" | "退回" | "回退" => {
                if arg.is_empty() {
                    Some(TextCommand::Rewind(None))
                } else {
                    arg.trim_start_matches('到')
                        .parse::<usize>()
                        .ok()
                        .map(|n| TextCommand::Rewind(Some(n)))
                }
            }
            "skip" | "跳过" if arg.is_empty() => Some(TextCommand::Skip),
            "stamp" | "盖章" | "通过" if arg.is_empty() => Some(TextCommand::Stamp),
            "pause" | "暂停" if arg.is_empty() => Some(TextCommand::Pause),
            "resume" | "恢复" if arg.is_empty() => Some(TextCommand::Resume),
            "sleep" | "sleep_all" | "休眠" | "全员休眠" if arg.is_empty() => {
                Some(TextCommand::SleepAll)
            }
            // /override <理由>：理由必填，空理由不算指令（落普通消息）
            "override" | "覆盖" if !arg.is_empty() => {
                Some(TextCommand::Override(arg.to_string()))
            }
            // /install <描述>：描述必填，空描述落普通消息
            "install" | "安装" if !arg.is_empty() => Some(TextCommand::Install(arg.to_string())),
            _ => None, // 未知 /verb 或多余参数 → 普通消息，不吞
        };
    }
    match b {
        "跳过" => return Some(TextCommand::Skip),
        "盖章" | "通过" => return Some(TextCommand::Stamp),
        "暂停" => return Some(TextCommand::Pause),
        "恢复" => return Some(TextCommand::Resume),
        "休眠" | "全员休眠" => return Some(TextCommand::SleepAll),
        "退回" | "回退" | "退回上一阶段" | "回退上一阶段" => {
            return Some(TextCommand::Rewind(None))
        }
        _ => {}
    }
    // 「退回 2」「回退到 1」：前缀 + 纯数字尾巴才算命令，其他尾巴不劫持
    for prefix in ["退回", "回退"] {
        if let Some(rest) = b.strip_prefix(prefix) {
            let rest = rest.trim().trim_start_matches('到').trim();
            if let Ok(n) = rest.parse::<usize>() {
                return Some(TextCommand::Rewind(Some(n)));
            }
            return None; // 「退回」打头但尾巴不是数字 → 普通消息
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_commands_parse_both_rails() {
        // /verb 规范式 + 本地化斜杠别名，与裸词同权
        assert_eq!(parse_command("/skip"), Some(TextCommand::Skip));
        assert_eq!(parse_command("/stamp"), Some(TextCommand::Stamp));
        assert_eq!(parse_command("/pause"), Some(TextCommand::Pause));
        assert_eq!(parse_command("/resume"), Some(TextCommand::Resume));
        assert_eq!(parse_command("/sleep"), Some(TextCommand::SleepAll));
        assert_eq!(parse_command("/rewind"), Some(TextCommand::Rewind(None)));
        assert_eq!(
            parse_command("/rewind 2"),
            Some(TextCommand::Rewind(Some(2)))
        );
        assert_eq!(parse_command("/盖章"), Some(TextCommand::Stamp));
        assert_eq!(parse_command("/跳过"), Some(TextCommand::Skip));
        assert_eq!(parse_command("/退回 1"), Some(TextCommand::Rewind(Some(1))));
        // 未知 verb 与非法参数不劫持——算普通消息
        assert_eq!(parse_command("/dance"), None);
        assert_eq!(parse_command("/rewind abc"), None);
        assert_eq!(parse_command("/skip 多余尾巴"), None);
    }

    /// 票 05：旁路写入——回合持有的主连接之外，壳层开第二条 Db 连接落
    /// owner 消息并解析指令；WAL 下主连接立即可读（send_message_side 是
    /// src-tauri send_message/pause/resume 在回合进行中的真实通道）。
    #[test]
    fn send_message_side_visible_from_main_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let main = Db::open(&path).unwrap();
        main.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        // 第二条连接 = 壳层旁路；写 /pause 与普通消息
        let side = Db::open(&path).unwrap();
        let (id, cmd) = send_message_side(&side, "p1", "/pause").unwrap();
        assert_eq!(cmd, Some(TextCommand::Pause));
        let body: String = main
            .conn()
            .query_row("SELECT body FROM messages WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "/pause");
        let (_id2, cmd2) = send_message_side(&side, "p1", "普通插话").unwrap();
        assert_eq!(cmd2, None);
        // owner_message 事件也随 append_message 落了——主连接看得到
        let n: i64 = main
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='owner_message'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    /// ADR 0052 三通道：主连接（=wb 回合侧）握着未提交写事务时，
    /// 控制连接的读不阻塞（WAL 快照语义，只见已提交前缀）；
    /// 主连接提交后，控制连接的写/读双方互相可见。
    /// busy_timeout=5s——若读取真去排队等写者，耗时断言必超阈值。
    #[test]
    fn control_conn_reads_while_main_holds_write_txn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let main = Db::open(&path).unwrap();
        main.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p1','/tmp/x','x','pack')",
                [],
            )
            .unwrap();
        let ctrl = Db::open(&path).unwrap();

        // 主连接开写事务但不提交——模拟回合侧长事务占着写者位
        main.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
        main.conn()
            .execute(
                "INSERT INTO messages (project_id, author, body) VALUES ('p1','a1','未提交')",
                [],
            )
            .unwrap();

        // 控制连接的读走快照：立即返回且看不到未提交行
        let t = std::time::Instant::now();
        let items = ctrl.timeline("p1", None, 100, None).unwrap();
        assert!(
            t.elapsed() < std::time::Duration::from_secs(2),
            "timeline 被主连接的写事务阻塞了 {:?}",
            t.elapsed()
        );
        assert!(items.is_empty(), "快照读不该看见未提交消息");

        // 主连接提交后：控制连接写入 → 主连接立即可见（既有测试同语义）
        main.conn().execute_batch("COMMIT").unwrap();
        let (id, _) = send_message_side(&ctrl, "p1", "落库").unwrap();
        let body: String = main
            .conn()
            .query_row("SELECT body FROM messages WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "落库");
    }

    #[test]
    fn repo_paths_lists_dirs_and_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("src/nested/deep.rs"), "").unwrap();
        std::fs::write(root.join("AGENTS.md"), "").unwrap();
        let got = repo_paths(root, "", 60);
        assert!(got.contains(&"src/".to_string()));
        assert!(got.contains(&"src/main.rs".to_string()));
        assert!(got.contains(&"src/nested/".to_string()));
        assert!(got.contains(&"src/nested/deep.rs".to_string()));
        assert!(got.contains(&"AGENTS.md".to_string()));
        // BFS：浅层先返回
        let pos = |p: &str| got.iter().position(|x| x == p).unwrap();
        assert!(pos("src/") < pos("src/nested/"));
    }

    #[test]
    fn repo_paths_skips_noise_and_honors_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for d in [
            "node_modules/pkg",
            ".git/objects",
            ".secret",
            "target/debug",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::write(root.join(d).join("x.rs"), "").unwrap();
        }
        std::fs::write(root.join("keep.rs"), "").unwrap();
        let got = repo_paths(root, "", 60);
        assert_eq!(got, vec!["keep.rs".to_string()]);
        // limit 截断
        std::fs::write(root.join("a.rs"), "").unwrap();
        std::fs::write(root.join("b.rs"), "").unwrap();
        let capped = repo_paths(root, "", 2);
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn repo_paths_query_matches_case_insensitive_substring() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("Src")).unwrap();
        std::fs::write(root.join("Src/Main.RS"), "").unwrap();
        std::fs::write(root.join("README"), "").unwrap();
        let got = repo_paths(root, "main", 60);
        assert_eq!(got, vec!["Src/Main.RS".to_string()]);
        // 前导 # 容忍（composer 传入可能带井号）
        let got2 = repo_paths(root, "#main", 60);
        assert_eq!(got2, got);
    }

    #[cfg(unix)]
    #[test]
    fn repo_paths_does_not_follow_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
        std::fs::write(root.join("ok.rs"), "").unwrap();
        let got = repo_paths(root, "", 60);
        assert_eq!(got, vec!["ok.rs".to_string()]); // 逃逸链接既不列也不穿透
    }

    #[test]
    fn token_parsing() {
        let t = parse_tokens("继续 @后端 参考 #src/main.rs 谢谢");
        assert_eq!(
            t,
            vec![
                MessageToken::Mention {
                    agent_role: "后端".into()
                },
                MessageToken::PathRef {
                    path: "src/main.rs".into()
                }
            ]
        );
    }
}
