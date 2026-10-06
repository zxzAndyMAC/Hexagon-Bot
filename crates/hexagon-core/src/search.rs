//! 仓内搜索（code-search-and-subagent 票 01）：文件名/正文/语义索引共用的
//! 遍历层——`repo_files` 供相似索引，`search` 供流式范围搜索；
//! 两者遵守 ignore 遍历语义和同一读取权限边界。
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
/// 目录搜索逐文件体积上限，避免广搜批量读取大生成物/日志。
const GREP_FILE_CAP: u64 = 256 * 1024;
/// 2026-09-30 p08: real test source exceeds 256KiB. Explicit file scope
/// permits bounded follow-up search, not an unlimited whole-repo cap increase.
const FOCUSED_GREP_FILE_CAP: u64 = 8 * 1024 * 1024;
/// 文本搜索限制回调可见条目；旧相似索引遍历限制收录文件数。
/// ignore 内部丢弃的条目不计数；停止为协作式，不承诺硬中断文件系统。
const WALK_CAP: usize = 50_000;
/// 语义索引收录的文件数上限（票 02）：超界部分只走文本搜索。
pub const INDEX_FILE_CAP: usize = 4_000;

/// Shared ignore policy for both scoped searches and the compatibility index.
fn repo_walker(root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false);
    builder
}

pub(crate) struct Discovery {
    pub paths: Vec<String>,
    pub entries_visited: usize,
    pub excluded_by_policy: usize,
    pub reason: Option<&'static str>,
}

/// Benchmark R7 (2026-10-06): the old eager walker swallowed errors and ran
/// before the stop check. Bound callback-visible entries (including excluded
/// files/directories), stop at the index file cap, and refuse broken ignore rules.
/// Checkpoints are cooperative between walker yields; filesystem/ignore internals
/// cannot be hard-interrupted, just as in scoped search.
pub(crate) fn repo_files_checked(
    root: &Path,
    file_cap: usize,
    check: &dyn Fn() -> Result<(), crate::tools::ToolError>,
) -> Result<Discovery, crate::tools::ToolError> {
    use crate::tools::ToolError;
    check()?;
    let root = root.canonicalize()?;
    if !root.metadata()?.is_dir() || file_cap == 0 {
        return Err(ToolError::BadInput(
            "index discovery requires a directory and positive cap".into(),
        ));
    }
    check()?;
    let state = std::sync::Arc::new(std::sync::Mutex::new(Traversal {
        entries: 1,
        limited: false,
        error: None,
    }));
    let filter_state = state.clone();
    let walker = repo_walker(&root)
        .filter_entry(move |entry| {
            let mut state = filter_state.lock().unwrap();
            if state.entries >= WALK_CAP {
                state.limited = true;
                return true; // yield instead of silently scanning more siblings
            }
            state.entries += 1;
            !(entry.file_type().is_some_and(|t| t.is_dir()) && entry.file_name() == ".git")
        })
        .build();
    let mut found = Discovery {
        paths: Vec::new(),
        entries_visited: 1,
        excluded_by_policy: 0,
        reason: None,
    };
    for entry in walker {
        check()?;
        if state.lock().unwrap().limited {
            found.reason = Some("entry_limit");
            break;
        }
        let entry =
            entry.map_err(|error| ToolError::Exec(format!("index discovery failed: {error}")))?;
        if entry.error().is_some() {
            return Err(ToolError::Exec(
                "repository search cannot apply ignore rules; repair the ignore file".into(),
            ));
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&root)
            .expect("walker stays in root")
            .to_string_lossy()
            .replace('\\', "/");
        match crate::tools::agent_readable_repo_path(&root, &rel) {
            Ok(_) => {}
            Err(ToolError::BadInput(_) | ToolError::PathEscape(_)) => {
                found.excluded_by_policy += 1;
                continue;
            }
            Err(error) => return Err(error),
        }
        found.paths.push(rel);
        if found.paths.len() >= file_cap {
            found.reason = Some("file_limit");
            break;
        }
    }
    check()?;
    found.entries_visited = state.lock().unwrap().entries;
    Ok(found)
}

