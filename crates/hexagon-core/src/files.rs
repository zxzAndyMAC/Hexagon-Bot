//! 仓库文件树（hands-free 票 11 / ADR 0062）。
//!
//! 右栏「项目」页只列仓根内的一层目录；中栏编辑器读写的也是这些路径。
//! 围栏是 fail-closed：`..`、绝对路径、符号链接一律拒绝。
//! false negative（放行逃逸或跟着 symlink 写出仓根）是一次未审写盘；
//! false positive（拒一条怪路径或一个链接）是一次改名。实现偏向拒绝。
//! 不把 `..` 先规范化再比前缀——链接解析之后规范化是经典逃逸手法。
//!
//! 大小上限：内嵌编辑器只做小改（ADR 0031）。放进 webview 的巨型文件会
//! 卡死界面，比拒一次打开贵，所以超过 [`MAX_TEXT_BYTES`] 直接拒绝。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 单文件读/写上限。1 MiB：够看和改源码，不够把二进制或日志灌进编辑器。
pub const MAX_TEXT_BYTES: u64 = 1_048_576;

/// 树上的一条。`path` 相对仓根，分隔符固定 `/`（与 `repo_paths` 一致）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RepoEntry {
    pub name: String,
    pub path: String,
    pub kind: RepoEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub enum RepoEntryKind {
    File,
    Dir,
    /// 符号链接：列出来（磁盘上有它），但不展开、不打开、不写穿。
    Link,
}

