//! Task-benefit-evaluation 01: host-owned task acceptance, independent of turns.
//! Only scripted debug execution is admitted until isolation and quota gates land.
use crate::{db::Db, sessions::SessionTable, tools::ToolContext};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationTask {
    pub id: String,
    pub category: String,
    pub source: String,
    pub license: String,
    pub revision: String,
    pub requirements: String,
    pub allowed_paths: Vec<String>,
    pub files: BTreeMap<String, String>,
    /// Entry path in host-owned validation_files; never a mutable task command.
    pub validator: String,
    pub validation_files: BTreeMap<String, String>,
    pub reference: BTreeMap<String, String>,
    pub wrong: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub command: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub before: String,
    pub after: String,
    pub passed: bool,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub version: u32,
    pub id: String,
    pub task_id: String,
    pub task_fingerprint: String,
    pub workspace: String,
    pub evidence_kind: String,
    pub state: String,
    pub flow_completed: bool,
    pub independent_passed: bool,
    pub owner_exception: bool,
    pub acceptance: Option<Acceptance>,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub(crate) fn safe_path(path: &str) -> bool {
    // D02/D18: reject ambiguous/host paths. False rejection costs one corrected
    // fixture; false acceptance can overwrite host evidence or the daily tree.
    !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && p != ".hexagon" && p != ".git")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

pub(crate) fn validate(task: &EvaluationTask) -> io::Result<()> {
    let started = Instant::now();
    let valid = [
        &task.id,
        &task.category,
        &task.source,
        &task.license,
        &task.revision,
        &task.requirements,
        &task.validator,
    ]
    .iter()
    .all(|s| !s.trim().is_empty())
        && task.validation_files.contains_key(&task.validator)
        && task.validation_files.keys().all(|p| {
            safe_path(p) && !task.allowed_paths.contains(p) && !task.files.contains_key(p)
        })
        && !task.files.is_empty()
        && !task.allowed_paths.is_empty()
        && task
            .files
            .keys()
            .chain(task.allowed_paths.iter())
            .chain(task.reference.keys())
            .chain(task.wrong.keys())
            .all(|p| safe_path(p))
        && task
            .reference
            .keys()
            .chain(task.wrong.keys())
            .all(|p| task.allowed_paths.contains(p));
    crate::diag::note(
        if valid {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !valid,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_manifest",
        if valid { "valid" } else { "invalid" },
        started,
    );
    if !valid {
        return Err(err("invalid evaluation manifest"));
    }
    Ok(())
}

pub(crate) fn materialize(root: &Path, files: &BTreeMap<String, String>) -> io::Result<()> {
    for (path, body) in files {
        if !safe_path(path) {
            return Err(err("unsafe evaluation path"));
        }
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().ok_or_else(|| err("missing parent"))?)?;
        std::fs::write(target, body)?;
    }
    Ok(())
}

pub(crate) fn fingerprint(root: &Path) -> io::Result<String> {
    fn visit(root: &Path, here: &Path, out: &mut BTreeMap<String, String>) -> io::Result<()> {
        for entry in std::fs::read_dir(here)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(root).map_err(err)?;
            if rel
                .components()
                .next()
                .is_some_and(|c| c.as_os_str() == ".hexagon" || c.as_os_str() == ".git")
            {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_symlink() {
                return Err(err("symlink in evaluation inputs"));
            }
            if meta.is_dir() {
                visit(root, &path, out)?;
            } else if meta.is_file() {
                if meta.len() > 16 * 1024 * 1024 || out.len() >= 10000 {
                    return Err(err("evaluation snapshot exceeds limits"));
                }
                out.insert(
                    rel.to_string_lossy().into_owned(),
                    format!(
                        "{}:{:x}",
                        permissions(&meta),
                        Sha256::digest(std::fs::read(path)?)
                    ),
                );
            } else {
                return Err(err("special file in evaluation inputs"));
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(format!(
        "v1:{:x}",
        Sha256::digest(serde_json::to_vec(&files)?)
    ))
}

fn permissions(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        u32::from(meta.permissions().readonly())
    }
}

fn now_ms() -> io::Result<u64> {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_millis();
    u64::try_from(ms).map_err(err)
}

fn acceptance_passed(exit: i64, timeout: bool, stable: bool, preserved: bool) -> bool {
    // D06/D18: false negatives require another investigation; false positives
    // can attest a tampered delivery. Missing/unstable evidence fails closed.
    exit == 0 && !timeout && stable && preserved
}

fn copy_inputs(from: &Path, to: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if matches!(entry.file_name().to_str(), Some(".hexagon" | ".git")) {
            continue;
        }
        let source = entry.path();
        let target = to.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&source)?;
        if meta.is_symlink() {
            return Err(err("symlink in verification copy"));
        }
        if meta.is_dir() {
            std::fs::create_dir(&target)?;
            copy_inputs(&source, &target)?;
        } else if meta.is_file() {
            std::fs::copy(source, target)?;
        } else {
            return Err(err("special file in verification copy"));
        }
    }
    Ok(())
}

pub(crate) fn accept(db: &Db, root: &Path, task: &EvaluationTask) -> io::Result<Acceptance> {
    let started = Instant::now();
    let started_at_ms = now_ms()?;
    let before = fingerprint(root)?;
    // Evaluation 01 review: running `sh check.sh` in the delivered tree let
    // an agent replace its own acceptance with `exit 0`. Host validation files
    // are disjoint from mutable inputs and are never placed in the worker tree.
    let verification = tempfile::tempdir()?;
    copy_inputs(root, verification.path())?;
    if fingerprint(verification.path())? != before {
        return Err(err("inputs changed while copying for acceptance"));
    }
    for path in task.validation_files.keys() {
        if verification.path().join(path).exists() {
            return Err(err("delivery overlaps host validator"));
        }
    }
    materialize(verification.path(), &task.validation_files)?;
    let verifier_before = fingerprint(verification.path())?;
    let command = format!("sh '{}'", task.validator.replace('\'', "'\\''"));
    let ctx = ToolContext::owner(db, verification.path());
    let output = SessionTable::default()
        .run_oneshot(db, &ctx, &command, Duration::from_secs(30), false)
        .map_err(err)?;
    let after = fingerprint(root)?;
    let verifier_after = fingerprint(verification.path())?;
    let exit_code = output["exit_code"].as_i64().unwrap_or(-1);
    let timed_out = output["timed_out"].as_bool().unwrap_or(true);
    let preserved = task
        .files
        .iter()
        .filter(|(p, _)| !task.allowed_paths.contains(p))
        .all(|(p, body)| std::fs::read_to_string(root.join(p)).ok().as_ref() == Some(body));
    let passed = acceptance_passed(
        exit_code,
        timed_out,
        before == after && verifier_before == verifier_after,
        preserved,
    );
    crate::diag::note(
        if passed {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !passed,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_acceptance",
        if passed {
            "passed"
        } else {
            "failed_or_unstable"
        },
        started,
    );
    Ok(Acceptance {
        command,
        exit_code,
        stdout: output["stdout"].as_str().unwrap_or("").into(),
        stderr: output["stderr"].as_str().unwrap_or("").into(),
        timed_out,
        before,
        after,
        passed,
        started_at_ms,
        finished_at_ms: now_ms()?,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

pub(crate) fn self_check(task: &EvaluationTask) -> io::Result<()> {
    validate(task)?;
    let started = Instant::now();
    for (patch, expected) in [
        (None, false),
        (Some(&task.reference), true),
        (Some(&task.wrong), false),
    ] {
        let copy = tempfile::tempdir()?;
        materialize(copy.path(), &task.files)?;
        if let Some(patch) = patch {
            materialize(copy.path(), patch)?;
        }
        let db = Db::open_in_memory().map_err(err)?;
        if accept(&db, copy.path(), task)?.passed != expected {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(crate::PROJECT_ID),
                None,
                None,
                None,
                "evaluation_self_check",
                "unexpected_outcome",
                started,
            );
            return Err(err("task self-check disagrees with declared outcome"));
        }
    }
    Ok(())
}

pub(crate) fn insert(db: &Db, task: &EvaluationTask, result: &EvaluationResult) -> io::Result<()> {
    db.conn()
        .execute(
            "INSERT INTO evaluation_runs (id, task_json, result_json) VALUES (?1,?2,?3)",
            rusqlite::params![
                result.id,
                serde_json::to_string(task)?,
                serde_json::to_string(result)?
            ],
        )
        .map_err(err)?;
    Ok(())
}

pub(crate) fn finish(db: &Db, result: &EvaluationResult) -> io::Result<()> {
    let changed = db.conn().execute("UPDATE evaluation_runs SET result_json=?2 WHERE id=?1 AND json_extract(result_json,'$.state')='started'", rusqlite::params![result.id,serde_json::to_string(result)?]).map_err(err)?;
    if changed != 1 {
        return Err(err("evaluation result already terminal or missing"));
    }
    Ok(())
}

pub(crate) fn read(db: &Db, id: &str) -> io::Result<EvaluationResult> {
    let json: String = db
        .conn()
        .query_row(
            "SELECT result_json FROM evaluation_runs WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(err)?;
    let result: EvaluationResult = serde_json::from_str(&json)?;
    if result.version != 1 || result.id != id {
        return Err(err("unsupported or corrupt evaluation result"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::acceptance_passed;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn removing_acceptance_evidence_never_passes(exit in any::<i64>(), timeout in any::<bool>(), stable in any::<bool>(), preserved in any::<bool>()) {
            prop_assert!(!acceptance_passed(exit, true, stable, preserved));
            prop_assert!(!acceptance_passed(exit, timeout, false, preserved));
            prop_assert!(!acceptance_passed(exit, timeout, stable, false));
            if exit != 0 { prop_assert!(!acceptance_passed(exit, timeout, stable, preserved)); }
        }
    }
}
