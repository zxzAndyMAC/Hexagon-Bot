//! Task-benefit-evaluation 01: host-owned task acceptance, independent of turns.
//! Only scripted debug execution is admitted until isolation and quota gates land.
pub(crate) mod budget;
pub use budget::{BudgetRunSummary, BudgetSummary, DebugPrice};
pub(crate) mod recovery;
pub use recovery::{RecoveryEntry, RecoveryReport, RecoveryState};
pub(crate) mod control;
pub use control::{EvaluationControl, EvaluationControlState};
pub(crate) mod config;
pub use config::{
    AdmissionBlock, EvaluationBatch, EvaluationLimits, FreezeRequest, ObservedOutcome, PriceSource,
    VerificationDimension, VerificationObservation,
};
pub(crate) mod human;
pub use human::{
    AttentionEnd, AttentionHandle, EvaluationActor, EvaluationDecision, HumanInterval, RunTiming,
};
pub(crate) mod isolation;
pub use isolation::{GenerationContext, IsolationReport};
pub(crate) mod outcome;
pub use outcome::{
    ExternalEffects, OutcomeObservation, RequiredSafetyFact, SafetyContract, SafetyVerdict,
};
pub(crate) mod plan;
pub use plan::{
    DebugActivation, EvaluationArm, EvaluationPlan, PlanKind, PlannedRun, PlannedState,
};
mod workspace;
use crate::{db::Db, sessions::SessionTable, tools::ToolContext};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path},
    time::{Duration, Instant},
};
pub(crate) use workspace::reconstruct;
pub use workspace::GitBaseline;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationTask {
    pub id: String,
    pub category: String,
    pub source: String,
    pub license: String,
    pub revision: String,
    pub requirements: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub allowed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<SafetyContract>,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub initial_changes: BTreeMap<String, String>,
    #[serde(default)]
    pub untracked_files: BTreeMap<String, String>,
    #[serde(default)]
    pub preserved_fragments: BTreeMap<String, Vec<String>>,
    /// Entry path in host-owned validation_files; never a mutable task command.
    pub validator: String,
    pub validation_files: BTreeMap<String, String>,
    /// Host-only assertion; never copied into the verifier or worker workspace.
    #[serde(default)]
    pub expected_stdout: Option<String>,
    pub reference: BTreeMap<String, String>,
    pub wrong: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCorpus {
    pub version: u32,
    pub cases: Vec<EvaluationCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCase {
    pub split: String,
    pub pilot: bool,
    pub task: EvaluationTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryReport {
    pub evidence_kind: String,
    pub category: String,
    pub passed: bool,
    pub cases: Vec<TaskSelfCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSelfCheck {
    pub task_id: String,
    pub initial: Acceptance,
    pub reference: Acceptance,
    pub wrong: Acceptance,
}

impl TaskSelfCheck {
    fn passed(&self) -> bool {
        !self.initial.passed && self.reference.passed && !self.wrong.passed
    }
}

fn category_valid(corpus: &EvaluationCorpus) -> bool {
    // Evaluation 02 / D02: invalid partitions cost a corrected manifest;
    // accepting one would let selected samples masquerade as full coverage.
    let Some(first) = corpus.cases.first() else {
        return false;
    };
    corpus.cases.iter().all(|c| {
        c.task
            .expected_stdout
            .as_ref()
            .is_some_and(|s| !s.is_empty())
    }) && corpus.version == 1
        && matches!(
            first.task.category.as_str(),
            "bug" | "feature" | "interface" | "dirty_tree"
        )
        && corpus.cases.len() == 5
        && corpus
            .cases
            .iter()
            .map(|c| &c.task.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == 5
        && corpus
            .cases
            .iter()
            .all(|c| c.task.category == first.task.category)
        && corpus
            .cases
            .iter()
            .filter(|c| c.split == "development")
            .count()
            == 3
        && corpus.cases.iter().filter(|c| c.split == "heldout").count() == 2
        && corpus.cases.iter().filter(|c| c.pilot).count() == 1
        && corpus
            .cases
            .iter()
            .all(|c| !c.pilot || c.split == "development")
}

pub(crate) fn check_category(corpus: &EvaluationCorpus) -> io::Result<CategoryReport> {
    let started = Instant::now();
    if !category_valid(corpus) {
        crate::diag::note(
            crate::diag::CLASS_REJECT,
            true,
            Some(crate::PROJECT_ID),
            None,
            None,
            None,
            "evaluation_category",
            "invalid_partition",
            started,
        );
        return Err(err("category requires five unique tasks with expected results, three development, two heldout and one development pilot"));
    }
    let cases = corpus
        .cases
        .iter()
        .map(|c| self_check_evidence(&c.task))
        .collect::<io::Result<Vec<_>>>()?;
    let report = CategoryReport {
        evidence_kind: "fixture_self_check".into(),
        category: corpus.cases[0].task.category.clone(),
        passed: cases.iter().all(TaskSelfCheck::passed),
        cases,
    };
    crate::diag::note(
        if report.passed {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !report.passed,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_category",
        if report.passed {
            "self_checked"
        } else {
            "self_check_failed"
        },
        started,
    );
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub command: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    #[serde(default)]
    pub git_preserved: Option<bool>,
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
    #[serde(default)]
    pub git_baseline: Option<GitBaseline>,
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
    let initial_files = workspace::effective_files(task);
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
            safe_path(p) && !task.allowed_paths.contains(p) && !initial_files.contains_key(p)
        })
        && task.safety.as_ref().is_none_or(outcome::valid_contract)
        && !task.files.is_empty()
        && !task.allowed_paths.is_empty()
        && task
            .files
            .keys()
            .chain(task.initial_changes.keys())
            .chain(task.untracked_files.keys())
            .chain(task.preserved_fragments.keys())
            .chain(task.allowed_paths.iter())
            .chain(task.reference.keys())
            .chain(task.wrong.keys())
            .all(|p| safe_path(p))
        && task
            .initial_changes
            .keys()
            .all(|p| task.files.contains_key(p))
        && task
            .untracked_files
            .keys()
            .all(|p| !task.files.contains_key(p))
        && task.preserved_fragments.iter().all(|(p, fragments)| {
            !fragments.is_empty()
                && initial_files
                    .get(p)
                    .is_some_and(|body| fragments.iter().all(|f| !f.is_empty() && body.contains(f)))
        })
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
        std::fs::write(&target, body)?;
        // D02/D06: fixture files have a deterministic 0644 baseline. Comparing
        // only executable bits missed unauthorized 0444/0666 mode changes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))?;
        }
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

pub(crate) fn now_ms() -> io::Result<u64> {
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

pub(crate) fn accept(
    db: &Db,
    root: &Path,
    task: &EvaluationTask,
    baseline: Option<&GitBaseline>,
) -> io::Result<Acceptance> {
    let started = Instant::now();
    let started_at_ms = now_ms()?;
    let before = fingerprint(root)?;
    let git_before = baseline.map(|b| workspace::git_preserved(root, b));
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
    let preserved = workspace::effective_files(task)
        .iter()
        .filter(|(p, _)| !task.allowed_paths.contains(p))
        .all(|(p, body)| std::fs::read_to_string(root.join(p)).ok().as_ref() == Some(body));
    let fragments_preserved = task.preserved_fragments.iter().all(|(path, parts)| {
        std::fs::read_to_string(root.join(path))
            .is_ok_and(|body| parts.iter().all(|p| body.contains(p)))
    });
    let git_preserved =
        baseline.map(|b| git_before == Some(true) && workspace::git_preserved(root, b));
    let required_git = !task.initial_changes.is_empty() || !task.untracked_files.is_empty();
    let passed = acceptance_passed(
        exit_code,
        timed_out,
        before == after && verifier_before == verifier_after,
        preserved
            && fragments_preserved
            && outcome::scope_violations(root, task)?.is_empty()
            && (!required_git || git_preserved == Some(true)),
    ) && task.expected_stdout.as_ref().is_none_or(|expected| {
        // Evaluation 02 review: os._exit(0) used to skip Python assertions.
        // Only the host compares the complete probe result; missing output fails.
        output["stdout"].as_str() == Some(expected.as_str())
    });
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
        git_preserved,
        before,
        after,
        passed,
        started_at_ms,
        finished_at_ms: now_ms()?,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

fn self_check_evidence(task: &EvaluationTask) -> io::Result<TaskSelfCheck> {
    validate(task)?;
    let run = |patch: Option<&BTreeMap<String, String>>| -> io::Result<Acceptance> {
        let copy = tempfile::tempdir()?;
        let baseline = reconstruct(copy.path(), task)?;
        if let Some(patch) = patch {
            materialize(copy.path(), patch)?;
        }
        let db = Db::open_in_memory().map_err(err)?;
        accept(&db, copy.path(), task, baseline.as_ref())
    };
    Ok(TaskSelfCheck {
        task_id: task.id.clone(),
        initial: run(None)?,
        reference: run(Some(&task.reference))?,
        wrong: run(Some(&task.wrong))?,
    })
}

pub(crate) fn self_check(task: &EvaluationTask) -> io::Result<()> {
    let started = Instant::now();
    if !self_check_evidence(task)?.passed() {
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

pub(crate) fn update_started(db: &Db, result: &EvaluationResult) -> io::Result<()> {
    let terminal = matches!(result.state.as_str(), "completed" | "failed" | "incomplete");
    // D06: late tools/cards cannot repair a missed upgrade in an already-ended
    // attempt. Retain its execution boundary; absent legacy seals stay unknown.
    let seal = if terminal {
        outcome::execution_fingerprint(Path::new(&result.workspace)).ok()
    } else {
        None
    };
    let tx = if db.conn().is_autocommit() {
        Some(
            rusqlite::Transaction::new_unchecked(
                db.conn(),
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(err)?,
        )
    } else {
        None
    };
    let changed = db.conn().execute("UPDATE evaluation_runs SET result_json=?2,execution_fingerprint=CASE WHEN ?3 THEN ?4 ELSE execution_fingerprint END WHERE id=?1 AND json_extract(result_json,'$.state') IN ('started','waiting_human')", rusqlite::params![result.id,serde_json::to_string(result)?,terminal,seal]).map_err(err)?;
    if changed != 1 {
        return Err(err("evaluation result already terminal or missing"));
    }
    #[cfg(test)]
    if terminal {
        recovery::crash_at(recovery::CrashPoint::ResultPersisted);
    }
    if terminal {
        settle_terminal(db, result)?;
    } else {
        control::sync(db, result)?;
    }
    if let Some(tx) = tx {
        tx.commit().map_err(err)?;
    }
    Ok(())
}

// D10 / ticket 13: result, fee closure, control and plan once had separate
// crash windows. Call inside a host transaction; never rewrite terminal evidence.
pub(crate) fn settle_terminal(db: &Db, result: &EvaluationResult) -> io::Result<()> {
    budget::finish(db, result)?;
    control::sync(db, result)?;
    use rusqlite::OptionalExtension;
    let plan: Option<(String, i64)> = db
        .conn()
        .query_row(
            "SELECT plan_id,position FROM evaluation_plan_runs WHERE run_id=?1 AND state='started'",
            [&result.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    if let Some((id, pos)) = plan {
        plan::finish(db, &id, usize::try_from(pos).map_err(err)?, &result.id)?;
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
    use super::{acceptance_passed, category_valid, EvaluationCorpus};
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn invalid_category_partition_never_admits(index in 0usize..5, heldout_pilot in any::<bool>()) {
            let mut corpus: EvaluationCorpus = serde_json::from_str(include_str!("../../../evaluation/corpus/bugs.json")).unwrap();
            prop_assert!(category_valid(&corpus));
            let mut missing = corpus.clone();
            missing.cases[index].task.expected_stdout = None;
            prop_assert!(!category_valid(&missing));
            if heldout_pilot {
                corpus.cases[0].pilot = false;
                corpus.cases[3].pilot = true;
            } else {
                corpus.cases[index].split = "unknown".into();
            }
            prop_assert!(!category_valid(&corpus));
        }

        #[test]
        fn removing_acceptance_evidence_never_passes(exit in any::<i64>(), timeout in any::<bool>(), stable in any::<bool>(), preserved in any::<bool>()) {
            prop_assert!(!acceptance_passed(exit, true, stable, preserved));
            prop_assert!(!acceptance_passed(exit, timeout, false, preserved));
            prop_assert!(!acceptance_passed(exit, timeout, stable, false));
            if exit != 0 { prop_assert!(!acceptance_passed(exit, timeout, stable, preserved)); }
        }
    }
}