/// Owner editor read, including the exact project incarnation and saved text.
/// The original text is the write precondition, not a refreshed save-time read.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct RepoFileSnapshot {
    pub path: String,
    pub content: String,
    pub project_root: String,
    #[ts(type = "number")]
    pub generation: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RepoFsError {
    #[error("path escapes repository: {0}")]
    PathEscape(String),
    #[error("not a utf-8 text file: {0}")]
    NotText(String),
    #[error("file too large to edit: {0}")]
    TooLarge(String),
    #[error("not a directory: {0}")]
    NotDir(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("project changed; reopen the file in the current project")]
    ProjectChanged,
    #[error("file changed since it was opened; saved text was not overwritten: {0}")]
    FileChanged(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// 列 `rel` 这一层（空串 = 仓根）。目录在前，其余按名字不区分大小写。
pub fn list_repo_dir(root: &Path, rel: &str) -> Result<Vec<RepoEntry>, RepoFsError> {
    let (dir, segs) = walk(root, rel)?;
    let meta = std::fs::symlink_metadata(&dir)?;
    if meta.file_type().is_symlink() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    if !meta.is_dir() {
        return Err(RepoFsError::NotDir(show(rel)));
    }
    let prefix = segs.join("/");
    let mut out = Vec::new();
    for ent in std::fs::read_dir(&dir)? {
        let ent = ent?;
        let name = ent.file_name().to_string_lossy().into_owned();
        let ft = ent.file_type()?;
        let kind = if ft.is_symlink() {
            RepoEntryKind::Link
        } else if ft.is_dir() {
            RepoEntryKind::Dir
        } else {
            RepoEntryKind::File
        };
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(RepoEntry { name, path, kind });
    }
    out.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

/// 读文本。二进制（含 NUL）、非 UTF-8、超限、链接都拒绝。
pub fn read_repo_file(root: &Path, rel: &str) -> Result<String, RepoFsError> {
    let (path, _) = walk(root, rel)?;
    require_regular_file(&path, rel)?;
    let meta = std::fs::symlink_metadata(&path)?;
    if meta.len() > MAX_TEXT_BYTES {
        return Err(RepoFsError::TooLarge(show(rel)));
    }
    let bytes = std::fs::read(&path)?;
    if bytes.contains(&0) {
        return Err(RepoFsError::NotText(show(rel)));
    }
    String::from_utf8(bytes).map_err(|_| RepoFsError::NotText(show(rel)))
}

pub fn read_repo_file_snapshot(
    root: &Path,
    generation: u64,
    rel: &str,
    expected_project_root: &str,
) -> Result<RepoFileSnapshot, RepoFsError> {
    let project_root = root.canonicalize()?.display().to_string();
    if project_root != expected_project_root {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            Some("owner"),
            None,
            None,
            "owner_editor_read",
            "project_changed",
            std::time::Instant::now(),
        );
        return Err(RepoFsError::ProjectChanged);
    }
    Ok(RepoFileSnapshot {
        path: rel.into(),
        content: read_repo_file(root, rel)?,
        project_root,
        generation,
    })
}

pub fn write_repo_file_snapshot(
    root: &Path,
    generation: u64,
    snapshot: &RepoFileSnapshot,
    content: &str,
) -> Result<(), RepoFsError> {
    let started = std::time::Instant::now();
    let result = (|| {
        // Benchmark I1 / owner decision 2026-10-06: path-only saves routed an
        // old draft into the new project. A root key alone also admits close /
        // reopen of that root. Missing/mismatched evidence must reject a save:
        // false negative costs rereading; false positive overwrites owner work.
        if generation != snapshot.generation
            || root.canonicalize()?.display().to_string() != snapshot.project_root
        {
            return Err(RepoFsError::ProjectChanged);
        }
        if content.len() as u64 > MAX_TEXT_BYTES {
            return Err(RepoFsError::TooLarge(show(&snapshot.path)));
        }
        let _lease = crate::tools::writeguard::repository_lock_for_root(root)?;
        if read_repo_file(root, &snapshot.path)? != snapshot.content {
            return Err(RepoFsError::FileChanged(show(&snapshot.path)));
        }
        let (path, _) = walk(root, &snapshot.path)?;
        let permissions = std::fs::metadata(&path)?.permissions();
        let parent = path
            .parent()
            .ok_or_else(|| RepoFsError::PathEscape(show(&snapshot.path)))?;
        // A short write / full disk must not truncate the saved original. Reuse
        // the installed tempfile mechanism used for experience publication.
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        std::io::Write::write_all(&mut temp, content.as_bytes())?;
        temp.as_file().set_permissions(permissions)?;
        temp.as_file().sync_all()?;
        // Host writers share the lease; external editors do not. Detect a
        // changed target before replacement without claiming external atomicity.
        if read_repo_file(root, &snapshot.path)? != snapshot.content {
            return Err(RepoFsError::FileChanged(show(&snapshot.path)));
        }
        temp.persist(&path).map_err(|e| RepoFsError::Io(e.error))?;
        Ok(())
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(crate::PROJECT_ID),
        Some("owner"),
        None,
        None,
        "owner_editor_save",
        match &result {
            Err(RepoFsError::ProjectChanged) => "project_changed",
            Err(RepoFsError::FileChanged(_)) => "file_changed",
            Err(_) => "save_refused",
            Ok(()) => "saved",
        },
        started,
    );
    result
}

/// 读 `rel` 在 git HEAD 的版本，作中栏文件页的对比基线。
/// 非 git 仓库、无 HEAD 提交、文件未跟踪、HEAD 版本是二进制——都回 Ok(None)：
/// 基线是增强不是门槛，UI 据此收起 HEAD 选项，而不是把文件页判死。
/// 注意：git::run 对 stdout 做 trim，文件头尾空白差异在该基线上不可见。
pub fn repo_file_at_head(root: &Path, rel: &str) -> Result<Option<String>, RepoFsError> {
    // 与 read_repo_file 同一道路径围栏：拼进 `HEAD:` rev:path 参数前先过
    // segments()，`..`/绝对路径/盘符在拼参数前就出局，路径纪律保持单点。
    let segs = segments(rel)?;
    if segs.is_empty() || !crate::git::is_repo(root) {
        return Ok(None);
    }
    let spec = format!("HEAD:{}", segs.join("/"));
    match crate::git::run(root, &["show", &spec]) {
        // HEAD 里的二进制/巨型版本同样不算可用基线——编辑器本体就拒这类
        // 文本（MAX_TEXT_BYTES），基线沿用同一条边界，不灌进 webview。
        Ok(text) if text.contains('\0') || text.len() as u64 > MAX_TEXT_BYTES => Ok(None),
        Ok(text) => Ok(Some(text)),
        // git 起不来的 IOError 是环境故障，如实上报；
        // Failed（未跟踪/无 HEAD/坏对象）一律视为「无基线」。
        Err(crate::git::GitError::Io(e)) => Err(e.into()),
        Err(_) => Ok(None),
    }
}

/// 覆写已有普通文件，或在已存在的父目录里新建。不写穿符号链接。
pub fn write_repo_file(root: &Path, rel: &str, content: &str) -> Result<(), RepoFsError> {
    if content.len() as u64 > MAX_TEXT_BYTES {
        return Err(RepoFsError::TooLarge(show(rel)));
    }
    let (path, _) = walk(root, rel)?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(RepoFsError::PathEscape(show(rel)));
        }
        Ok(meta) if meta.is_dir() => return Err(RepoFsError::NotDir(show(rel))),
        Ok(meta) if !meta.is_file() => return Err(RepoFsError::NotText(show(rel))),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ensure_parent_dir(&path, rel)?;
        }
        Err(e) => return Err(e.into()),
    }
    std::fs::write(&path, content)?;
    Ok(())
}

