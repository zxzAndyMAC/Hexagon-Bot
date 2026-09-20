//! git 管道（票 14）：短生命周期 `git` CLI 子进程，无常驻。
//!
//! - 项目工作全在工作分支（`projects.work_branch`，默认 hexagon/work）；
//! - 本地落地 = 工作分支合入基线，双闸：盖章点全过（无 waiting_stamp /
//!   未决 stamp 问题）+ 安全网必问（`git_baseline_merge` 天生安全网，永不记忆）；
//! - **指针回拨不动 git**：rewind 只改 stage_runs，代码回滚是独立显式操作。

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use crate::db::Db;
use crate::tools::{Tool, ToolContext, ToolError};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git {0}: {1}")]
    Cli(String, String),
    #[error("not a git repo: {0}")]
    NotRepo(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// 跑一次 git，成功返回 stdout。失败带 stderr 报错。
pub fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    log::debug!(
        "git -C {:?} {}",
        repo.file_name().unwrap_or_default(),
        args.join(" ")
    );
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        log::warn!("git {} failed: {err}", args.join(" "));
        Err(GitError::Cli(args.join(" "), err))
    }
}

pub fn is_repo(repo: &Path) -> bool {
    run(repo, &["rev-parse", "--git-dir"]).is_ok()
}

/// 初始化仓库（项目向导「无 git 确认初始化」的核侧入口）。
pub fn init(repo: &Path, baseline: &str) -> Result<(), GitError> {
    run(repo, &["init", "-b", baseline])?;
    // 空树首提交，让工作分支/合并有基点
    run(
        repo,
        &["commit", "--allow-empty", "-m", "chore: hexagon init"],
    )?;
    Ok(())
}

/// 确保工作分支存在并切上去。
pub fn ensure_work_branch(repo: &Path, branch: &str) -> Result<(), GitError> {
    if run(repo, &["rev-parse", "--verify", branch]).is_err() {
        run(repo, &["checkout", "-b", branch])?;
    } else {
        run(repo, &["checkout", branch])?;
    }
    Ok(())
}

