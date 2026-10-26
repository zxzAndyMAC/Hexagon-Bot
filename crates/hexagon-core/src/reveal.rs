//! 「在访达中显示」的路径判定（owner-followups-1015 #07；components A16b「在访达中显示」，
//! spec 2026-10-17 grilling 定稿）。
//!
//! 文件树右键把一条项目相对路径交给宿主，宿主调系统文件管理器揭示它。揭示本身只读，
//! 但它会把负责人的视线引到磁盘上某处——被诱导揭示项目外的文件（`../../.ssh`、指向
//! 仓外的符号链接）是这里要拦的事。所以根约束在 core 判，不交给插件或界面。
//!
//! 判定 = 两步规范化后比前缀：
//! 1. 词法：拒 NUL 与绝对路径（含 `C:\`、`\\server`、前导 `/` `\`），按段消解 `.` / `..`；
//!    任何一步越过根即拒。
//! 2. 落盘：对 `根/词法结果` 做 `canonicalize`（解开符号链接），结果必须仍以规范化后的
//!    根开头。不存在的路径拒（揭示不存在的东西没有意义，也省得宿主去猜父目录）。
//!
//! 与 `files.rs` 不同：那里是写盘围栏，见 `..` 就拒；这里先词法消解 `..` 再落盘解链接，
//! 因为第 2 步对**最终**路径再判一次根内——词法消解只决定「指的是哪条」，不决定放行。
//! `link_out/..` 这类「先解链接再 ..」与「先 .. 再解链接」含义不同的输入，按词法含义
//! 取（= 根内的那条），仍落在根内，不构成逃逸。
//!
//! 代价模型（fail-closed）：false negative（放行越界）= 一次被诱导的仓外揭示；
//! false positive（拒一条怪路径）= 负责人自己去访达里点一次。实现偏向拒绝：
//! 根本身 canonicalize 失败、任何 I/O 错误都按拒绝处理。
//!
//! 被否决：只做词法判定（不解链接）——树上会列出仓内的符号链接行，指向仓外时揭示的
//! 就是仓外文件；交给 tauri-plugin-opener 的 `revealItemInDir` 自带作用域——作用域是
//! 静态配置，管不到「当前项目根」，且判定会离开 core、测不到。

use std::path::{Component, Path, PathBuf};

/// 揭示被拒的原因（IPC 码面见 `errcode.rs`：`reveal_outside_project` / `reveal_missing`）。
#[derive(Debug, thiserror::Error)]
pub enum RevealError {
    /// 规范化后不在项目根内（含绝对路径、`..` 越根、符号链接指向仓外）。
    #[error("path is outside the project: {0}")]
    RevealOutsideProject(String),
    /// 项目内没有这条路径（或根本身读不到）。
    #[error("no such path in the project: {0}")]
    RevealMissing(String),
    /// 系统文件管理器没能打开（宿主侧失败）。
    #[error("could not open the file manager: {0}")]
    RevealLaunch(String),
}

/// 根约束判定本身的拒绝原因（[`resolve_in_root`]）。只有两种——越界、不存在；不含「启动失败」，
/// 那是揭示 / 打开方式各自宿主侧的事。ticket 33（2026-10-25 批次审查 S10）：原来直接返 [`RevealError`]，
/// 「打开方式」复用时不得不把 `RevealLaunch` 这个判定永远不会产生的变体也映射一遍（映到「缺失」，语义是错的）。
/// 不进 IPC，调用方各自映射到自己的错误面（`reveal_*` / `open_with_*`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InRootError {
    /// 规范化后不在项目根内（含绝对路径、`..` 越根、符号链接指向仓外）。
    Outside(String),
    /// 项目内没有这条路径（或根本身读不到）。
    Missing(String),
}

impl From<InRootError> for RevealError {
    fn from(e: InRootError) -> Self {
        match e {
            InRootError::Outside(p) => RevealError::RevealOutsideProject(p),
            InRootError::Missing(p) => RevealError::RevealMissing(p),
        }
    }
}

/// 词法规范化：`rel` 相对根，返回消解 `.` / `..` 后的段；越根、绝对、含 NUL → `None`。
/// 空串 / `.` = 根本身（`Some(vec![])`）。分隔符 `/` 与 `\` 同等对待（Windows 树路径）。
pub fn normalize_rel(rel: &str) -> Option<Vec<String>> {
    if rel.contains('\0') || looks_absolute(rel) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s.to_string()),
        }
    }
    Some(out)
}