// Compatibility for the opt-in real-project measurement only. Failed or capped
// discovery must stop that measurement, not print a misleading complete count.
#[cfg(test)]
pub fn repo_files(root: &Path) -> Vec<String> {
    let found =
        repo_files_checked(root, WALK_CAP, &|| Ok(())).expect("measurement discovery failed");
    assert!(found.reason.is_none(), "measurement discovery truncated");
    found.paths
}

/// 2026-09-29 owner decision: scoped exploration must prune before caps, not
/// filter an already truncated whole-repo result. Existing read policy remains
/// authoritative; a rejected scope is an error, never a whole-repo fallback.
pub enum Query<'a> {
    Find(&'a str),
    Grep(&'a str),
}

pub fn search(
    root: &Path,
    query: Query<'_>,
    scope: &str,
    glob: Option<&str>,
    checkpoint: impl Fn() -> Result<(), crate::tools::ToolError> + Send + Sync + 'static,
) -> Result<Value, crate::tools::ToolError> {
    use crate::tools::ToolError;
    use std::io::Read;
    checkpoint()?;
    // 2026-09-29 regression: macOS /var aliases /private/var. Compare the
    // canonical scope with a canonical walker root, including direct core calls.
    let root = root.canonicalize()?;
    let root = root.as_path();
    let mut scoped = root.to_path_buf();
    // Reuse the read boundary, additionally refusing symlink scope aliases:
    // starting a walker at a symlink could otherwise bypass ignore rules.
    for part in Path::new(scope).components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                scoped.push(name);
                if std::fs::symlink_metadata(&scoped)?.file_type().is_symlink() {
                    return Err(ToolError::BadInput(
                        "search path must not contain symlinks".into(),
                    ));
                }
            }
            _ => {
                return Err(ToolError::BadInput(
                    "search path must be repo-relative without parent components".into(),
                ))
            }
        }
    }
    let scoped = crate::tools::agent_readable_repo_path(root, scope)?;
    let file_cap = if scoped.is_file() {
        FOCUSED_GREP_FILE_CAP
    } else {
        GREP_FILE_CAP
    };
    let needle = match query {
        Query::Find(s) => s.trim(),
        Query::Grep(s) => s,
    };
    if needle.is_empty() {
        return Err(ToolError::BadInput("search query must not be empty".into()));
    }
    let has_glob = needle.contains(['*', '?', '[', ']']);
    let lower = needle.to_lowercase();
    let name_glob = if has_glob && matches!(query, Query::Find(_)) {
        Some(search_glob(needle)?)
    } else {
        None
    };
    let filter_glob = glob.map(search_glob).transpose()?;
    let within = scoped.clone();
    let checkpoint = std::sync::Arc::new(checkpoint);
    let filter_check = checkpoint.clone();
    let traversal = std::sync::Arc::new(std::sync::Mutex::new(Traversal {
        entries: 1, // ignore does not call filter_entry for the root.
        limited: false,
        error: None,
    }));
    let filter_state = traversal.clone();
    // Walk from root, rather than scope, so root and intermediate ignore files
    // still apply even when the requested subtree itself is ignored.
    let walker = repo_walker(root)
        .filter_entry(move |entry| {
            // Returning true on stop makes the iterator yield immediately;
            // returning false would keep scanning rejected siblings internally.
            let mut state = filter_state.lock().unwrap();
            if let Err(error) = filter_check() {
                state.error = Some(error);
                return true;
            }
            if state.entries >= WALK_CAP {
                state.limited = true;
                return true;
            }
            state.entries += 1;
            let p = entry.path();
            !(entry.file_type().is_some_and(|t| t.is_dir()) && entry.file_name() == ".git")
                && (p.starts_with(&within) || within.starts_with(p))
        })
        .build();
    let mut paths = Vec::new();
    let mut hits = Vec::new();
    let mut files_searched = 0;
    let mut oversized = 0;
    let mut large_paths = Vec::new();
    let mut binary = 0;
    let mut unreadable = 0;
    let mut excluded = 0;
    let mut reason = None;
    for entry in walker {
        checkpoint()?;
        {
            let mut state = traversal.lock().unwrap();
            if let Some(error) = state.error.take() {
                return Err(error);
            }
            if state.limited {
                reason = Some("entry_limit");
                break;
            }
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        if entry.error().is_some() {
            return Err(ToolError::Exec(
                "repository search cannot apply ignore rules; repair the ignore file".into(),
            ));
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .expect("walker stays in root")
            .to_string_lossy()
            .replace('\\', "/");
        if filter_glob
            .as_ref()
            .is_some_and(|pat| !matches_glob(pat, &rel))
        {
            continue;
        }
        let path = match crate::tools::agent_readable_repo_path(root, &rel) {
            Ok(p) => p,
            Err(_) => {
                excluded += 1;
                continue;
            }
        };
        match query {
            Query::Find(_) => {
                if if let Some(pat) = &name_glob {
                    matches_glob(pat, &rel)
                } else {
                    rel.to_lowercase().contains(&lower)
                } {
                    paths.push(rel);
                    if paths.len() >= FIND_CAP {
                        reason = Some("hit_limit");
                        break;
                    }
                }
            }
            Query::Grep(_) => {
                let meta = match path.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        unreadable += 1;
                        continue;
                    }
                };
                if meta.len() > file_cap {
                    oversized += 1;
                    // 2026-09-30 live p08: counts alone hid a skipped test file.
                    // Sample only paths that already passed the read policy.
                    if large_paths.len() < 8 {
                        large_paths.push(rel.clone());
                    }
                    continue;
                }
                let mut bytes = Vec::new();
                // Bounded even if a file grows between metadata and read.
                let read = std::fs::File::open(&path)
                    .and_then(|f| f.take(file_cap + 1).read_to_end(&mut bytes));
                if read.is_err() {
                    unreadable += 1;
                    continue;
                }
                checkpoint()?;
                if bytes.len() as u64 > file_cap {
                    oversized += 1;
                    // 2026-09-30 live p08: counts alone hid a skipped test file.
                    // Sample only paths that already passed the read policy.
                    if large_paths.len() < 8 {
                        large_paths.push(rel.clone());
                    }
                    continue;
                }
                if bytes.contains(&0) {
                    binary += 1;
                    continue;
                }
                files_searched += 1;
                let text = String::from_utf8_lossy(&bytes);
                for (i, line) in text.lines().enumerate() {
                    if !line.contains(needle) {
                        continue;
                    }
                    hits.push(json!({"path":rel,"line":i+1,"text":line.trim().chars().take(200).collect::<String>()}));
                    if hits.len() >= GREP_CAP {
                        reason = Some("hit_limit");
                        break;
                    }
                }
                if reason.is_some() {
                    break;
                }
            }
        }
    }
    checkpoint()?;
    paths.sort();
    let coverage = json!({
        "entries_visited":traversal.lock().unwrap().entries, "files_searched":files_searched,
        "skipped_large":oversized, "skipped_large_paths_sample":large_paths,
        "skipped_large_sample_truncated":oversized > large_paths.len(), "skipped_binary":binary,
        "skipped_unreadable":unreadable, "excluded_by_policy":excluded,
        "truncated":reason.is_some(), "reason":reason,
        "complete":reason.is_none() && oversized == 0 && binary == 0 && unreadable == 0 && excluded == 0,
        "scope":scope,
        "note":"Coverage applies only to the requested scope/filter and non-ignored files. Skipped files can be source code or tests. Search relevant skipped_large_paths_sample files by setting path to that exact file (up to 8MiB), then use fs_read offset/limit; the sample is not exhaustive when skipped_large_sample_truncated is true. Narrow capped searches. Empty hits do not prove absence, even when complete within a scope/filter."
    });
    Ok(match query {
        Query::Find(_) => json!({"count":paths.len(),"paths":paths,"coverage":coverage}),
        Query::Grep(_) => json!({"count":hits.len(),"hits":hits,"coverage":coverage}),
    })
}

