//! Owner autonomy ticket 04 (2026-10-01): host-derived quality obligations.
//! Check selection uses actual source changes, never an agent's "all passed".
//! Unknown scope/runner remains missing evidence; an owner may use the existing
//! version-bound exception card, which never rewrites the execution verdict.
use super::*;
use rusqlite::OptionalExtension;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const BASELINE_KIND: &str = "quality_source_baseline";
type Files = BTreeMap<String, (String, u32)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct PerformanceBaseline {
    pub signature: String,
    pub median_ms: f64,
    pub event_id: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct SourceBaseline {
    files: Option<Files>,
    #[serde(default)]
    performance: BTreeMap<String, PerformanceBaseline>,
}

fn baseline(db: &Db, project: &str) -> Result<Option<(i64, SourceBaseline)>, OrchError> {
    let row: Option<(i64, String)> = db.conn().query_row(
        "SELECT event_id,snapshot_json FROM quality_baselines WHERE project_id=?1 ORDER BY event_id DESC LIMIT 1",
        [project], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    Ok(row.map(|(id, body)| (id, serde_json::from_str(&body).unwrap_or_default())))
}

pub(super) fn capture_baseline(db: &Db, project: &str, accepted: bool) -> Result<(), OrchError> {
    let previous = baseline(db, project)?;
    if !accepted && previous.is_some() {
        return Ok(());
    }
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let mut state = SourceBaseline {
        files: crate::artifacts::fingerprint::source_manifest(Path::new(&root)).ok(),
        performance: previous
            .as_ref()
            .map(|(_, b)| b.performance.clone())
            .unwrap_or_default(),
    };
    // A completed owner acceptance makes this measured version the reference.
    // A tolerated regression remains a failed TestRan fact; this only advances
    // the comparison baseline, preserving the owner exception in the trace.
    if accepted {
        let after = previous.as_ref().map(|(id, _)| *id).unwrap_or(0);
        let mut q = db.conn().prepare("SELECT id,payload FROM events WHERE project_id=?1 AND id>?2 AND kind='test_ran' AND json_extract(payload,'$.quality.category')='performance' ORDER BY id")?;
        for row in q.query_map(rusqlite::params![project, after], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, body) = row?;
            let p: Value = serde_json::from_str(&body)?;
            // Q2 owner decision 2026-10-05: delivery acceptance is not consent
            // to a different measurement. Only explicit confirmation below
            // may adopt changed conditions; keep the old failed fact intact.
            if p["stable"] != true
                || p["quality"]["execution_exit_code"] != 0
                || p["quality"]["verdict"] == "measurement_changed"
            {
                continue;
            }
            if let (Some(command), Some(signature), Some(median)) = (
                p["quality"]["command"].as_str(),
                p["quality"]["signature"].as_str(),
                p["quality"]["median_ms"].as_f64(),
            ) {
                if median.is_finite()
                    && median > 0.0
                    && state
                        .performance
                        .get(command)
                        .is_none_or(|b| b.signature == signature)
                {
                    state.performance.insert(
                        command.into(),
                        PerformanceBaseline {
                            signature: signature.into(),
                            median_ms: median,
                            event_id: id,
                        },
                    );
                }
            }
        }
    }
    let snapshot = serde_json::to_string(&state)?;
    // Source manifests stay in protected local storage. Sending thousands of
    // file hashes through every timeline page previously defeated UI budgets.
    let id = db.append_event(
        project,
        EventKind::System,
        json!({
            "kind":BASELINE_KIND, "available":state.files.is_some(),
            "file_count":state.files.as_ref().map(BTreeMap::len),
            "reason":if accepted { "accepted_version" } else { "before_work" }
        }),
        None,
        None,
    )?;
    db.conn().execute(
        "INSERT INTO quality_baselines(project_id,event_id,snapshot_json) VALUES (?1,?2,?3)",
        rusqlite::params![project, id, snapshot],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct QualityEvidence {
    pub category: String,
    pub command: Option<String>,
    pub changed_paths: Vec<String>,
    pub reason: String,
    #[ts(type = "number | null")]
    pub baseline_event_id: Option<i64>,
    pub baseline_ms: Option<f64>,
    pub measured_ms: Option<f64>,
}

#[derive(Clone, Debug)]
pub(super) struct Requirement {
    pub key: String,
    pub category: &'static str,
    /// Only a command already declared by the owner-approved process pack.
    pub command: Option<String>,
    pub paths: Vec<String>,
}

// Ticket04 scope judgment: selected checks are conservative, but unrelated UI
// checks must not block backend-only changes (owner decision 2026-10-01).
// False negatives can miss validation; false positives cost a check/owner
// exception. Unknown baseline fails closed separately instead of guessing scope.
fn categories(path: &str) -> BTreeSet<&'static str> {
    let path = path.to_lowercase();
    let file = Path::new(&path);
    let ext = file.extension().and_then(|v| v.to_str()).unwrap_or("");
    let name = file.file_name().and_then(|v| v.to_str()).unwrap_or("");
    let dependency = matches!(
        name,
        "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "cargo.toml"
            | "cargo.lock"
            | "requirements.txt"
            | "pyproject.toml"
            | "poetry.lock"
            | "go.mod"
            | "go.sum"
            | "gemfile"
            | "gemfile.lock"
    );
    let code = matches!(
        ext,
        "sh" | "bash"
            | "zsh"
            | "sql"
            | "rs"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "cs"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "rb"
            | "php"
            | "js"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "jsx"
            | "vue"
            | "svelte"
            | "html"
            | "css"
            | "scss"
            | "sass"
    );
    let mut out = BTreeSet::new();
    if !code && !dependency {
        return out;
    }
    out.insert("tests");
    let test_file = path.contains("/tests/")
        || path.starts_with("tests/")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.starts_with("test_");
    if test_file {
        return out;
    }
    let ui = matches!(
        ext,
        "tsx" | "jsx" | "vue" | "svelte" | "html" | "css" | "scss" | "sass"
    ) || (code
        && path
            .split('/')
            .any(|p| matches!(p, "ui" | "frontend" | "components" | "pages")));
    if ui {
        out.insert("accessibility");
        out.insert("performance");
    }
    if dependency
        || [
            "auth",
            "permission",
            "credential",
            "security",
            "session",
            "router",
            "api/",
        ]
        .iter()
        .any(|p| path.contains(p))
    {
        out.insert("security");
    }
    if code
        && ["cache", "query", "render", "benchmark", "worker"]
            .iter()
            .any(|p| path.contains(p))
    {
        out.insert("performance");
    }
    out
}

pub(super) fn requirements(
    db: &Db,
    project: &str,
    run: &str,
    stage: &StageDef,
    pack: &PackDef,
) -> Result<Vec<Requirement>, OrchError> {
    let Some(active) = acceptance_run(db, project, pack)? else {
        return Ok(Vec::new());
    };
    let seq: i64 = db.conn().query_row(
        "SELECT seq FROM stage_runs WHERE project_id=?1 AND id=?2",
        rusqlite::params![project, run],
        |r| r.get(0),
    )?;
    let seq = usize::try_from(seq)
        .ok()
        .filter(|seq| *seq < pack.stages.len())
        .ok_or(OrchError::BadSeq(seq))?;
    let configuration_categories = quality_config::unaccepted_categories(db, project, pack, seq)?;
    // Q4 review / 2026-10-05: a later local runner hid changed historical
    // quality commands when that stage had no ordinary check. Keep each affected
    // run's alias (including performance's three samples), while source-change
    // selection remains on the current acceptance run. Missing proof costs one
    // recheck; dropping an obligation could accept an untested configuration.
    let current_acceptance = active.id == run;
    if !current_acceptance && configuration_categories.is_empty() {
        return Ok(Vec::new());
    }
    // Intermediate implementation can proceed to its testing stage. Delivery,
    // explicit check stages and the last stage enforce current obligations.
    if !stage.stamp_point
        && stage.checks.is_empty()
        && stage.quality_checks.is_empty()
        && configuration_categories.is_empty()
        && seq + 1 < pack.stages.len()
    {
        return Ok(Vec::new());
    }
    let root: String =
        db.conn()
            .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                r.get(0)
            })?;
    let current = crate::artifacts::fingerprint::source_manifest(Path::new(&root)).ok();
    let old = baseline(db, project)?.and_then(|(_, b)| b.files);
    let (Some(current), Some(old)) = (current, old) else {
        return Ok(vec![Requirement {
            key: "quality:change_scope".into(),
            category: "change_scope",
            command: None,
            paths: vec![],
        }]);
    };
    let paths: BTreeSet<_> = old
        .keys()
        .chain(current.keys())
        .filter(|p| old.get(*p) != current.get(*p))
        .cloned()
        .collect();
    let mut grouped: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for category in configuration_categories {
        grouped.entry(category.key()).or_default();
    }
    if current_acceptance {
        for path in paths {
            for category in categories(&path) {
                grouped.entry(category).or_default().push(path.clone());
            }
        }
    }
    let declared: Vec<(&super::QualityCategory, &String)> = stage
        .quality_checks
        .iter()
        .chain(pack.stages.iter().flat_map(|s| &s.quality_checks))
        .collect();
    Ok(grouped
        .into_iter()
        .map(|(category, paths)| Requirement {
            key: format!("quality:{category}"),
            category,
            command: declared
                .iter()
                .find(|(kind, command)| kind.key() == category && !command.trim().is_empty())
                .map(|(_, command)| (*command).clone()),
            paths,
        })
        .collect())
}