/// 任一平台上看起来像绝对路径的都算绝对：前导分隔符、盘符（`C:`）、
/// 以及本平台 `Path::is_absolute` / 带前缀的形式。
fn looks_absolute(rel: &str) -> bool {
    if rel.starts_with(['/', '\\']) {
        return true;
    }
    let b = rel.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    let p = Path::new(rel);
    p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
}

/// 根约束判定：返回可交给系统揭示的规范化绝对路径。
/// 拒绝记 Warn 诊断（分类「拒绝」，分支 `reveal_path`，码 = 错误码）。
pub fn reveal_target(root: &Path, rel: &str) -> Result<PathBuf, RevealError> {
    let started = std::time::Instant::now();
    let out = resolve_in_root(root, rel).map_err(RevealError::from);
    if let Err(e) = &out {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "reveal_path",
            &crate::errcode::variant_code(e),
            started,
        );
    }
    out
}

/// 同一根约束判定，不写诊断——给别的入口复用（#31「打开方式」用它判路径，诊断分支记自己的）。
pub fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf, InRootError> {
    let shown = || rel.trim().to_string();
    let segs = normalize_rel(rel.trim()).ok_or_else(|| InRootError::Outside(shown()))?;
    let base = std::fs::canonicalize(root).map_err(|_| InRootError::Missing(shown()))?;
    let mut lexical = base.clone();
    lexical.extend(&segs);
    let real = match std::fs::canonicalize(&lexical) {
        Ok(p) => p,
        // 悬空链接 / 不存在：区分不出来也不必区分——都没有东西可揭示。
        Err(_) => return Err(InRootError::Missing(shown())),
    };
    if real.starts_with(&base) {
        Ok(real)
    } else {
        Err(InRootError::Outside(shown()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::fs;

    /// 模型仓：根下 a/（含 b/ 与 f.txt）、top.md、link_in → a、link_out → 仓外目录。
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("a/f.txt"), "x").unwrap();
        fs::write(root.join("top.md"), "x").unwrap();
        fs::write(outside.join("secret"), "x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("a"), root.join("link_in")).unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link_out")).unwrap();
        }
        (tmp, root)
    }

    fn code(r: Result<PathBuf, RevealError>) -> String {
        crate::errcode::variant_code(&r.unwrap_err())
    }

    #[test]
    fn in_root_paths_resolve_to_canonical_targets() {
        let (_t, root) = fixture();
        let base = fs::canonicalize(&root).unwrap();
        assert_eq!(
            reveal_target(&root, "a/f.txt").unwrap(),
            base.join("a/f.txt")
        );
        assert_eq!(reveal_target(&root, "a/b/").unwrap(), base.join("a/b"));
        assert_eq!(
            reveal_target(&root, "./a/../top.md").unwrap(),
            base.join("top.md")
        );
        assert_eq!(
            reveal_target(&root, "a\\f.txt").unwrap(),
            base.join("a/f.txt")
        );
        assert_eq!(reveal_target(&root, "").unwrap(), base);
    }

    #[test]
    fn escapes_are_refused_with_registered_code() {
        let (_t, root) = fixture();
        for rel in [
            "..",
            "../outside/secret",
            "a/../../outside",
            "/etc/passwd",
            "\\x",
            "C:\\Windows",
            "a\0b",
        ] {
            assert_eq!(
                code(reveal_target(&root, rel)),
                "reveal_outside_project",
                "{rel:?}"
            );
        }
        assert_eq!(code(reveal_target(&root, "nope.txt")), "reveal_missing");
    }

    #[cfg(unix)]
    #[test]
    fn root_guard_reports_only_outside_or_missing() {
        // ticket 33（2026-10-25 批次审查 S10）：根约束判定有自己的错误类型，只有越界 / 缺失；
        // 揭示把它映到 reveal_*，「打开方式」映到 open_with_*，谁都不再借对方的「启动失败」变体。
        let (_t, root) = fixture();
        assert_eq!(
            resolve_in_root(&root, "../outside"),
            Err(InRootError::Outside("../outside".into()))
        );
        assert_eq!(
            resolve_in_root(&root, "nope.txt"),
            Err(InRootError::Missing("nope.txt".into()))
        );
        assert_eq!(
            crate::errcode::variant_code(&RevealError::from(InRootError::Missing("x".into()))),
            "reveal_missing"
        );
    }

    #[test]
    fn symlinks_are_judged_after_resolution() {
        let (_t, root) = fixture();
        let base = fs::canonicalize(&root).unwrap();
        assert_eq!(
            reveal_target(&root, "link_in/f.txt").unwrap(),
            base.join("a/f.txt")
        );
        assert_eq!(
            code(reveal_target(&root, "link_out")),
            "reveal_outside_project"
        );
        assert_eq!(
            code(reveal_target(&root, "link_out/secret")),
            "reveal_outside_project"
        );
    }

    #[test]
    fn refusal_writes_a_warn_reject_record() {
        // max_level 是进程全局：与其他记录用例同锁排队，设「关着」（Warn 仍落盘）。
        let _g = crate::diag::TEST_LEVEL_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        log::set_max_level(log::LevelFilter::Warn);
        let (_t, root) = fixture();
        let _ = reveal_target(&root, "../outside");
        let rs = crate::diag::records(Some(crate::PROJECT_ID), Some(crate::diag::CLASS_REJECT));
        let hit = rs
            .iter()
            .rev()
            .find(|r| r.branch == "reveal_path")
            .expect("reject record");
        assert_eq!(hit.level, "warn");
        assert_eq!(hit.code, "reveal_outside_project");
    }

    /// 独立的词法模型：深度计数，越根即 None——不复用实现的段消解。
    fn model_depth(parts: &[&str]) -> Option<Vec<String>> {
        let mut stack: Vec<String> = Vec::new();
        for p in parts {
            match *p {
                "" | "." => {}
                ".." => {
                    if stack.is_empty() {
                        return None;
                    }
                    stack.pop();
                }
                s => stack.push(s.to_string()),
            }
        }
        Some(stack)
    }

    fn seg() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec![
            "a", "b", "f.txt", "top.md", "link_in", "link_out", "secret", "..", ".", "",
        ])
    }

    proptest! {
        // D12：新判定面配属性测试。不变量 = 允许 ⇔ 规范化后在根内。
        #[test]
        fn allowed_iff_normalized_path_stays_in_root(
            parts in prop::collection::vec(seg(), 0..7),
            sep in prop::sample::select(vec!["/", "\\"]),
            lead in prop::sample::select(vec!["", "/", "\\", "C:"]),
        ) {
            let (_t, root) = fixture();
            let base = fs::canonicalize(&root).unwrap();
            let rel = format!("{lead}{}", parts.join(sep));
            let got = reveal_target(&root, &rel);
            // 模型的「绝对」：显式前缀，或首段为空导致的前导分隔符（种子 parts=["","a"]）。
            let absolute = !lead.is_empty() || rel.starts_with(['/', '\\']);
            let expect_in_root = if !absolute {
                model_depth(&parts).and_then(|segs| {
                    let mut p = base.clone();
                    p.extend(&segs);
                    fs::canonicalize(&p).ok().map(|real| (real.starts_with(&base), real))
                })
            } else {
                None
            };
            match (got, expect_in_root) {
                (Ok(p), Some((true, real))) => {
                    prop_assert!(p.starts_with(&base));
                    prop_assert_eq!(p, real);
                }
                (Err(e), Some((false, _))) | (Err(e), None) => {
                    let c = crate::errcode::variant_code(&e);
                    prop_assert!(c == "reveal_outside_project" || c == "reveal_missing", "{}", c);
                }
                (got, want) => prop_assert!(false, "rel={:?} got={:?} want={:?}", rel, got, want),
            }
        }

        /// 任意字节串（含 NUL、盘符、UNC）都不会得到根外的放行。
        #[test]
        fn arbitrary_input_never_leaves_root(rel in ".{0,40}") {
            let (_t, root) = fixture();
            let base = fs::canonicalize(&root).unwrap();
            if let Ok(p) = reveal_target(&root, &rel) {
                prop_assert!(p.starts_with(&base), "{:?} -> {:?}", rel, p);
            }
        }
    }
}