struct Traversal {
    entries: usize,
    limited: bool,
    error: Option<crate::tools::ToolError>,
}

// Reuse ignore's existing globset dependency instead of the recursive permission
// glob matcher: model-supplied search patterns must not cause exponential work.
fn search_glob(pattern: &str) -> Result<globset::GlobMatcher, crate::tools::ToolError> {
    if pattern.len() > 1024 {
        return Err(crate::tools::ToolError::BadInput(
            "search glob exceeds 1024 bytes".into(),
        ));
    }
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|_| crate::tools::ToolError::BadInput("invalid search glob".into()))
}
fn matches_glob(pattern: &globset::GlobMatcher, rel: &str) -> bool {
    pattern.is_match(rel) || pattern.is_match(rel.rsplit('/').next().unwrap_or(rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn find_by_name(root: &Path, pattern: &str) -> Vec<String> {
        if pattern.is_empty() {
            return Vec::new();
        }
        serde_json::from_value(
            search(root, Query::Find(pattern), ".", None, || Ok(())).unwrap()["paths"].clone(),
        )
        .unwrap()
    }
    fn grep_content(root: &Path, query: &str) -> Vec<Value> {
        search(root, Query::Grep(query), ".", None, || Ok(())).unwrap()["hits"]
            .as_array()
            .unwrap()
            .clone()
    }

    // Complements the Workbench stop tests at the existing core search seam:
    // stop deterministically during pruning, without sleeps or production hooks.
    #[test]
    fn scoped_search_stops_while_pruning_unrelated_siblings() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..100 {
            std::fs::create_dir(dir.path().join(format!("d{i}"))).unwrap();
        }
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let result = search(dir.path(), Query::Grep("absent"), "d0", None, move || {
            if observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 10 {
                return Err(crate::tools::ToolError::Exec(
                    "fixture stopped during traversal".into(),
                ));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 11);
    }

    #[test]
    fn index_discovery_stops_between_visible_entries() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..100 {
            std::fs::create_dir(dir.path().join(format!("d{i}"))).unwrap();
        }
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let found = repo_files_checked(dir.path(), INDEX_FILE_CAP, &|| {
            if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 10 {
                return Err(crate::tools::ToolError::Exec("discovery stopped".into()));
            }
            Ok(())
        });
        assert!(found.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 11);
    }

    proptest::proptest! {
        #[test]
        fn capped_index_discovery_never_claims_exhaustion(total in 1usize..24, cap in 1usize..16) {
            let dir = tempfile::tempdir().unwrap();
            for i in 0..total {
                std::fs::write(dir.path().join(format!("f{i}.rs")), "marker").unwrap();
            }
            let found = repo_files_checked(dir.path(), cap, &|| Ok(())).unwrap();
            proptest::prop_assert_eq!(found.paths.len(), total.min(cap));
            proptest::prop_assert_eq!(found.reason, if total >= cap { Some("file_limit") } else { None });
            proptest::prop_assert!(found.entries_visited <= cap + 1);
        }
    }

    // Opt-in performance evidence: directory creation excluded from timing.
    #[test]
    #[ignore = "creates 50100 files; run explicitly for index discovery performance evidence"]
    fn index_discovery_large_repository_benchmark() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..50100 {
            std::fs::write(dir.path().join(format!("f{i}.rs")), "marker").unwrap();
        }
        for trial in 0..6 {
            let started = std::time::Instant::now();
            let found = repo_files_checked(dir.path(), INDEX_FILE_CAP, &|| Ok(())).unwrap();
            assert_eq!(found.paths.len(), INDEX_FILE_CAP);
            assert_eq!(found.reason, Some("file_limit"));
            assert_eq!(found.entries_visited, INDEX_FILE_CAP + 1);
            println!(
                "index_discovery trial={trial} elapsed_ms={:.2} files={} entries={} reason={:?}",
                started.elapsed().as_secs_f64() * 1000.,
                found.paths.len(),
                found.entries_visited,
                found.reason
            );
            let calls = std::sync::atomic::AtomicUsize::new(0);
            let stopped = std::time::Instant::now();
            let result = repo_files_checked(dir.path(), INDEX_FILE_CAP, &|| {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 100 {
                    Err(crate::tools::ToolError::Exec("fixture stopped".into()))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            println!(
                "index_discovery_cancel trial={trial} elapsed_ms={:.2} checkpoints={}",
                stopped.elapsed().as_secs_f64() * 1000.,
                calls.load(std::sync::atomic::Ordering::SeqCst)
            );
        }
    }

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
