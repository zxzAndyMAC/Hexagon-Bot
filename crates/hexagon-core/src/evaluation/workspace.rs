//! Evaluation 05: reconstruct only fresh owned copies; never clean the daily tree.
use super::{err, fingerprint, materialize, EvaluationTask};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io, path::Path, process::Command};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBaseline {
    pub head: String,
    /// Evaluation05 review: -v preserves assume-unchanged/skip-worktree flags,
    /// which can otherwise hide owner edits without changing staged hashes.
    pub index_entries: String,
    pub config_fingerprint: String,
}

fn git(root: &Path, args: &[&str]) -> io::Result<String> {
    // D02/D18: inherited GIT_DIR, templates, filters or signing hooks could
    // touch the daily repository. These commands operate only on our new copy.
    let out = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_AUTHOR_NAME", "Evaluation fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@localhost")
        .env("GIT_COMMITTER_NAME", "Evaluation fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@localhost")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .current_dir(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(err(String::from_utf8_lossy(&out.stderr)));
    }
    String::from_utf8(out.stdout).map_err(err)
}

pub(crate) fn effective_files(task: &EvaluationTask) -> BTreeMap<String, String> {
    let mut files = task.files.clone();
    files.extend(task.initial_changes.clone());
    files.extend(task.untracked_files.clone());
    files
}

fn config_fingerprint(root: &Path) -> io::Result<String> {
    let directory = root.join(".git");
    let metadata = directory.symlink_metadata()?;
    if !metadata.is_dir()
        || metadata.is_symlink()
        || directory.join("objects/info/alternates").exists()
    {
        return Err(err("ambiguous evaluation Git directory"));
    }
    // Scan before asking Git to interpret metadata: no links/special files or
    // oversized external-object inventories can turn a check into host access.
    fingerprint(&directory)?;
    let config = directory.join("config");
    Ok(format!(
        "{}:{:x}",
        super::permissions(&config.metadata()?),
        Sha256::digest(std::fs::read(config)?)
    ))
}

pub(crate) fn reconstruct(root: &Path, task: &EvaluationTask) -> io::Result<Option<GitBaseline>> {
    materialize(root, &task.files)?;
    if task.initial_changes.is_empty() && task.untracked_files.is_empty() {
        return Ok(None);
    }
    git(
        root,
        &["-c", "init.templateDir=", "init", "-b", "evaluation"],
    )?;
    let mut args = vec!["add", "--force", "--"];
    args.extend(task.files.keys().map(String::as_str));
    git(root, &args)?;
    git(
        root,
        &["commit", "--no-verify", "-m", "Evaluation fixture baseline"],
    )?;
    let baseline = GitBaseline {
        head: git(root, &["rev-parse", "HEAD"])?,
        index_entries: git(root, &["ls-files", "-v", "--stage", "-z"])?,
        config_fingerprint: config_fingerprint(root)?,
    };
    materialize(root, &task.initial_changes)?;
    materialize(root, &task.untracked_files)?;
    Ok(Some(baseline))
}

pub(crate) fn git_preserved(root: &Path, baseline: &GitBaseline) -> bool {
    // False rejection costs review; accepting a changed index loses the owner's
    // uncommitted state. Do not silently repair or reset the delivered tree.
    (|| -> io::Result<bool> {
        if config_fingerprint(root)? != baseline.config_fingerprint {
            return Ok(false);
        }
        Ok(git(root, &["rev-parse", "HEAD"])? == baseline.head
            && git(root, &["ls-files", "-v", "--stage", "-z"])? == baseline.index_entries)
    })()
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        // D12/Evaluation05: preserving bytes is insufficient if the index or
        // baseline changed. No owner edit can be silently staged or committed.
        #[test]
        fn staging_or_committing_owner_work_invalidates_start_state(body in "[a-z]{1,20}", action in 0u8..4) {
            let corpus: super::super::EvaluationCorpus = serde_json::from_str(include_str!("../../../../evaluation/corpus/dirty-trees.json")).unwrap();
            let mut task = corpus.cases[0].task.clone();
            task.initial_changes.insert("notes.md".into(), body);
            let root = tempfile::tempdir().unwrap();
            let baseline = reconstruct(root.path(), &task).unwrap().unwrap();
            prop_assert!(git_preserved(root.path(), &baseline));
            match action {
                0 | 1 => {
                    git(root.path(), &["add", "--", "notes.md"]).unwrap();
                    if action == 1 { git(root.path(), &["commit", "--no-verify", "-m", "incorrectly commit owner work"]).unwrap(); }
                }
                2 => { git(root.path(), &["update-index", "--assume-unchanged", "notes.md"]).unwrap(); }
                _ => { git(root.path(), &["update-index", "--skip-worktree", "notes.md"]).unwrap(); }
            }
            prop_assert!(!git_preserved(root.path(), &baseline));
        }
    }
}