pub(super) fn performance_baseline(
    db: &Db,
    project: &str,
    command: &str,
) -> Result<Option<PerformanceBaseline>, OrchError> {
    let previous = baseline(db, project)?;
    if let Some(value) = previous.as_ref().and_then(|(_, b)| {
        b.performance
            .get(command)
            .or_else(|| b.performance.values().max_by_key(|v| v.event_id))
    }) {
        return Ok(Some(value.clone()));
    }
    let after = previous.map(|(id, _)| id).unwrap_or(0);
    let row: Option<(i64,String)> = db.conn().query_row(
        "SELECT id,payload FROM events WHERE project_id=?1 AND id>?2 AND kind='test_ran' AND json_extract(payload,'$.quality.category')='performance' AND json_extract(payload,'$.quality.verdict')='baseline_established' AND json_extract(payload,'$.stable')=1 ORDER BY (json_extract(payload,'$.quality.command')=?3) DESC,id LIMIT 1",
        rusqlite::params![project,after,command], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    Ok(row.and_then(|(id, body)| {
        let p: Value = serde_json::from_str(&body).ok()?;
        Some(PerformanceBaseline {
            signature: p["quality"]["signature"].as_str()?.into(),
            median_ms: p["quality"]["median_ms"].as_f64()?,
            event_id: id,
        })
    }))
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PerformanceBaselineConfirmation {
    #[ts(type = "number")]
    pub event_id: i64,
    #[ts(type = "number")]
    pub measurement_event_id: i64,
}

fn confirmation_measurement(payload: &Value) -> Option<(String, String, f64)> {
    let q = &payload["quality"];
    if payload["stable"] != true
        || q["category"] != "performance"
        || q["execution_exit_code"] != 0
        || q["verdict"] != "measurement_changed"
    {
        return None;
    }
    let mut samples = q["samples_ms"]
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()?;
    if samples.len() != 3 || samples.iter().any(|n| !n.is_finite() || *n <= 0.0) {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    let measured = q["median_ms"].as_f64()?;
    if measured != samples[1] {
        return None;
    }
    Some((
        q["command"].as_str()?.into(),
        q["signature"].as_str()?.into(),
        measured,
    ))
}

/// Q2: owner-only control, bound to a current real measurement and version.
/// False rejection costs another measurement; false acceptance can conceal a
/// regression. Confirmation updates only the baseline, never execution facts.
pub fn confirm_performance_baseline(
    db: &Db,
    project: &str,
    pack: &PackDef,
    measurement: i64,
    expected: &str,
    reason: &str,
) -> Result<PerformanceBaselineConfirmation, OrchError> {
    let started = std::time::Instant::now();
    let effective = quality_config::effective_pack(db, project, pack)?;
    let pack = &effective;
    let result = (|| {
        let _lease = write_boundary(db, project)?;
        let tx = db.conn().unchecked_transaction()?;
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(OrchError::InvalidAcceptanceException);
        }
        let evidence =
            stage_evidence(db, project, pack)?.ok_or(OrchError::InvalidAcceptanceException)?;
        if expected.is_empty() || evidence.fingerprint.as_deref() != Some(expected) {
            return Err(OrchError::StaleAcceptanceVersion);
        }
        let check = evidence
            .checks
            .iter()
            .find(|c| {
                c.event_id == Some(measurement)
                    && c.state == CheckState::Failed
                    && c.quality.as_ref().is_some_and(|q| {
                        q.category == "performance" && q.reason == "measurement_changed"
                    })
            })
            .ok_or(OrchError::InvalidAcceptanceException)?;
        let body: String = db.conn().query_row("SELECT payload FROM events WHERE project_id=?1 AND id=?2 AND kind='test_ran' AND stage_run_id=?3",
            rusqlite::params![project,measurement,check.run_id], |r| r.get(0))?;
        let p: Value = serde_json::from_str(&body)?;
        let (command, signature, median_ms) =
            confirmation_measurement(&p).ok_or(OrchError::InvalidAcceptanceException)?;
        let root: String =
            db.conn()
                .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
                    r.get(0)
                })?;
        if check.quality.as_ref().and_then(|q| q.command.as_deref()) != Some(command.as_str())
            || measurement_signature(Path::new(&root), &command)? != signature
        {
            return Err(OrchError::StaleAcceptanceVersion);
        }
        let previous = baseline(db, project)?.ok_or(OrchError::InvalidAcceptanceException)?;
        let mut state = previous.1;
        let old = performance_baseline(db, project, &command)?;
        state.performance.insert(
            command.clone(),
            PerformanceBaseline {
                signature: signature.clone(),
                median_ms,
                event_id: measurement,
            },
        );
        // Never advance source acceptance while changing a comparison reference.
        if stage_evidence(db, project, pack)?
            .and_then(|e| e.fingerprint)
            .as_deref()
            != Some(expected)
        {
            return Err(OrchError::StaleAcceptanceVersion);
        }
        let prior: Option<(i64,String)> = db.conn().query_row("SELECT id,payload FROM events WHERE project_id=?1 AND kind='system' AND json_extract(payload,'$.kind')='performance_baseline_confirmed' AND json_extract(payload,'$.measurement_event_id')=?2 ORDER BY id DESC LIMIT 1",
            rusqlite::params![project,measurement], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((event_id, text)) = prior {
            let p: Value = serde_json::from_str(&text)?;
            if p["reason"] != reason || p["fingerprint"] != expected {
                return Err(OrchError::InvalidAcceptanceException);
            }
            return Ok(PerformanceBaselineConfirmation {
                event_id,
                measurement_event_id: measurement,
            });
        }
        let event_id = db.append_event(project, EventKind::System,
            json!({"kind":"performance_baseline_confirmed","by":"owner","reason":reason,"measurement_event_id":measurement,"fingerprint":expected,"command":command,"signature":signature,"median_ms":median_ms,"previous":old}),None,Some(&check.run_id))?;
        db.conn().execute(
            "INSERT INTO quality_baselines(project_id,event_id,snapshot_json) VALUES (?1,?2,?3)",
            rusqlite::params![project, event_id, serde_json::to_string(&state)?],
        )?;
        tx.commit()?;
        Ok(PerformanceBaselineConfirmation {
            event_id,
            measurement_event_id: measurement,
        })
    })();
    let code = match &result {
        Ok(_) => "confirmed",
        Err(OrchError::StaleAcceptanceVersion) => "stale_version",
        Err(OrchError::InvalidAcceptanceException) if reason.trim().is_empty() => "missing_reason",
        Err(OrchError::InvalidAcceptanceException) => "invalid_measurement",
        Err(_) => "boundary_refused",
    };
    let run = db.active_stage_run(project).ok().flatten();
    crate::diag::note(
        if result.is_err() {
            crate::diag::CLASS_REJECT
        } else {
            crate::diag::CLASS_JUDGE
        },
        result.is_err(),
        Some(project),
        Some("owner"),
        run.as_ref().map(|r| r.id.as_str()),
        Some(&measurement.to_string()),
        "performance_baseline_confirm",
        code,
        started,
    );
    result
}

#[cfg(test)]
mod confirmation_tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn eligibility_requires_stable_success_and_the_actual_three_sample_median(stable in any::<bool>(), success in any::<bool>(), changed in any::<bool>(), matching_median in any::<bool>(), samples in prop::array::uniform3(0.01f64..10000.0)) {
            let mut sorted = samples;
            sorted.sort_by(f64::total_cmp);
            let median = if matching_median { sorted[1] } else { sorted[1] + 10001.0 };
            let p = json!({"stable":stable,"quality":{"category":"performance","execution_exit_code":if success {0} else {7},"verdict":if changed {"measurement_changed"} else {"regressed"},"samples_ms":samples,"median_ms":median,"command":"runner","signature":"v1"}});
            prop_assert_eq!(confirmation_measurement(&p).is_some(), stable && success && changed && matching_median);
        }
        #[test]
        fn failed_or_unstable_measurements_never_qualify(exit in 1i32..255, unstable in any::<bool>(), ms in 0.001f64..10000.0) {
            let p = json!({"stable":!unstable,"quality":{"category":"performance","execution_exit_code":exit,"verdict":"measurement_changed","samples_ms":[ms,ms,ms],"median_ms":ms,"command":"runner","signature":"v1"}});
            prop_assert!(confirmation_measurement(&p).is_none());
            let mut p = p;
            p["quality"]["execution_exit_code"] = json!(0);
            p["stable"] = json!(false);
            prop_assert!(confirmation_measurement(&p).is_none());
        }
    }
}

