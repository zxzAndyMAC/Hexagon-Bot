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
    let report = report::build(db, Path::new(&source.host), &batch.id)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
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
