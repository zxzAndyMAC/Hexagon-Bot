//! D13/D14: a candidate needs its own original heldout attempts and host source.
use super::{config, err, report, BenefitRun, EvaluationArm, PlannedState, SafetyVerdict};
use crate::{db::Db, tools::ToolContext};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateQuality {
    Unverified,
    Incomplete,
    Failed,
    Stale,
    Qualified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSource {
    pub version: u32,
    pub proposal_id: String,
    pub generation_id: String,
    pub batch_id: String,
    pub plan_id: String,
    pub host: String,
    pub proposal_binding: String,
    pub batch_fingerprint: String,
    pub generation_material: String,
    pub source_kind: String,
    #[serde(default)]
    pub generation_operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvaluation {
    pub source: CandidateSource,
    pub state: CandidateQuality,
    pub adoptable: bool,
    pub original_planned: usize,
    pub original_passed: usize,
    pub rows: Vec<BenefitRun>,
    pub reasons: Vec<String>,
}

pub(crate) fn source(db: &Db, proposal: &str) -> io::Result<Option<CandidateSource>> {
    let row: Option<(String, String, String, String, String)> = db.conn().query_row(
        "SELECT source_json,fingerprint,generation_id,batch_id,plan_id FROM evaluation_candidates WHERE proposal_id=?1",
        [proposal], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional().map_err(err)?;
    let Some((json, fingerprint, generation, batch, plan)) = row else {
        return Ok(None);
    };
    let source: CandidateSource = serde_json::from_str(&json)?;
    if source.version != 1
        || source.proposal_id != proposal
        || source.generation_id != generation
        || source.batch_id != batch
        || source.plan_id != plan
        || config::digest(&source)? != fingerprint
    {
        return Err(err("candidate_source_corrupt"));
    }
    Ok(Some(source))
}

pub(crate) fn freeze(
    db: &Db,
    ctx: &ToolContext,
    proposal: &str,
    generation: &str,
) -> io::Result<CandidateSource> {
    let started = std::time::Instant::now();
    let _lease = crate::tools::writeguard::repository_lock(ctx).map_err(err)?;
    let (binding, baseline, candidate) =
        crate::proposals::evaluation_binding(db, ctx, proposal).map_err(err)?;
    if let Some(existing) = source(db, proposal)? {
        if existing.generation_id == generation && existing.proposal_binding == binding {
            return Ok(existing);
        }
        return Err(freeze_refusal(ctx, "candidate_already_frozen", started));
    }
    let context = super::isolation::read(db, generation)?;
    let original = config::check(
        db,
        &ctx.project_id,
        &context.batch_id,
        &config::read(db, &context.batch_id)?.request,
    )?;
    if !context.eligible_for_generation
        || original.blocks.iter().any(|b| {
            matches!(
                b,
                super::AdmissionBlock::HeldoutRetired
                    | super::AdmissionBlock::ConfigurationDrift
                    | super::AdmissionBlock::RuntimeDrift
            )
        })
    {
        return Err(freeze_refusal(
            ctx,
            "candidate_generation_stale_or_disclosed",
            started,
        ));
    }
    if config::digest(&baseline)? != config::digest(&original.request.full_pack)? {
        return Err(freeze_refusal(ctx, "candidate_baseline_mismatch", started));
    }
    let mut request = original.request.clone();
    request.full_pack = candidate;
    let batch = config::freeze(db, &ctx.project_id, &request, Some(&original.id))?;
    let plan = super::plan::create(db, &batch.id, super::PlanKind::Candidate)?;
    let source = CandidateSource {
        version: 1,
        proposal_id: proposal.into(),
        generation_id: generation.into(),
        batch_id: batch.id,
        plan_id: plan.id,
        host: ctx.repo_root.canonicalize()?.to_string_lossy().into_owned(),
        proposal_binding: binding,
        batch_fingerprint: batch.fingerprint,
        generation_material: context.material_fingerprint,
        // D14: attaching an existing proposal to a clean context does not prove
        // it was generated there. Such records permit regression only.
        source_kind: "existing_proposal_unverified".into(),
        generation_operation: None,
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
    if !super::isolation::read(db, generation)?.eligible_for_generation
        || config::read(db, &original.id)?
            .blocks
            .contains(&super::AdmissionBlock::HeldoutRetired)
    {
        return Err(freeze_refusal(
            ctx,
            "candidate_disclosed_during_freeze",
            started,
        ));
    }
    if crate::proposals::evaluation_binding(db, ctx, proposal)
        .map_err(err)?
        .0
        != source.proposal_binding
    {
        return Err(freeze_refusal(ctx, "candidate_binding_changed", started));
    }
    db.conn().execute("INSERT INTO evaluation_candidates(proposal_id,generation_id,batch_id,plan_id,source_json,fingerprint) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![proposal,generation,source.batch_id,source.plan_id,serde_json::to_string(&source)?,config::digest(&source)?]).map_err(err)?;
    if let Some(tx) = tx {
        tx.commit().map_err(err)?;
    }
    Ok(source)
}

fn row_passes(row: &BenefitRun) -> bool {
    row.state == PlannedState::Completed
        && row.flow_completed == Some(true)
        && row.independent_passed == Some(true)
        && row.owner_exception == Some(false)
        && (row.evidence_kind.as_deref() == Some("live_model")
            || (cfg!(test) && row.evidence_kind.as_deref() == Some("provider_boundary_fixture")))
        && row.safety == Some(SafetyVerdict::Passed)
        && row.known_mc.is_some()
        && row.unknown_mc == Some(0)
        && row.in_flight_mc == Some(0)
        && row.requests.is_some_and(|n| n > 0)
        && row.reasons.iter().all(|reason| {
            cfg!(test)
                && row.evidence_kind.as_deref() == Some("provider_boundary_fixture")
                && reason == "scripted_or_unverified"
        })
        && row.outcome.as_ref().is_some_and(|o| {
            o.acceptance_is_current
                && o.independent_passed
                && o.flow_completed
                && !o.owner_exception
                && o.unknowns.is_empty()
                && o.violations.is_empty()
        })
}

fn qualifies(
    rows: &[BenefitRun],
    expected: &BTreeSet<(String, u32)>,
    plan: &str,
    binding_current: bool,
    trusted_source: bool,
) -> bool {
    // D13: false negatives cost another independent evaluation; a false positive
    // admits an unreviewed side effect. Missing/duplicate/unknown evidence fails closed.
    let keys: BTreeSet<_> = rows
        .iter()
        .map(|r| (r.task_id.clone(), r.repetition))
        .collect();
    let runs: BTreeSet<_> = rows.iter().filter_map(|r| r.run_id.as_ref()).collect();
    binding_current
        && trusted_source
        && expected.len() == 24
        && keys == *expected
        && rows.len() == expected.len()
        && runs.len() == expected.len()
        && rows.iter().enumerate().all(|(position, r)| {
            r.position == position
                && r.plan_id == plan
                && r.arm == EvaluationArm::Full
                && row_passes(r)
        })
}

pub(crate) fn inspect(
    db: &Db,
    ctx: &ToolContext,
    proposal: &str,
) -> io::Result<CandidateEvaluation> {
    let started = std::time::Instant::now();
    let source = source(db, proposal)?.ok_or_else(|| err("candidate_evaluation_missing"))?;
    let batch = config::read(db, &source.batch_id)?;
    let plan = super::plan::read(db, &source.plan_id)?;
    let generation = super::isolation::read(db, &source.generation_id)?;
    let check = config::check(db, &ctx.project_id, &batch.id, &batch.request)?;
    let captured = super::generation::source_valid(db, &source).unwrap_or(false);
    let current = source.host == ctx.repo_root.canonicalize()?.to_string_lossy()
        && source.batch_fingerprint == batch.fingerprint
        && plan.batch_id == source.batch_id
        && plan.kind == super::PlanKind::Candidate
        && plan.supplement.is_none()
        && (captured
            || (source.generation_operation.is_none() && generation.eligible_for_generation))
        && generation.material_fingerprint == source.generation_material
        && crate::proposals::evaluation_binding(db, ctx, proposal)
            .is_ok_and(|b| b.0 == source.proposal_binding)
        && !check.blocks.iter().any(|b| {
            matches!(
                b,
                super::AdmissionBlock::ConfigurationDrift | super::AdmissionBlock::RuntimeDrift
            )
        })
        && (captured
            || !check
                .blocks
                .contains(&super::AdmissionBlock::HeldoutRetired));
    let expected: BTreeSet<_> = batch
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .filter(|c| c.split == "heldout")
        .flat_map(|c| (1..=3).map(|n| (c.task.id.clone(), n)))
        .collect();
    let report = report::build_from_checked(db, Path::new(&source.host), &check)?;
    let rows: Vec<_> = report
        .groups
        .into_iter()
        .filter(|g| g.kind == report::ReportGroupKind::Candidate)
        .flat_map(|g| g.runs)
        .filter(|r| r.plan_id == source.plan_id)
        .collect();
    // No string, including a copied source_kind, grants provenance. Ticket 17's
    // host generation operation must supply independently revalidated facts.
    let trusted_source = captured
        && (source.source_kind == "live_generation"
            || (cfg!(test) && source.source_kind == "boundary_generation"))
        && check
            .blocks
            .iter()
            .all(|b| *b == super::AdmissionBlock::HeldoutRetired);
    let adoptable = qualifies(&rows, &expected, &source.plan_id, current, trusted_source);
    let original_passed = rows.iter().filter(|r| row_passes(r)).count();
    let state = if !current {
        CandidateQuality::Stale
    } else if rows.iter().any(|r| {
        matches!(
            r.state,
            PlannedState::Failed | PlannedState::Incomplete | PlannedState::NotRun
        ) || r.safety == Some(SafetyVerdict::Failed)
            || (r.state == PlannedState::Completed
                && (r.independent_passed == Some(false)
                    || r.flow_completed == Some(false)
                    || r.owner_exception == Some(true)))
    }) {
        CandidateQuality::Failed
    } else if rows.iter().any(|r| r.state != PlannedState::Completed) {
        CandidateQuality::Incomplete
    } else if adoptable {
        CandidateQuality::Qualified
    } else {
        CandidateQuality::Unverified
    };
    let reasons = if adoptable {
        vec![]
    } else {
        vec![match state {
            CandidateQuality::Stale => "candidate_binding_stale",
            CandidateQuality::Incomplete => "original_coverage_incomplete",
            CandidateQuality::Failed => "original_attempt_failed",
            _ => "independent_evidence_unverified",
        }
        .into()]
    };
    crate::diag::note(
        if adoptable {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !adoptable,
        Some(&ctx.project_id),
        Some("owner"),
        None,
        None,
        "candidate_quality",
        reasons.first().map(String::as_str).unwrap_or("qualified"),
        started,
    );
    Ok(CandidateEvaluation {
        source,
        state,
        adoptable,
        original_planned: expected.len(),
        original_passed,
        rows,
        reasons,
    })
}

pub(crate) fn validate_start(db: &Db, root: &Path, plan: &str) -> io::Result<bool> {
    let proposal: String = db
        .conn()
        .query_row(
            "SELECT proposal_id FROM evaluation_candidates WHERE plan_id=?1",
            [plan],
            |r| r.get(0),
        )
        .map_err(err)?;
    let source = source(db, &proposal)?.ok_or_else(|| err("candidate_source_missing"))?;
    if source.host != root.canonicalize()?.to_string_lossy()
        || crate::proposals::evaluation_binding(db, &ToolContext::owner(db, root), &proposal)
            .map_err(err)?
            .0
            != source.proposal_binding
    {
        return Err(err("candidate_binding_stale"));
    }
    // D05: a captured immutable candidate predating disclosure may finish its
    // own original plan. New or modified candidates need a fresh heldout set.
    if source.generation_operation.is_some() {
        if !super::generation::source_valid(db, &source)? {
            return Err(err("candidate_generation_source_invalid"));
        }
        return Ok(true);
    }
    Ok(false)
}

fn freeze_refusal(ctx: &ToolContext, code: &str, started: std::time::Instant) -> io::Error {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        None,
        None,
        "candidate_freeze",
        code,
        started,
    );
    err(code)
}

#[derive(Serialize, Deserialize)]
struct Assessment {
    source_fingerprint: String,
    plan_fingerprint: String,
    code_fingerprint: String,
    state: CandidateQuality,
}

pub(crate) fn assess(
    db: &Db,
    ctx: &ToolContext,
    proposal: &str,
) -> io::Result<CandidateEvaluation> {
    let result = inspect(db, ctx, proposal)?;
    let cached = Assessment {
        source_fingerprint: config::digest(&result.source)?,
        plan_fingerprint: config::digest(&super::plan::read(db, &result.source.plan_id)?)?,
        code_fingerprint: config::code_fingerprint()?,
        state: result.state,
    };
    db.conn().execute("UPDATE evaluation_candidates SET assessment_json=?2,assessment_fingerprint=?3 WHERE proposal_id=?1",rusqlite::params![proposal,serde_json::to_string(&cached)?,config::digest(&cached)?]).map_err(err)?;
    Ok(result)
}

/// D14/18: UI polling reads only the host record and current proposal/config
/// binding. It never scans 24 worker workspaces. A displayed qualified state is
/// explicitly the last assessment; adoption must run inspect again.
pub(crate) fn display_state(
    db: &Db,
    project: &str,
    proposal: &str,
) -> io::Result<CandidateQuality> {
    let Some(source) = source(db, proposal)? else {
        return Ok(CandidateQuality::Unverified);
    };
    let root: String = db
        .conn()
        .query_row("SELECT dir FROM projects WHERE id=?1", [project], |r| {
            r.get(0)
        })
        .map_err(err)?;
    let mut ctx = ToolContext::owner(db, Path::new(&root));
    ctx.project_id = project.into();
    let batch = config::read(db, &source.batch_id)?;
    if source.host != Path::new(&root).canonicalize()?.to_string_lossy()
        || batch.runtime.executable_fingerprint != config::code_fingerprint()?
        || !crate::proposals::evaluation_binding(db, &ctx, proposal)
            .is_ok_and(|b| b.0 == source.proposal_binding)
    {
        return Ok(CandidateQuality::Stale);
    }
    let plan = super::plan::read(db, &source.plan_id)?;
    if plan.entries.iter().any(|r| {
        matches!(
            r.state,
            PlannedState::Failed | PlannedState::Incomplete | PlannedState::NotRun
        )
    }) {
        return Ok(CandidateQuality::Failed);
    }
    if plan
        .entries
        .iter()
        .any(|r| r.state != PlannedState::Completed)
    {
        return Ok(CandidateQuality::Incomplete);
    }
    let (json,fingerprint):(Option<String>,Option<String>)=db.conn().query_row("SELECT assessment_json,assessment_fingerprint FROM evaluation_candidates WHERE proposal_id=?1",[proposal],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
    let Some(json) = json else {
        return Ok(CandidateQuality::Unverified);
    };
    let cached: Assessment = serde_json::from_str(&json)?;
    if !assessment_matches(
        &cached,
        fingerprint.as_deref(),
        &config::digest(&source)?,
        &config::digest(&plan)?,
        &config::code_fingerprint()?,
    )? {
        return Ok(CandidateQuality::Unverified);
    }
    Ok(cached.state)
}

// D12/18: this is only a last-check display hint. A false acceptance could
// enable a misleading button; mismatches therefore fall back to unverified.
fn assessment_matches(
    cached: &Assessment,
    fingerprint: Option<&str>,
    source: &str,
    plan: &str,
    code: &str,
) -> io::Result<bool> {
    Ok(fingerprint == Some(config::digest(cached)?.as_str())
        && cached.source_fingerprint == source
        && cached.plan_fingerprint == plan
        && cached.code_fingerprint == code)
}

pub(crate) fn refresh_finished_plan(db: &Db, root: &Path, plan_id: &str) -> io::Result<()> {
    let proposal: Option<String> = db
        .conn()
        .query_row(
            "SELECT proposal_id FROM evaluation_candidates WHERE plan_id=?1",
            [plan_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let Some(proposal) = proposal else {
        return Ok(());
    };
    let unfinished:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_plan_runs WHERE plan_id=?1 AND state IN ('planned','started'))",[plan_id],|r|r.get(0)).map_err(err)?;
    if !unfinished {
        assess(db, &ToolContext::owner(db, root), &proposal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Ticket 09: no generation or worker/model request is needed to inspect an
    // original, unstarted heldout plan through the real Workbench boundary.
    fn unstarted_candidate() -> (tempfile::TempDir, crate::api::Workbench, CandidateSource) {
        let root = tempfile::tempdir().unwrap();
        crate::git::init(root.path(), "main").unwrap();
        std::fs::write(root.path().join("AGENTS.md"), "Owner instructions\n").unwrap();
        crate::git::commit_all(root.path(), "seed").unwrap();
        let wb = crate::api::Workbench::for_test(root.path(), &["流程优化"], None).unwrap();
        let baseline: crate::orchestra::PackDef = serde_json::from_value(serde_json::json!({
            "name":"policy", "version":1, "stages":[
                {"name":"accept", "roles":["架构师"], "due":[], "stamp_point":true}
            ]
        }))
        .unwrap();
        baseline.pin(root.path()).unwrap();
        let mut candidate = baseline.clone();
        candidate.knobs.flag_patience = Some(7);
        let body = format!(
            "---\nkind: 改进提案\nauthor: a0\nsurface: pack_copy\ntarget: .hexagon/pack.active.json\n---\n## 动机\nTune policy\n## 改动面\n```diff\n+ knobs.flag_patience: 7\n```\n## 预期收益\nCompare independently\n## 验证方法\nFrozen heldout plan\n```replay\n{}\n```\n```judge\n{{\"verdict\":\"needs-human\",\"backend\":\"mechanical\"}}\n```\n```policy\n{}\n```\n",
            serde_json::json!({"schema":1,"scenario_fingerprint":"fixed","baseline_pack":"policy@v1","candidate_pack":"policy@v1","baseline":{"stages_done":0},"candidate":{"stages_done":1}}),
            serde_json::json!({"baseline":baseline,"candidate":candidate}),
        );
        let ctx = ToolContext::for_agent(&wb.db, root.path(), "a0");
        let artifact = crate::artifacts::deliver(
            &wb.db,
            &ctx,
            &ctx.tiers,
            "proposals/policy.md",
            &body,
            Some("改进提案"),
        )
        .unwrap();
        let proposal = crate::proposals::submit(&wb.db, &ctx, &artifact, &body).unwrap();
        let corpora: Vec<serde_json::Value> = [
            include_str!("../../../../evaluation/corpus/bugs.json"),
            include_str!("../../../../evaluation/corpus/features.json"),
            include_str!("../../../../evaluation/corpus/interfaces.json"),
            include_str!("../../../../evaluation/corpus/dirty-trees.json"),
        ]
        .into_iter()
        .map(|text| serde_json::from_str(text).unwrap())
        .collect();
        let request: config::FreezeRequest = serde_json::from_value(serde_json::json!({
            "corpora":corpora,"main_slot":"default","fast_role":"架构师", "task_owners":["架构师"],
            "full_pack":baseline,"prices":{},
            "limits":{"total_mc":20000000,"pilot_mc":2000000,"run_mc":500000,"requests":80,"active_ms":1800000},
            "statistics_version":"paired-benefit-v1"
        })).unwrap();
        let batch = wb.freeze_evaluation(&request, None).unwrap();
        let generation = wb.prepare_evaluation_generation(&batch.id).unwrap();
        let source = wb
            .freeze_policy_evaluation(&proposal, &generation.id)
            .unwrap();
        (root, wb, source)
    }

    #[test]
    fn candidate_quality_and_report_share_one_current_environment_observation() {
        let (_root, wb, source) = unstarted_candidate();
        let capture = config::VersionProbeCapture::start();
        let started = std::time::Instant::now();
        let observed = wb.policy_evaluation(&source.proposal_id).unwrap();
        let elapsed = started.elapsed();
        let probes = capture.observations();
        let probe_elapsed: std::time::Duration = probes.iter().map(|p| p.elapsed).sum();
        eprintln!(
            "R8 inspect probes={} transaction_probes={} probe_ms={:.3} operation_ms={:.3} probe_fraction={:.6}",
            probes.len(), probes.iter().filter(|p| p.in_transaction).count(),
            probe_elapsed.as_secs_f64() * 1000.0, elapsed.as_secs_f64() * 1000.0,
            probe_elapsed.as_secs_f64() / elapsed.as_secs_f64(),
        );
        assert_eq!(observed.state, CandidateQuality::Incomplete);
        assert!(!observed.adoptable);
        assert_eq!(observed.original_planned, 24);
        assert_eq!(
            probes.len(),
            1,
            "quality/report share this call's fresh observation"
        );
        assert!(probes.iter().all(|p| !p.in_transaction));
    }

    #[test]
    fn standalone_reports_and_candidate_calls_each_observe_the_environment_again() {
        let (_root, wb, source) = unstarted_candidate();
        for call in 0..4 {
            let capture = config::VersionProbeCapture::start();
            if call % 2 == 0 {
                assert!(!wb.policy_evaluation(&source.proposal_id).unwrap().adoptable);
            } else {
                let report = wb.evaluation_benefit_report(&source.batch_id).unwrap();
                assert_eq!(report.batch_id, source.batch_id);
                assert_eq!(report.batch_fingerprint, source.batch_fingerprint);
            }
            let observations = capture.observations();
            assert_eq!(
                observations.len(),
                1,
                "call {call} needs its own fresh probe"
            );
            assert!(!observations[0].in_transaction);
        }
    }

    #[test]
    fn new_environment_drift_still_refuses_candidate_quality_and_standalone_report() {
        let (_root, wb, source) = unstarted_candidate();
        assert_eq!(
            wb.policy_evaluation(&source.proposal_id).unwrap().state,
            CandidateQuality::Incomplete
        );
        // Change the real frozen role-definition input, as configuration tests
        // already do. This supplies no quality receipt or altered verdict.
        wb.db.conn().execute(
            "INSERT INTO role_defs(project_id,name,duty,model_slot,skills) VALUES (?1,'架构师','changed environment instructions','default','[]')",
            [&wb.project_id],
        ).unwrap();
        let capture = config::VersionProbeCapture::start();
        let observed = wb.policy_evaluation(&source.proposal_id).unwrap();
        assert_eq!(observed.state, CandidateQuality::Stale);
        assert!(!observed.adoptable);
        let report = wb.evaluation_benefit_report(&source.batch_id).unwrap();
        assert!(report
            .configuration_blocks
            .contains(&super::super::AdmissionBlock::RuntimeDrift));
        assert_eq!(capture.observations().len(), 2);
    }

    fn good_rows() -> Vec<BenefitRun> {
        (0..24)
            .map(|position| {
                let run = format!("run-{position}");
                BenefitRun {
                    plan_id: "candidate-plan".into(),
                    position,
                    task_id: format!("task-{}", position / 3),
                    category: "bug".into(),
                    repetition: (position % 3 + 1) as u32,
                    arm: EvaluationArm::Full,
                    state: PlannedState::Completed,
                    run_id: Some(run.clone()),
                    evidence_kind: Some("live_model".into()),
                    flow_completed: Some(true),
                    independent_passed: Some(true),
                    owner_exception: Some(false),
                    formal_success: true,
                    safety: Some(SafetyVerdict::Passed),
                    human_ms: Some(100),
                    active_ms: Some(100),
                    known_mc: Some(10),
                    unknown_mc: Some(0),
                    in_flight_mc: Some(0),
                    requests: Some(1),
                    outcome: Some(super::super::OutcomeObservation {
                        id: format!("outcome-{position}"),
                        run_id: run,
                        observed_at_ms: 1,
                        task_fingerprint: "task".into(),
                        execution_state: "completed".into(),
                        evidence_fingerprint: "evidence".into(),
                        files_fingerprint: Some("files".into()),
                        action_count: 1,
                        last_event_id: 1,
                        acceptance_is_current: true,
                        independent_passed: true,
                        flow_completed: true,
                        owner_exception: false,
                        safety: SafetyVerdict::Passed,
                        violations: vec![],
                        unknowns: vec![],
                        formal_success: true,
                    }),
                    reasons: vec![],
                }
            })
            .collect()
    }
    proptest! {
        #[test]
        fn stale_cached_quality_never_enables_a_fresh_candidate(suffix in "x[a-z]{1,20}", field in 0u8..4) {
            let mut cached=Assessment{source_fingerprint:"source".into(),plan_fingerprint:"plan".into(),code_fingerprint:"code".into(),state:CandidateQuality::Qualified};
            let fingerprint=config::digest(&cached).unwrap();
            prop_assert!(assessment_matches(&cached,Some(&fingerprint),"source","plan","code").unwrap());
            prop_assert!(!assessment_matches(&cached,None,"source","plan","code").unwrap());
            match field {0=>cached.source_fingerprint.push_str(&suffix),1=>cached.plan_fingerprint.push_str(&suffix),2=>cached.code_fingerprint.push_str(&suffix),_=>cached.state=CandidateQuality::Failed}
            prop_assert!(!assessment_matches(&cached,Some(&fingerprint),"source","plan","code").unwrap());
            // Evaluation-18: a valid digest of an old assessment cannot bind new inputs.
            if field < 3 {
                let valid_digest=config::digest(&cached).unwrap();
                prop_assert!(!assessment_matches(&cached,Some(&valid_digest),"source","plan","code").unwrap());
            }
        }
        #[test]
        fn candidate_qualification_cannot_gain_from_missing_failed_foreign_or_unknown_evidence(index in 0usize..24, corruption in 0u8..10) {
            let mut rows=good_rows();
            let expected=rows.iter().map(|r|(r.task_id.clone(),r.repetition)).collect();
            prop_assert!(qualifies(&rows,&expected,"candidate-plan",true,true));
            match corruption {
                0 => { rows.remove(index); },
                1 => rows[index].independent_passed=Some(false),
                2 => rows[index].unknown_mc=Some(1),
                3 => rows[index].plan_id="other-plan".into(),
                4 => rows[index].task_id="other-task".into(),
                5 => rows[index].run_id=rows[(index+1)%24].run_id.clone(),
                6 => rows[index].known_mc=None,
                7 => rows[index].safety=Some(SafetyVerdict::Failed),
                8 => rows[index].evidence_kind=Some("scripted_debug".into()),
                _ => rows[index].outcome.as_mut().unwrap().unknowns.push("unknown_action".into()),
            }
            prop_assert!(!qualifies(&rows,&expected,"candidate-plan",true,true));
            prop_assert!(!qualifies(&good_rows(),&expected,"candidate-plan",false,true));
            prop_assert!(!qualifies(&good_rows(),&expected,"candidate-plan",true,false));
        }
    }
}
