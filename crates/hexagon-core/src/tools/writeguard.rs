//! Reliability 15 / D09: approval binds to a complete, durable target manifest.
//! False rejection costs rereading; false acceptance overwrites an unreviewed
//! owner edit. Missing evidence, changed aliases and busy targets fail closed.
//! OS locks coordinate host writers; external editors remain outside that lock.

use super::{repo_path, Tool, ToolContext, ToolError};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Target {
    path: String,
    resolved: String,
    digest: Option<String>,
}

pub(crate) fn digest(path: &Path) -> std::io::Result<Option<String>> {
    // QA14: measurement runners and write manifests can name a DB alias too.
    crate::db::validate_generic_file_access(path)?;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target is not a file",
        ));
    }
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    Ok(Some(format!("{:x}", hash.finalize())))
}

fn targets(ctx: &ToolContext, paths: &[String]) -> Result<Vec<Target>, ToolError> {
    let root = ctx.repo_root.canonicalize()?;
    paths
        .iter()
        .map(|path| {
            let actual = repo_path(&root, path)?;
            Ok(Target {
                path: path.clone(),
                resolved: actual
                    .strip_prefix(&root)
                    .map_err(|_| ToolError::PathEscape(path.clone()))?
                    .to_string_lossy()
                    .into(),
                digest: digest(&actual)?,
            })
        })
        .collect()
}

/// Opaque terminal commands cannot enumerate their write targets. A repository
/// lease coordinates them with structured writes, including path aliases. Long
/// lived shells must finish/close before another writer proceeds (D09 review).
pub(crate) fn repository_lock(ctx: &ToolContext) -> std::io::Result<File> {
    let started = std::time::Instant::now();
    let result = (|| {
        let dir = ctx.repo_root.join(".hexagon/write-locks");
        let path = dir.join("repository.lock");
        if [&dir, &path].iter().any(|p| {
            p.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        }) {
            return Err(std::io::Error::other("write lock path is an alias"));
        }
        std::fs::create_dir_all(&dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // Reliability 15: concurrent fork/exec can briefly inherit a held
        // flock descriptor until CLOEXEC closes it. Bound acquisition waiting;
        // this retries no action and never refreshes approval evidence.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline =>
                    std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(std::fs::TryLockError::WouldBlock) => return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "repository has an active writer; stop/wait for terminal sessions before writing")),
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        Ok(file)
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "repository_write_lease",
        if result.is_err() {
            "busy_or_unavailable"
        } else {
            "acquired"
        },
        started,
    );
    result
}

fn locks(ctx: &ToolContext, _paths: &[String]) -> Result<Vec<File>, ToolError> {
    repository_lock(ctx)
        .map(|file| vec![file])
        .map_err(|e| ToolError::NotExecuted(e.to_string()))
}

fn check_scope(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    paths: &[String],
) -> Result<(), ToolError> {
    let mut current = ctx.clone();
    current.owned_globs = crate::permissions::agent_globs(db, &ctx.agent_id)?;
    for path in paths {
        let (name, logical) = if tool.name() == "artifact_write" {
            (
                "artifact_write",
                path.strip_prefix(".hexagon/").unwrap_or(path),
            )
        } else {
            ("fs_write", path.as_str())
        };
        let input = serde_json::json!({"path":logical});
        if super::FsWrite
            .builtin_deny(&serde_json::json!({"path":path}), ctx)
            .is_some()
            || crate::permissions::violates_ownership(name, &input, ctx)
            || crate::permissions::violates_ownership(name, &input, &current)
            || crate::permissions::matching_rule(db, &current, name, &input, "deny")?.is_some()
            || crate::permissions::agent_role(db, ctx).as_deref() == Some(crate::pm_route::PM_ROLE)
        {
            return Err(ToolError::NotExecuted(
                "write target no longer permitted".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn capture(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    input: &Value,
    id: &str,
) -> Result<(), ToolError> {
    let mut paths = tool.write_targets(input)?;
    if paths.is_empty() {
        return Ok(());
    }
    paths.sort();
    paths.dedup();
    let started = std::time::Instant::now();
    let result = (|| {
        check_scope(db, ctx, tool, &paths)?;
        let _locks = locks(ctx, &paths)?;
        // Recheck the activation read under the same host lock as the snapshot.
        tool.precondition(input, ctx)?;
        let expected = serde_json::to_string(&targets(ctx, &paths)?)
            .map_err(|e| ToolError::BadInput(e.to_string()))?;
        db.conn().execute("UPDATE tool_actions SET write_targets_json=?3 WHERE project_id=?1 AND id=?2 AND state='pending' AND question_id IS NULL AND write_targets_json IS NULL", rusqlite::params![ctx.project_id,id,expected])?;
        Ok(())
    })();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        ctx.stage_run_id.as_deref(),
        None,
        "write_precondition_capture",
        if result.is_err() {
            "capture_refused"
        } else {
            "captured"
        },
        started,
    );
    result
}

pub(crate) fn verify(
    db: &Db,
    ctx: &ToolContext,
    tool: &dyn Tool,
    input: &Value,
    id: &str,
) -> Result<Vec<File>, ToolError> {
    let mut paths = tool.write_targets(input)?;
    if paths.is_empty() {
        return Ok(vec![]);
    }
    paths.sort();
    paths.dedup();
    let started = std::time::Instant::now();
    let check = || -> Result<Vec<File>, ToolError> {
        let held = locks(ctx, &paths)?;
        check_scope(db, ctx, tool, &paths)?;
        let saved: Option<String> = db.conn().query_row(
            "SELECT write_targets_json FROM tool_actions WHERE project_id=?1 AND id=?2",
            [&ctx.project_id, id],
            |r| r.get(0),
        )?;
        let expected: Vec<Target> = saved
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(|| {
                ToolError::NotExecuted("legacy approval lacks target evidence".into())
            })?;
        let actual = targets(ctx, &paths)?;
        if expected != actual {
            return Err(ToolError::NotExecuted("target manifest changed".into()));
        }
        Ok(held)
    };
    match check() {
        Ok(held) => {
            crate::diag::note(
                crate::diag::CLASS_JUDGE,
                false,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "write_precondition",
                "unchanged",
                started,
            );
            Ok(held)
        }
        Err(_) => {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&ctx.project_id),
                Some(&ctx.agent_id),
                ctx.stage_run_id.as_deref(),
                None,
                "write_precondition",
                "conflict_or_missing_evidence",
                started,
            );
            Err(ToolError::WriteConflict {
                action_id: id.into(),
                path: paths.join(", "),
            })
        }
    }
}
