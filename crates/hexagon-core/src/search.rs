//! 仓内搜索（code-search-and-subagent 票 01）：文件名/正文/语义索引共用的
//! 遍历层——`repo_files` 是「被忽略的不进结果」的单一实现。
//!
//! gitignore 语义不自研：嵌套 .gitignore、`!` 反选、锚点与非锚点、目录级
//! 规则——自研必漏（OpenWorker readonly.py 曾因只支持 basename 规则把
//! `docs/private/` 漏进结果）。直接吃 `ignore` crate（rg 同款 walker），
//! `require_git(false)` 保证无 .git 的项目目录同样生效。

use serde_json::{json, Value};
use std::path::Path;

/// 文件名匹配结果上限：找文件是定位动作不是导出动作，超界说明模式太宽。
pub const FIND_CAP: usize = 100;
/// 正文命中行数上限：同上——要更多请缩小范围，不是翻页。
pub const GREP_CAP: usize = 100;
/// 单文件正文搜索体积上限：超大生成物/日志不进文本搜索（索引也不收）。
const GREP_FILE_CAP: u64 = 256 * 1024;
/// 遍历硬上限：防御畸形仓（几十万文件）拖死一次工具调用。
const WALK_CAP: usize = 50_000;
/// 语义索引收录的文件数上限（票 02）：超界部分只走文本搜索。
pub const INDEX_FILE_CAP: usize = 4_000;

/// 仓内未被忽略的文件相对路径（正斜杠分隔，与工具层路径口径一致）。
/// `.git/` 恒跳——它不是仓内容；不跟符号链接（与 `repo_path` 防逃逸同口径）。
pub fn repo_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false) // Hidden files remain searchable only after the read policy.
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .ignore(true)
        .parents(true)
        .require_git(false) // 没初始化 git 的目录 .gitignore 照样生效
        .follow_links(false)
        .filter_entry(|e| !(e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"))
        .build();
    for entry in walker.flatten() {
        if out.len() >= WALK_CAP {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if crate::tools::readable_repo_path(root, &rel).is_ok() {
            out.push(rel);
        }
    }
    out
}

/// 文件名搜索（票 01）：含通配符 → glob 匹配（相对路径或 basename 命中其一）；
/// 否则按相对路径大小写不敏感子串。返回排序后的相对路径，封顶 FIND_CAP。
pub fn find_by_name(root: &Path, pattern: &str) -> Vec<String> {
    let pat = pattern.trim();
    if pat.is_empty() {
        return Vec::new();
    }
    let has_glob = pat.contains(['*', '?', '[', ']']);
    let lower = pat.to_lowercase();
    let mut hits: Vec<String> = repo_files(root)
        .into_iter()
        .filter(|rel| {
            if has_glob {
                let base = rel.rsplit('/').next().unwrap_or(rel);
                crate::tools::glob_match(pat, rel) || crate::tools::glob_match(pat, base)
            } else {
                rel.to_lowercase().contains(&lower)
            }
        })
        .collect();
    hits.sort();
    hits.truncate(FIND_CAP);
    hits
}

/// 正文搜索（票 01）：字面量子串（不做正则——正则引擎是模型可控输入的
/// 复杂度炸弹面，字面量没有；要模式请把搜索词写准）。命中行截 ~200 字符。
/// 跳过：超体积文件、含 NUL 的二进制、被忽略文件。
pub fn grep_content(root: &Path, needle: &str) -> Vec<Value> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for rel in repo_files(root) {
        if hits.len() >= GREP_CAP {
            break;
        }
        let Ok(p) = crate::tools::readable_repo_path(root, &rel) else {
            continue;
        };
        let Ok(meta) = p.metadata() else { continue };
        if meta.len() > GREP_FILE_CAP {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if bytes[..bytes.len().min(8192)].contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if !line.contains(needle) {
                continue;
            }
            let trimmed = line.trim();
            let shown: String = trimmed.chars().take(200).collect();
            hits.push(json!({"path": rel, "line": i + 1, "text": shown}));
            if hits.len() >= GREP_CAP {
                break;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let w = |rel: &str, body: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w("src/main.rs", "fn main() {}\n// target_marker here\n");
        w("src/lib.rs", "pub fn helper() {}\n");
        w("docs/readme.md", "target_marker in docs\n");
        w("build/out.log", "target_marker in ignored\n");
        w(".gitignore", "build/\n");
        dir
    }

    #[test]
    fn find_by_name_glob_and_substr() {
        let d = fixture();
        let hits = find_by_name(d.path(), "*.rs");
        assert_eq!(hits, vec!["src/lib.rs", "src/main.rs"]);
        let hits = find_by_name(d.path(), "readme");
        assert_eq!(hits, vec!["docs/readme.md"]);
        assert!(find_by_name(d.path(), "").is_empty());
    }

    #[test]
    fn grep_returns_path_and_line() {
        let d = fixture();
        let hits = grep_content(d.path(), "target_marker");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0]["path"], "docs/readme.md");
        assert_eq!(hits[0]["line"], 1);
        assert_eq!(hits[1]["path"], "src/main.rs");
        assert_eq!(hits[1]["line"], 2);
    }

    #[test]
    fn gitignored_files_are_omitted() {
        let d = fixture();
        assert!(!find_by_name(d.path(), "*.log")
            .iter()
            .any(|p| p.contains("out.log")));
        assert!(!grep_content(d.path(), "ignored")
            .iter()
            .any(|h| h["path"].as_str().unwrap().contains("out.log")));
    }

    #[test]
    fn find_cap_enforced() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(FIND_CAP + 20) {
            std::fs::write(dir.path().join(format!("f{i:04}.txt")), "x").unwrap();
        }
        assert_eq!(find_by_name(dir.path(), "*.txt").len(), FIND_CAP);
    }

    #[test]
    fn grep_cap_and_binary_skip() {
        let dir = tempfile::tempdir().unwrap();
        let body: String = (0..(GREP_CAP + 10))
            .map(|i| format!("hit line {i}\n"))
            .collect();
        std::fs::write(dir.path().join("many.txt"), body).unwrap();
        assert_eq!(grep_content(dir.path(), "hit").len(), GREP_CAP);
        // 二进制（含 NUL）不搜——needle 只出现在 bin.dat 里，命中必须为空。
        std::fs::write(dir.path().join("bin.dat"), b"a\0b unique_needle").unwrap();
        assert!(grep_content(dir.path(), "unique_needle").is_empty());
    }
}