/// 新建空文件。已存在则拒绝，不覆盖。
pub fn create_repo_file(root: &Path, rel: &str) -> Result<(), RepoFsError> {
    let (path, segs) = walk(root, rel)?;
    if segs.is_empty() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    ensure_parent_dir(&path, rel)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(RepoFsError::AlreadyExists(show(rel)))
        }
        Err(e) => Err(e.into()),
    }
}

/// 新建目录（只一层，不 mkdir -p）。已存在则拒绝。
pub fn create_repo_dir(root: &Path, rel: &str) -> Result<(), RepoFsError> {
    let (path, segs) = walk(root, rel)?;
    if segs.is_empty() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    ensure_parent_dir(&path, rel)?;
    match std::fs::create_dir(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(RepoFsError::AlreadyExists(show(rel)))
        }
        Err(e) => Err(e.into()),
    }
}

fn rank(k: RepoEntryKind) -> u8 {
    match k {
        RepoEntryKind::Dir => 0,
        RepoEntryKind::File => 1,
        RepoEntryKind::Link => 2,
    }
}

fn show(rel: &str) -> String {
    let t = rel.trim().trim_matches('/');
    if t.is_empty() {
        ".".to_string()
    } else {
        t.to_string()
    }
}

/// 逐段拼接。任何 `..` / 绝对路径 / 中途符号链接都在碰到目标之前拒绝。
fn walk(root: &Path, rel: &str) -> Result<(PathBuf, Vec<String>), RepoFsError> {
    let segs = segments(rel)?;
    let mut cur = root.to_path_buf();
    for seg in &segs {
        cur.push(seg);
        if let Ok(meta) = std::fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return Err(RepoFsError::PathEscape(show(rel)));
            }
        }
    }
    // QA14: the owner editor used to read SHM before rejecting its NUL bytes,
    // losing SQLite's live locks. The same guard protects write/create paths.
    crate::db::validate_generic_file_access(&cur)?;
    Ok((cur, segs))
}

fn segments(rel: &str) -> Result<Vec<String>, RepoFsError> {
    let rel = rel.trim();
    if rel.contains('\0') || Path::new(rel).is_absolute() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    let mut out = Vec::new();
    for seg in rel.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(RepoFsError::PathEscape(show(rel)));
        }
        out.push(seg.to_string());
    }
    Ok(out)
}