/// Bind the measurement definition as well as its command. Workload files
/// explicitly named by a runner and npm script definitions cannot silently
/// change the meaning of an old baseline. Source under test remains free to change.
pub(super) fn measurement_signature(root: &Path, command: &str) -> Result<String, OrchError> {
    use sha2::{Digest, Sha256};
    let mut inputs = BTreeMap::<String, String>::new();
    inputs.insert("command".into(), command.into());
    inputs.insert("measurement".into(), "owned_process_leader_ms_v1".into());
    inputs.insert(
        "platform".into(),
        format!("{}:{}", std::env::consts::OS, std::env::consts::ARCH),
    );
    let words: Vec<&str> = command.split_whitespace().collect();
    let package_path = root.join("package.json");
    crate::db::validate_generic_file_access(&package_path)?;
    let package: Value = std::fs::read(package_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let script = words
        .windows(2)
        .find(|w| w[0] == "run")
        .and_then(|w| package["scripts"][w[1]].as_str());
    if let Some(script) = script {
        inputs.insert("package_script".into(), script.into());
    }
    for word in words
        .into_iter()
        .chain(script.into_iter().flat_map(str::split_whitespace))
    {
        let path = word.trim_matches(['\'', '"']);
        if !path.starts_with('-')
            && !Path::new(path).is_absolute()
            && !path.split('/').any(|p| p == "..")
            && root.join(path).is_file()
        {
            if let Some(digest) =
                crate::tools::writeguard::digest(&root.join(path)).map_err(std::io::Error::other)?
            {
                inputs.insert(path.into(), digest);
            }
        }
    }
    Ok(format!(
        "v1:{:x}",
        Sha256::digest(serde_json::to_vec(&inputs)?)
    ))
}

pub(super) fn describe(
    requirement: &Requirement,
    state: &CheckState,
    payload: &Value,
) -> QualityEvidence {
    let reason = match state {
        CheckState::Missing if requirement.category == "change_scope" => "scope_unavailable",
        CheckState::Missing if requirement.command.is_none() => "missing_runner",
        CheckState::Missing => "not_run",
        CheckState::Stale => "source_changed",
        CheckState::Unavailable => "scope_unavailable",
        CheckState::Failed => payload["quality"]["verdict"]
            .as_str()
            .unwrap_or("execution_failed"),
        CheckState::Passed => payload["quality"]["verdict"].as_str().unwrap_or("passed"),
    };
    QualityEvidence {
        category: requirement.category.into(),
        command: requirement.command.clone(),
        changed_paths: requirement.paths.iter().take(32).cloned().collect(),
        reason: reason.into(),
        baseline_event_id: payload["quality"]["baseline_event_id"].as_i64(),
        baseline_ms: payload["quality"]["baseline_ms"].as_f64(),
        measured_ms: payload["quality"]["median_ms"].as_f64(),
    }
}

/// Ticket04 owner decision: missing/changed measurement never becomes a pass.
/// False negative costs an owner review or retest; false positive accepts a
/// regression. Three-sample medians tolerate 15% plus a 10ms timing noise floor.
pub(super) fn performance_verdict(
    code: i32,
    signature: &str,
    measured: f64,
    baseline: Option<&PerformanceBaseline>,
) -> (&'static str, i32) {
    if code != 0 {
        return ("execution_failed", code);
    }
    if !measured.is_finite() || measured <= 0.0 {
        return ("measurement_changed", -3);
    }
    let Some(baseline) = baseline else {
        return ("baseline_established", 0);
    };
    if signature != baseline.signature
        || !measured.is_finite()
        || !baseline.median_ms.is_finite()
        || baseline.median_ms <= 0.0
    {
        return ("measurement_changed", -3);
    }
    if measured > baseline.median_ms * 1.15 && measured - baseline.median_ms > 10.0 {
        ("regressed", -2)
    } else {
        ("passed", 0)
    }
}

#[cfg(all(test, unix))]
#[test]
fn qa14_measurement_definition_refuses_database_aliases() {
    for hard in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join(".hexagon/state.db-shm");
        std::fs::create_dir(state.parent().unwrap()).unwrap();
        std::fs::write(&state, b"fixture not a package definition").unwrap();
        let package = root.path().join("package.json");
        if hard {
            std::fs::hard_link(&state, &package).unwrap();
        } else {
            std::os::unix::fs::symlink(&state, &package).unwrap();
        }
        // QA14: treating unreadable measurement definitions as absent used to
        // read/close SHM first and then return a plausible command-only hash.
        assert!(measurement_signature(root.path(), "npm run performance").is_err());
    }
}
