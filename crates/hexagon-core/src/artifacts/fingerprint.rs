//! D08 / reliability 18: checks attest tracked and untracked worktree inputs.
//! Ignore files cannot hide source; host state and explicit output directories
//! are excluded, but tracked files and required deliverables override outputs.
use crate::{db::Db, tools::repo_path};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io, path::Path};

// D08: only declared root output/cache locations; src/build is source, not
// an output merely because one component happens to share a name.
const OUTPUT_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "coverage",
    ".cache",
    ".next",
    ".turbo",
    ".venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
];

pub(crate) fn capture(
    db: &Db,
    root: &Path,
    project: &str,
    run: &str,
    required: &[String],
) -> io::Result<String> {
    let root = root.canonicalize()?;
    let mut inputs = source_inputs(&root)?;
    let mut query=db.conn().prepare("SELECT id,path,kind,author_agent_id,version FROM artifacts WHERE project_id=?1 AND stage_run_id=?2 AND status IN ('valid','stamped') AND kind IN (SELECT value FROM json_each(?3)) ORDER BY id").map_err(io::Error::other)?;
    let required = serde_json::to_string(required).map_err(io::Error::other)?;
    let artifacts = query
        .query_map(rusqlite::params![project, run, required], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(io::Error::other)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    for (_, path, kind, _, _) in &artifacts {
        let path = path.trim_start_matches('/');
        inputs.collect(&root.join(format!(".hexagon/{path}")), true)?;
        if kind == "代码" || super::evidence::runnable_source(path) {
            inputs.collect(&root.join(path), true)?;
        }
    }
    let bytes = serde_json::to_vec(&(1, inputs.files, artifacts)).map_err(io::Error::other)?;
    Ok(format!("v1:{:x}", Sha256::digest(bytes)))
}

/// Same source inventory as delivery fingerprints, without artifact metadata.
/// Quality scopes must not use a weaker ignore/glob view than acceptance.
pub(crate) fn source_manifest(root: &Path) -> io::Result<BTreeMap<String, (String, u32)>> {
    let root = root.canonicalize()?;
    Ok(source_inputs(&root)?.files)
}

fn source_inputs(root: &Path) -> io::Result<Inputs<'_>> {
    let mut inputs = Inputs {
        root,
        files: BTreeMap::new(),
        bytes: 0,
    };
    inputs.collect(root, false)?;
    // --cached ignores ignore rules; -z preserves spaces/newlines without Git
    // quoting. Disable fsmonitor so a repository hook is never run while reading.
    if root.ancestors().any(|p| p.join(".git").exists()) {
        let out = std::process::Command::new("git")
            .args(["-c", "core.fsmonitor=false", "ls-files", "--cached", "-z"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_CONFIG_COUNT")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other("cannot enumerate tracked check inputs"));
        }
        for path in out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            let path = std::str::from_utf8(path).map_err(io::Error::other)?;
            if !host_path(Path::new(path)) {
                inputs.collect(&root.join(path), true)?;
            }
        }
    }
    Ok(inputs)
}

fn host_path(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|c| matches!(c.as_os_str().to_str(), Some(".git" | ".hexagon")))
}
struct Inputs<'a> {
    root: &'a Path,
    files: BTreeMap<String, (String, u32)>,
    bytes: u64,
}
impl Inputs<'_> {
    fn collect(&mut self, path: &Path, explicit: bool) -> io::Result<()> {
        let relative = path.strip_prefix(self.root).map_err(io::Error::other)?;
        if !explicit
            && (host_path(relative)
                || relative.components().next().is_some_and(|c| {
                    c.as_os_str()
                        .to_str()
                        .is_some_and(|s| OUTPUT_DIRS.contains(&s))
                }))
        {
            return Ok(());
        }
        let key = relative
            .to_str()
            .ok_or_else(|| io::Error::other("non-UTF8 check input"))?
            .to_string();
        if self.files.contains_key(&key) {
            return Ok(());
        }
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if explicit && e.kind() == io::ErrorKind::NotFound => {
                self.files.insert(key, ("missing".into(), 0));
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                self.collect(&entry?.path(), explicit)?;
            }
            return Ok(());
        }
        if self.files.len() >= 100_000 {
            return Err(io::Error::other("check inputs exceed fingerprint limits"));
        }
        let actual = repo_path(self.root, &key).map_err(io::Error::other)?;
        let (digest, bytes, mode) = hash_input(&actual, 512 * 1024 * 1024 - self.bytes)?;
        self.bytes += bytes;
        let digest = if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(path)?;
            format!(
                "link:{}:{digest}",
                link.to_str()
                    .ok_or_else(|| io::Error::other("non-UTF8 link target"))?
            )
        } else {
            digest
        };
        self.files.insert(key, (digest, mode));
        Ok(())
    }
}

// Reliability-18 review: a symlink's own length did not bound its target read.
// Bound the opened file's actual bytes too, including growth after metadata.
fn hash_input(path: &Path, remaining: u64) -> io::Result<(String, u64, u32)> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > remaining {
        return Err(io::Error::other("check input exceeds fingerprint limits"));
    }
    let mut reader = file.take(remaining + 1);
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > remaining {
            return Err(io::Error::other(
                "check input grew beyond fingerprint limits",
            ));
        }
        hash.update(&buffer[..n]);
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    };
    #[cfg(not(unix))]
    let mode = u32::from(metadata.permissions().readonly());
    Ok((format!("{:x}", hash.finalize()), bytes, mode))
}

// False negative costs another check; false positive accepts unchecked work.
// Missing/legacy evidence never equals a current fingerprint.
pub(crate) fn current(recorded: Option<&str>, stable: bool, now: Option<&str>) -> bool {
    stable && recorded.zip(now).is_some_and(|(a, b)| a == b)
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn changed_or_unproven_inputs_never_attest(value in ".{0,70}") {
            let old=format!("old:{value}");let new=format!("new:{value}");
            prop_assert!(!current(Some(&old),true,Some(&new)));
            prop_assert!(!current(None,true,Some(&new)));
            prop_assert!(!current(Some(&old),false,Some(&old)));
            prop_assert!(current(Some(&old),true,Some(&old)));
        }
    }
}