fn require_regular_file(path: &Path, rel: &str) -> Result<(), RepoFsError> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    if meta.is_dir() {
        return Err(RepoFsError::NotDir(show(rel)));
    }
    if !meta.is_file() {
        return Err(RepoFsError::NotText(show(rel)));
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path, rel: &str) -> Result<(), RepoFsError> {
    let Some(parent) = path.parent() else {
        return Err(RepoFsError::PathEscape(show(rel)));
    };
    let meta = std::fs::symlink_metadata(parent)?;
    if meta.file_type().is_symlink() {
        return Err(RepoFsError::PathEscape(show(rel)));
    }
    if !meta.is_dir() {
        return Err(RepoFsError::NotDir(show(rel)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // Benchmark I1 / 2026-10-06: an old editor targeted the current root;
    // remounting alone loses its draft and still cannot reject a delayed IPC.
    proptest::proptest! {
        #[test]
        fn editor_snapshot_never_overwrites_another_project_or_incarnation(
            original in "[a-z]{0,64}", replacement in "[A-Z]{1,64}", same_root in proptest::bool::ANY,
        ) {
            let a = root();
            let b = root();
            fs::write(a.path().join("README.md"), &original).unwrap();
            fs::write(b.path().join("README.md"), "B owner file").unwrap();
            let expected_root = a.path().canonicalize().unwrap().display().to_string();
            let snapshot = read_repo_file_snapshot(a.path(), 1, "README.md", &expected_root).unwrap();
            let target = if same_root { a.path() } else { b.path() };
            let generation = if same_root { 2 } else { 1 };
            proptest::prop_assert!(write_repo_file_snapshot(target, generation, &snapshot, &replacement).is_err());
            proptest::prop_assert_eq!(read_repo_file(a.path(), "README.md").unwrap(), original);
            proptest::prop_assert_eq!(read_repo_file(b.path(), "README.md").unwrap(), "B owner file");
        }

        #[test]
        fn editor_snapshot_preserves_newer_saved_text(
            original in "[a-z]{0,64}", replacement in "[A-Z]{1,64}",
        ) {
            let dir = root();
            fs::write(dir.path().join("README.md"), &original).unwrap();
            let expected = dir.path().canonicalize().unwrap().display().to_string();
            let snapshot = read_repo_file_snapshot(dir.path(), 7, "README.md", &expected).unwrap();
            fs::write(dir.path().join("README.md"), &replacement).unwrap();
            proptest::prop_assert!(matches!(
                write_repo_file_snapshot(dir.path(), 7, &snapshot, "owner draft"),
                Err(RepoFsError::FileChanged(_))
            ));
            proptest::prop_assert_eq!(read_repo_file(dir.path(), "README.md").unwrap(), replacement);
        }
    }

    #[test]
    fn editor_snapshot_save_reuses_host_lease_and_refreshes_only_after_success() {
        let dir = root();
        fs::write(dir.path().join("README.md"), "original").unwrap();
        let expected = dir.path().canonicalize().unwrap().display().to_string();
        let mut snapshot = read_repo_file_snapshot(dir.path(), 7, "README.md", &expected).unwrap();
        let lease = crate::tools::writeguard::repository_lock_for_root(dir.path()).unwrap();
        assert!(write_repo_file_snapshot(dir.path(), 7, &snapshot, "draft").is_err());
        assert_eq!(read_repo_file(dir.path(), "README.md").unwrap(), "original");
        drop(lease);
        write_repo_file_snapshot(dir.path(), 7, &snapshot, "draft").unwrap();
        assert!(matches!(
            write_repo_file_snapshot(dir.path(), 7, &snapshot, "next draft"),
            Err(RepoFsError::FileChanged(_))
        ));
        snapshot.content = "draft".into();
        write_repo_file_snapshot(dir.path(), 7, &snapshot, "next draft").unwrap();
        assert_eq!(
            read_repo_file(dir.path(), "README.md").unwrap(),
            "next draft"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2); // README and shared .hexagon lease, no leaked temp.
        assert!(matches!(
            read_repo_file_snapshot(dir.path(), 7, "README.md", "/stale/root"),
            Err(RepoFsError::ProjectChanged)
        ));
    }

    #[test]
    fn list_matches_directory_hierarchy() {
        let dir = root();
        let root = dir.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("README.md"), "# hi\n").unwrap();
        let top = list_repo_dir(root, "").unwrap();
        assert_eq!(top[0].name, "src");
        assert_eq!(top[0].kind, RepoEntryKind::Dir);
        assert_eq!(top[0].path, "src");
        assert!(top
            .iter()
            .any(|e| e.name == "README.md" && e.kind == RepoEntryKind::File));
        let kids = list_repo_dir(root, "src").unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].path, "src/main.rs");
        assert_eq!(kids[0].kind, RepoEntryKind::File);
    }

    #[test]
    fn create_then_list_matches_disk() {
        let dir = root();
        let root = dir.path();
        create_repo_dir(root, "docs").unwrap();
        create_repo_file(root, "docs/note.md").unwrap();
        write_repo_file(root, "docs/note.md", "hello\n").unwrap();
        let top = list_repo_dir(root, "").unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].kind, RepoEntryKind::Dir);
        let kids = list_repo_dir(root, "docs/").unwrap();
        assert_eq!(kids[0].path, "docs/note.md");
        assert_eq!(read_repo_file(root, "docs/note.md").unwrap(), "hello\n");
        assert_eq!(
            fs::read_to_string(root.join("docs/note.md")).unwrap(),
            "hello\n"
        );
        assert!(create_repo_file(root, "docs/note.md").is_err());
    }

    #[test]
    fn escape_shapes_never_leave_root() {
        let dir = root();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "nope").unwrap();
        for rel in [
            "../x",
            "..",
            "/etc/passwd",
            "a/../../x",
            "a/../b",
            "./../x",
            "foo/../../../etc/passwd",
        ] {
            assert!(list_repo_dir(root, rel).is_err(), "{rel}");
            assert!(read_repo_file(root, rel).is_err(), "{rel}");
            assert!(create_repo_file(root, rel).is_err(), "{rel}");
            assert!(create_repo_dir(root, rel).is_err(), "{rel}");
            assert!(write_repo_file(root, rel, "x").is_err(), "{rel}");
        }
        assert!(!outside.path().join("x").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
            "nope"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_listed_but_not_followed() {
        let dir = root();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "nope").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("alias")).unwrap();
        let top = list_repo_dir(root, "").unwrap();
        assert!(top.iter().all(|e| e.kind == RepoEntryKind::Link));
        assert!(matches!(
            list_repo_dir(root, "link").unwrap_err(),
            RepoFsError::PathEscape(_)
        ));
        assert!(read_repo_file(root, "link/secret.txt").is_err());
        assert!(write_repo_file(root, "alias", "pwned").is_err());
        assert!(create_repo_file(root, "link/evil.txt").is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
            "nope"
        );
        assert!(!outside.path().join("evil.txt").exists());
    }

    #[test]
    fn read_refuses_binary_and_oversize() {
        let dir = root();
        let root = dir.path();
        fs::write(root.join("bin.dat"), b"a\0b").unwrap();
        assert!(matches!(
            read_repo_file(root, "bin.dat").unwrap_err(),
            RepoFsError::NotText(_)
        ));
        let big = "a".repeat(MAX_TEXT_BYTES as usize + 1);
        fs::write(root.join("big.txt"), &big).unwrap();
        assert!(matches!(
            read_repo_file(root, "big.txt").unwrap_err(),
            RepoFsError::TooLarge(_)
        ));
        assert!(write_repo_file(root, "big.txt", &big).is_err());
    }

    #[test]
    fn repo_file_at_head_reads_committed_version() {
        let dir = root();
        let repo = dir.path();
        fs::create_dir(repo.join("src")).unwrap();
        fs::write(repo.join("src/a.rs"), "old\n").unwrap();
        crate::git::init(repo, "main").unwrap();
        crate::git::commit_all(repo, "add a").unwrap();
        // 工作区改动不影响 HEAD 基线
        fs::write(repo.join("src/a.rs"), "new\n").unwrap();
        // git::run 对 stdout trim，断言对齐这个边界。
        assert_eq!(
            repo_file_at_head(repo, "src/a.rs").unwrap().as_deref(),
            Some("old")
        );
        // 未跟踪文件没有基线
        fs::write(repo.join("b.txt"), "x").unwrap();
        assert_eq!(repo_file_at_head(repo, "b.txt").unwrap(), None);
        // 路径越界照旧硬拒
        assert!(matches!(
            repo_file_at_head(repo, "../x").unwrap_err(),
            RepoFsError::PathEscape(_)
        ));
        // 非 git 目录一律无基线
        let plain = tempfile::tempdir().unwrap();
        fs::write(plain.path().join("a.txt"), "x").unwrap();
        assert_eq!(repo_file_at_head(plain.path(), "a.txt").unwrap(), None);
    }

    #[test]
    fn kind_serializes_snake() {
        let e = RepoEntry {
            name: "a.rs".into(),
            path: "a.rs".into(),
            kind: RepoEntryKind::File,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "file");
    }
}