/// 脏树检测：有未暂存/未提交内容即 true。
pub fn is_dirty(repo: &Path) -> bool {
    run(repo, &["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// 全量提交（产物与代码随仓入库）。无变更返回 false。
pub fn commit_all(repo: &Path, msg: &str) -> Result<bool, GitError> {
    run(repo, &["add", "-A"])?;
    if run(repo, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    run(
        repo,
        &[
            "-c",
            "user.name=hexagon-bot",
            "-c",
            "user.email=bot@hexagon.local",
            "commit",
            "-m",
            msg,
        ],
    )?;
    Ok(true)
}

pub fn head(repo: &Path) -> Result<String, GitError> {
    run(repo, &["rev-parse", "HEAD"])
}

/// 探测基线分支：main > master > 当前分支。
pub fn baseline_branch(repo: &Path) -> Result<String, GitError> {
    for cand in ["main", "master"] {
        if run(repo, &["rev-parse", "--verify", cand]).is_ok() {
            return Ok(cand.into());
        }
    }
    run(repo, &["branch", "--show-current"])
}

/// 本地落地：工作分支 --no-ff 合入基线，随后切回工作分支。
pub fn land(repo: &Path, work: &str, baseline: &str) -> Result<String, GitError> {
    run(repo, &["checkout", baseline])?;
    let r = run(
        repo,
        &[
            "merge",
            "--no-ff",
            "-m",
            "hexagon: stage baseline merge",
            work,
        ],
    );
    let _ = run(repo, &["checkout", work]); // 无论成败都回工作分支
    r
}

/// `git_baseline_merge` 工具：安全网必问（permissions::is_safety_net 已挂），
/// exec 内再查盖章闸——有阶段在 waiting_stamp 或存在未决 stamp 问题即拒执行。
pub struct GitBaselineMerge;

impl Tool for GitBaselineMerge {
    fn name(&self) -> &str {
        "git_baseline_merge"
    }
    fn description(&self) -> &str {
        "merge work branch into baseline (stamp-gated, always asks)"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"baseline":{"type":"string"}}})
    }
    fn exec(&self, db: &Db, _input: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        // 盖章闸：任何 waiting_stamp 阶段或未决 stamp 问题都挡住合入
        let waiting: i64 = db.conn().query_row(
            "SELECT COUNT(*) FROM stage_runs WHERE project_id=?1 AND state='waiting_stamp'",
            [&ctx.project_id],
            |r| r.get(0),
        )?;
        let blocked = waiting
            + crate::cards::count_queued(db, &ctx.project_id, Some(crate::cards::CardKind::Stamp))?;
        if blocked > 0 {
            return Err(ToolError::Exec(
                "baseline merge blocked: stamp gate not cleared".into(),
            ));
        }
        let work: String = db.conn().query_row(
            "SELECT work_branch FROM projects WHERE id=?1",
            [&ctx.project_id],
            |r| r.get(0),
        )?;
        let baseline =
            baseline_branch(&ctx.repo_root).map_err(|e| ToolError::Exec(e.to_string()))?;
        // 先落一次工作分支提交，保证未提交产物随合入入库
        commit_all(&ctx.repo_root, "hexagon: work in progress")
            .map_err(|e| ToolError::Exec(e.to_string()))?;
        land(&ctx.repo_root, &work, &baseline).map_err(|e| ToolError::Exec(e.to_string()))?;
        db.append_event(
            &ctx.project_id,
            crate::trace::EventKind::BaselineMerged,
            json!({"baseline": baseline, "work": work}),
            Some(&ctx.agent_id),
            ctx.stage_run_id.as_deref(),
        )?;
        Ok(json!({"merged": work, "into": baseline}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{CallOutcome, Registry};

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        init(d.path(), "main").unwrap();
        d
    }

    fn ctx(dir: &Path) -> (Db, ToolContext) {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, dir, name, mode) VALUES ('p',?1,'x','pack')",
                [dir.to_string_lossy().to_string()],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO agents (id, project_id, role, status) VALUES ('a0','p','后端','active')",
                [],
            )
            .unwrap();
        (
            db,
            ToolContext {
                project_id: "p".into(),
                agent_id: "a0".into(),
                repo_root: dir.to_path_buf(),
                stage_run_id: None,
                owned_globs: vec![],
                tiers: Default::default(),
            },
        )
    }

    #[test]
    fn work_branch_commit_land() {
        let d = repo();
        ensure_work_branch(d.path(), "hexagon/work").unwrap();
        std::fs::write(d.path().join("a.txt"), "hi").unwrap();
        assert!(is_dirty(d.path()));
        assert!(commit_all(d.path(), "work: add a").unwrap());
        assert!(!is_dirty(d.path()));
        assert!(!commit_all(d.path(), "nothing").unwrap()); // 无变更=false
        land(d.path(), "hexagon/work", "main").unwrap();
        // 合入后主支包含工作分支全部提交，且当前分支切回了工作分支
        assert!(run(
            d.path(),
            &["merge-base", "--is-ancestor", "hexagon/work", "main"]
        )
        .is_ok());
        assert_eq!(
            run(d.path(), &["branch", "--show-current"]).unwrap(),
            "hexagon/work"
        );
        let log = run(d.path(), &["log", "main", "--oneline"]).unwrap();
        assert!(log.contains("work: add a"));
    }

    #[test]
    fn merge_tool_double_gated() {
        let d = repo();
        ensure_work_branch(d.path(), "hexagon/work").unwrap();
        std::fs::write(d.path().join("f.rs"), "x").unwrap();
        commit_all(d.path(), "wip").unwrap();
        let (db, c) = ctx(d.path());
        let reg = Registry::builtin();
        // 闸 1：安全网必问
        let out = reg.call(&db, &c, "git_baseline_merge", json!({})).unwrap();
        let CallOutcome::Asked(qid) = out else {
            panic!("expected ask, got {out:?}");
        };
        // 批准 → 执行合入
        let out = reg
            .resolve(&db, &c, &qid, true, None, "activation", None, "owner")
            .unwrap();
        assert!(matches!(out, CallOutcome::Done(_)));
        assert!(run(
            d.path(),
            &["merge-base", "--is-ancestor", "hexagon/work", "main"]
        )
        .is_ok());
    }

    #[test]
    fn stamp_gate_blocks_merge() {
        let d = repo();
        ensure_work_branch(d.path(), "hexagon/work").unwrap();
        let (db, c) = ctx(d.path());
        db.conn()
            .execute(
                "INSERT INTO stage_runs (id, project_id, stage_name, seq, state)
                 VALUES ('r0','p','验收',0,'waiting_stamp')",
                [],
            )
            .unwrap();
        let reg = Registry::builtin();
        let out = reg.call(&db, &c, "git_baseline_merge", json!({})).unwrap();
        let CallOutcome::Asked(qid) = out else {
            panic!("expected ask, got {out:?}");
        };
        // 批准了也执行不了——盖章闸在 exec 内
        assert!(reg
            .resolve(&db, &c, &qid, true, None, "activation", None, "owner")
            .is_err());
    }

    #[test]
    fn rewind_does_not_touch_git() {
        let d = repo();
        ensure_work_branch(d.path(), "hexagon/work").unwrap();
        std::fs::write(d.path().join("f.rs"), "x").unwrap();
        commit_all(d.path(), "wip").unwrap();
        let before = head(d.path()).unwrap();
        let (db, _c) = ctx(d.path());
        // 两个 run：seq0 done、seq1 active，回拨到 0
        db.conn()
            .execute_batch(
                "INSERT INTO stage_runs VALUES
                 ('r0','p','s0',0,'done',NULL,NULL),
                 ('r1','p','s1',1,'active',NULL,NULL);",
            )
            .unwrap();
        let pack = crate::orchestra::PackDef {
            name: "t".into(),
            version: 1,
            knobs: Default::default(),
            stages: vec![
                crate::orchestra::StageDef {
                    name: "s0".into(),
                    roles: vec![],
                    due: vec![],
                    checks: vec![],
                    reviews: vec![],
                    stamp_point: false,
                    backfill_edges: vec![],
                    consult_wake: vec![],
                },
                crate::orchestra::StageDef {
                    name: "s1".into(),
                    roles: vec![],
                    due: vec![],
                    checks: vec![],
                    reviews: vec![],
                    stamp_point: false,
                    backfill_edges: vec![],
                    consult_wake: vec![],
                },
            ],
        };
        crate::orchestra::rewind(&db, "p", &pack, 0).unwrap();
        assert_eq!(head(d.path()).unwrap(), before); // git 状态不变
        assert!(!is_dirty(d.path()));
    }
}
