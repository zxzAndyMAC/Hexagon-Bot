//! D11/D12: raw original attempts, complete costs and matched time samples.
use super::{
    budget, config, err, human, outcome, plan, EvaluationArm, PlannedState, SafetyVerdict,
};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportGroupKind {
    Development,
    Pilot,
    Formal,
    Supplement,
    Candidate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenefitRecommendation {
    ExploratoryFull,
    NoDemonstratedAdvantage,
    InsufficientEvidence,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenefitRun {
    pub plan_id: String,
    pub position: usize,
    pub task_id: String,
    pub category: String,
    pub repetition: u32,
    pub arm: EvaluationArm,
    pub state: PlannedState,
    pub run_id: Option<String>,
    pub evidence_kind: Option<String>,
    pub flow_completed: Option<bool>,
    pub independent_passed: Option<bool>,
    pub owner_exception: Option<bool>,
    pub formal_success: bool,
    pub safety: Option<SafetyVerdict>,
    pub human_ms: Option<u64>,
    pub active_ms: Option<u64>,
    pub known_mc: Option<u64>,
    pub unknown_mc: Option<u64>,
    pub in_flight_mc: Option<u64>,
    pub requests: Option<u64>,
    pub outcome: Option<super::OutcomeObservation>,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArmBenefits {
    pub planned: usize,
    pub started: usize,
    pub successes: usize,
    pub success_rate: Option<f64>,
    pub known_mc: Option<u64>,
    pub unknown_mc: Option<u64>,
    pub in_flight_mc: Option<u64>,
    pub unmeasured_cost_runs: usize,
    pub costs_complete: bool,
    pub cost_per_success_mc: Option<f64>,
    pub human_median_ms: Option<f64>,
    pub active_median_ms: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairBenefits {
    pub task_id: String,
    pub repetition: u32,
    pub fast_run: Option<String>,
    pub full_run: Option<String>,
    pub fast_known_mc: Option<u64>,
    pub full_known_mc: Option<u64>,
    pub fast_human_ms: Option<u64>,
    pub full_human_ms: Option<u64>,
    pub fast_active_ms: Option<u64>,
    pub full_active_ms: Option<u64>,
    pub included: bool,
    pub exclusions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryBenefits {
    pub category: String,
    pub fast: ArmBenefits,
    pub full: ArmBenefits,
    pub paired_samples: usize,
    pub human_ratio_range: Option<(f64, f64)>,
    pub active_ratio_range: Option<(f64, f64)>,
    pub cost_ratio_range: Option<(f64, f64)>,
    pub pairs: Vec<PairBenefits>,
    pub recommendation: BenefitRecommendation,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenefitGroup {
    pub kind: ReportGroupKind,
    pub runs: Vec<BenefitRun>,
    pub categories: Vec<CategoryBenefits>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSource {
    pub task_id: String,
    pub category: String,
    pub split: String,
    pub fingerprint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenefitReport {
    pub version: u32,
    pub runtime_fingerprint: String,
    pub main_slot: String,
    pub fast_role: String,
    pub full_pack_fingerprint: String,
    pub models: std::collections::BTreeMap<String, super::config::ModelSnapshot>,
    pub task_sources: Vec<TaskSource>,
    pub batch_id: String,
    pub batch_fingerprint: String,
    pub statistics_version: String,
    pub configuration_blocks: Vec<super::AdmissionBlock>,
    pub groups: Vec<BenefitGroup>,
    pub paid_round: super::BudgetSummary,
    pub scripted_round: super::BudgetSummary,
    pub limitations: Vec<String>,
}
impl BenefitReport {
    pub fn markdown(&self) -> String {
        use std::fmt::Write;
        let mut text = format!(
            "# Evaluation benefit report v{}\n\nBatch: {}\n\nFingerprint: {}\n",
            self.version, self.batch_id, self.batch_fingerprint
        );
        let _ = writeln!(text, "\nRuntime fingerprint: {}\n\nStatistics: {} | main slot: {} | fast role: {} | full pack: {}\n\nConfiguration blocks: {:?}", self.runtime_fingerprint, self.statistics_version, self.main_slot, self.fast_role, self.full_pack_fingerprint, self.configuration_blocks);
        for (slot, model) in &self.models {
            let _ = writeln!(
                text,
                "\nModel {slot}: {} / {} ({:?})",
                model.provider_id, model.model, model.endpoint
            );
        }
        for task in &self.task_sources {
            let _ = writeln!(
                text,
                "\nTask {} / {} / {}: {}",
                task.task_id, task.category, task.split, task.fingerprint
            );
        }
        for group in &self.groups {
            let _=write!(text,"\n## {:?}\n\n{} planned runs\n\n| Category | Fast success / started / planned | Full success / started / planned | Matched times | Conclusion |\n| --- | --- | --- | --- | --- |\n",group.kind,group.runs.len());
            for c in &group.categories {
                let _ = writeln!(
                    text,
                    "| {} | {} / {} / {} | {} / {} / {} | {} | {:?}: {} |",
                    c.category,
                    c.fast.successes,
                    c.fast.started,
                    c.fast.planned,
                    c.full.successes,
                    c.full.started,
                    c.full.planned,
                    c.paired_samples,
                    c.recommendation,
                    c.reasons.join(", ")
                );
            }
            let _ = writeln!(text, "\n| Category / arm | Known / unknown / in-flight mc | Cost / success mc | Human median ms | Active median ms | Unmeasured costs |\n| --- | --- | --- | --- | --- | --- |");
            for category in &group.categories {
                for (name, arm) in [("fast", &category.fast), ("full", &category.full)] {
                    let _ = writeln!(
                        text,
                        "| {} / {} | {:?} / {:?} / {:?} | {:?} | {:?} | {:?} | {} |",
                        category.category,
                        name,
                        arm.known_mc,
                        arm.unknown_mc,
                        arm.in_flight_mc,
                        arm.cost_per_success_mc,
                        arm.human_median_ms,
                        arm.active_median_ms,
                        arm.unmeasured_cost_runs
                    );
                }
                let _ = writeln!(
                    text,
                    "\nPair details — {} (full / fast; absent values are unknown):\n",
                    category.category
                );
                for pair in &category.pairs {
                    let human_ratio = ratio(pair.full_human_ms, pair.fast_human_ms);
                    let active_ratio = ratio(pair.full_active_ms, pair.fast_active_ms);
                    let _ = writeln!(text, "- {} #{}: runs {:?} / {:?}; human {:?} / {:?} ms (ratio {:?}); active {:?} / {:?} ms (ratio {:?}); included {}; excluded {:?}", pair.task_id, pair.repetition, pair.full_run, pair.fast_run, pair.full_human_ms, pair.fast_human_ms, human_ratio, pair.full_active_ms, pair.fast_active_ms, active_ratio, pair.included, pair.exclusions);
                    let _ = writeln!(
                        text,
                        "  Known costs including failed attempts: {:?} / {:?} mc; ratio {:?}.",
                        pair.full_known_mc,
                        pair.fast_known_mc,
                        ratio(pair.full_known_mc, pair.fast_known_mc)
                    );
                }
                let _ = writeln!(text, "\nMatched time ratio ranges: human {:?}; active {:?}. All started pair cost ratio range: {:?}.", category.human_ratio_range, category.active_ratio_range, category.cost_ratio_range);
            }
            let _ = writeln!(text, "\n| Task / repetition / arm | Plan position / run | State / evidence | Flow / independent / exception | Formal success / safety | Human / active ms | Known / unknown / in-flight mc | Reasons |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
            for r in &group.runs {
                let _ = writeln!(text, "| {} / {} / {:?} | {}:{} / {:?} | {:?} / {:?} | {:?} / {:?} / {:?} | {} / {:?} | {:?} / {:?} | {:?} / {:?} / {:?} | {} |", r.task_id, r.repetition, r.arm, r.plan_id, r.position, r.run_id, r.state, r.evidence_kind, r.flow_completed, r.independent_passed, r.owner_exception, r.formal_success, r.safety, r.human_ms, r.active_ms, r.known_mc, r.unknown_mc, r.in_flight_mc, r.reasons.join(", "));
            }
            let human_total = group
                .runs
                .iter()
                .filter_map(|r| r.human_ms)
                .map(u128::from)
                .sum::<u128>();
            let active_total = group
                .runs
                .iter()
                .filter_map(|r| r.active_ms)
                .map(u128::from)
                .sum::<u128>();
            let missing = group
                .runs
                .iter()
                .filter(|r| r.run_id.is_some() && (r.human_ms.is_none() || r.active_ms.is_none()))
                .count();
            let _ = writeln!(text, "\nMeasured total effort including failures: human {human_total} ms; active {active_total} ms; {missing} started runs with missing time. Supplement totals stay separate above and contribute to round expenditure below.");
        }
        let _=write!(text,"\nPaid round: known {} mc; reserved {} mc (includes unknown {} mc and in-flight {} mc). Scripted round: known {} mc, unknown {} mc.\n",self.paid_round.known_mc,self.paid_round.reserved_mc,self.paid_round.unknown_mc,self.paid_round.in_flight_mc,self.scripted_round.known_mc,self.scripted_round.unknown_mc);
        for limitation in &self.limitations {
            let _ = writeln!(text, "\n- {limitation}");
        }
        text
    }
}

fn ratio(full: Option<u64>, fast: Option<u64>) -> Option<f64> {
    full.zip(fast)
        .filter(|(_, fast)| *fast != 0)
        .map(|(full, fast)| full as f64 / fast as f64)
}
fn range(values: &[f64]) -> Option<(f64, f64)> {
    values.first().map(|first| {
        values
            .iter()
            .fold((*first, *first), |(min, max), v| (min.min(*v), max.max(*v)))
    })
}

pub(crate) fn build(db: &Db, root: &Path, batch_id: &str) -> io::Result<BenefitReport> {
    let started = std::time::Instant::now();
    let frozen = config::read(db, batch_id)?;
    let current = config::check(db, crate::PROJECT_ID, batch_id, &frozen.request)?;
    let scripted = budget::summary(db, "scripted_debug")?;
    let paid = budget::paid_summary()?;
    let host = root.canonicalize()?.to_string_lossy().into_owned();
    let mut q = db
        .conn()
        .prepare("SELECT id FROM evaluation_plans WHERE batch_id=?1 ORDER BY id")
        .map_err(err)?;
    let ids = q
        .query_map([batch_id], |r| r.get::<_, String>(0))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut groups: Vec<_> = [
        ReportGroupKind::Development,
        ReportGroupKind::Pilot,
        ReportGroupKind::Formal,
        ReportGroupKind::Supplement,
        ReportGroupKind::Candidate,
    ]
    .into_iter()
    .map(|kind| BenefitGroup {
        kind,
        runs: vec![],
        categories: vec![],
    })
    .collect();
    for id in ids {
        let plan = plan::read(db, &id)?;
        let kind = if plan.supplement.is_some() {
            ReportGroupKind::Supplement
        } else {
            match plan.kind {
                plan::PlanKind::Pilot => ReportGroupKind::Pilot,
                plan::PlanKind::Formal => ReportGroupKind::Formal,
                plan::PlanKind::Candidate => ReportGroupKind::Candidate,
            }
        };
        let group = groups
            .iter_mut()
            .find(|g| g.kind == kind)
            .ok_or_else(|| err("report group missing"))?;
        for entry in &plan.entries {
            let task = &frozen
                .request
                .corpora
                .iter()
                .flat_map(|c| &c.cases)
                .find(|c| c.task.id == entry.task_id)
                .ok_or_else(|| err("report task binding missing"))?
                .task;
            let mut row = BenefitRun {
                plan_id: id.clone(),
                position: entry.position,
                task_id: entry.task_id.clone(),
                category: task.category.clone(),
                repetition: entry.repetition,
                arm: entry.arm,
                state: entry.state.clone(),
                run_id: entry.run_id.clone(),
                evidence_kind: None,
                flow_completed: None,
                independent_passed: None,
                owner_exception: None,
                formal_success: false,
                safety: None,
                human_ms: None,
                active_ms: None,
                known_mc: None,
                unknown_mc: None,
                in_flight_mc: None,
                requests: None,
                outcome: None,
                reasons: vec![],
            };
            if let Some(run_id) = &entry.run_id {
                match super::read(db, run_id) {
                    Err(_) => row
                        .reasons
                        .push("original_result_missing_or_corrupt".into()),
                    Ok(run) => {
                        row.evidence_kind = Some(run.evidence_kind.clone());
                        row.flow_completed = Some(run.flow_completed);
                        row.independent_passed = Some(run.independent_passed);
                        row.owner_exception = Some(run.owner_exception);
                        // Ticket 15 / D10: a moved result must not confer its
                        // success on another frozen task or planned position.
                        let bound = run.task_id == task.id
                            && run.task_fingerprint == config::digest(task)?
                            && super::control::validate_owner(root, &run).is_ok();
                        if !bound {
                            row.reasons.push("run_binding_corrupt".into());
                        }
                        if bound {
                            match outcome::snapshot(db, run_id) {
                                Ok(observed) => {
                                    row.formal_success = observed.formal_success
                                        && entry.state == PlannedState::Completed;
                                    row.safety = Some(observed.safety);
                                    row.outcome = Some(observed);
                                }
                                Err(_) => row.reasons.push("outcome_unavailable".into()),
                            }
                        }
                        if let Ok(timing) = human::timing(db, run_id) {
                            row.active_ms = timing.active_ms;
                            if timing.human_benefit_eligible {
                                row.human_ms = timing.human_ms;
                            }
                        }
                        let ledger = if matches!(
                            run.evidence_kind.as_str(),
                            "live_model" | "provider_boundary_fixture"
                        ) {
                            &paid
                        } else {
                            &scripted
                        };
                        // Ticket 15: macOS /var and /private/var name the same
                        // temp workspace; compare canonical paths as the ledger does.
                        let canonical_workspace = Path::new(&run.workspace).canonicalize().ok();
                        if let Some(fee) = ledger.runs.iter().find(|r| {
                            r.run_id.as_deref() == Some(run_id)
                                && r.host == host
                                && r.plan_id == id
                                && r.position == entry.position as u64
                                && r.workspace.as_deref().map(Path::new)
                                    == canonical_workspace.as_deref()
                        }) {
                            row.known_mc = Some(fee.known_mc);
                            row.unknown_mc = Some(fee.unknown_mc);
                            row.in_flight_mc = Some(fee.in_flight_mc);
                            row.requests = Some(fee.requests);
                        } else {
                            row.reasons.push("cost_unmeasured".into());
                            if ledger.runs.iter().any(|fee| {
                                fee.host == host && fee.run_id.as_deref() == Some(run_id)
                            }) {
                                // D10: a known receipt at another position is
                                // corrupt provenance, not merely absent pricing.
                                row.formal_success = false;
                                row.reasons.push("run_binding_corrupt".into());
                            }
                        }
                        if run.evidence_kind != "live_model" {
                            row.reasons.push("scripted_or_unverified".into());
                        }
                    }
                }
            } else {
                row.reasons.push("not_started".into());
            }
            group.runs.push(row);
        }
    }
    for group in &mut groups {
        for category in group
            .runs
            .iter()
            .map(|r| r.category.clone())
            .collect::<BTreeSet<_>>()
        {
            let rows: Vec<_> = group
                .runs
                .iter()
                .filter(|r| r.category == category)
                .collect();
            let mut summary = summarize(&category, &rows);
            if group.kind != ReportGroupKind::Formal {
                summary.recommendation = BenefitRecommendation::InsufficientEvidence;
                summary
                    .reasons
                    .push("not_original_formal_comparison".into());
            }
            if !current.blocks.is_empty() {
                summary.recommendation = BenefitRecommendation::InsufficientEvidence;
                summary
                    .reasons
                    .push("configuration_unverified_or_drifted".into());
            }
            let reason = match summary.recommendation {
                BenefitRecommendation::ExploratoryFull => "exploratory_full",
                BenefitRecommendation::NoDemonstratedAdvantage => "no_demonstrated_advantage",
                BenefitRecommendation::InsufficientEvidence => "insufficient_evidence",
            };
            crate::diag::note(
                crate::diag::CLASS_JUDGE,
                false,
                Some(crate::PROJECT_ID),
                None,
                None,
                None,
                "evaluation_benefit_report",
                reason,
                started,
            );
            group.categories.push(summary);
        }
    }
    let task_sources = frozen
        .request
        .corpora
        .iter()
        .flat_map(|c| &c.cases)
        .map(|c| {
            Ok(TaskSource {
                task_id: c.task.id.clone(),
                category: c.task.category.clone(),
                split: c.split.clone(),
                fingerprint: config::digest(&c.task)?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(BenefitReport{version:1,runtime_fingerprint:config::digest(&frozen.runtime)?,main_slot:frozen.request.main_slot.clone(),fast_role:frozen.request.fast_role.clone(),full_pack_fingerprint:config::digest(&frozen.request.full_pack)?,models:frozen.runtime.models.clone(),task_sources,batch_id:batch_id.into(),batch_fingerprint:frozen.fingerprint,statistics_version:frozen.request.statistics_version,configuration_blocks:current.blocks,groups,paid_round:paid,scripted_round:scripted,limitations:vec!["Only two heldout tasks per category; exploratory evidence, not statistical significance or general software coverage.".into(),"Balanced first-side order limits but does not remove owner learning effects; retain task/repetition detail.".into(),"Original failures remain in success and cost denominators; supplements never replace original attempts.".into(),"Scripted calls and missing human measurements cannot prove human-time savings; paid and scripted spend stay separate.".into(),"Standalone development runs without this frozen batch binding are excluded, not relabelled as formal evidence.".into(),"This report never changes product defaults, models, roles or workflows.".into()]})
}
fn arm(rows: &[&BenefitRun], which: EvaluationArm) -> ArmBenefits {
    let selected: Vec<_> = rows.iter().copied().filter(|r| r.arm == which).collect();
    let started: Vec<_> = selected
        .iter()
        .copied()
        .filter(|r| r.run_id.is_some())
        .collect();
    let successes = started.iter().filter(|r| r.formal_success).count();
    let known = started
        .iter()
        .filter_map(|r| r.known_mc)
        .try_fold(0u64, |n, fee| n.checked_add(fee));
    let unknown = started
        .iter()
        .filter_map(|r| r.unknown_mc)
        .try_fold(0u64, |n, fee| n.checked_add(fee));
    let flight = started
        .iter()
        .filter_map(|r| r.in_flight_mc)
        .try_fold(0u64, |n, fee| n.checked_add(fee));
    let unmeasured = started
        .iter()
        .filter(|r| r.known_mc.is_none() || r.unknown_mc.is_none() || r.in_flight_mc.is_none())
        .count();
    let complete = !started.is_empty()
        && unmeasured == 0
        && known.is_some()
        && unknown == Some(0)
        && flight == Some(0);
    ArmBenefits {
        planned: selected.len(),
        started: started.len(),
        successes,
        success_rate: (!started.is_empty())
            .then_some(successes as f64 / started.len().max(1) as f64),
        known_mc: known,
        unknown_mc: unknown,
        in_flight_mc: flight,
        unmeasured_cost_runs: unmeasured,
        costs_complete: complete,
        cost_per_success_mc: if complete && successes > 0 {
            known.map(|v| v as f64 / successes as f64)
        } else {
            None
        },
        ..Default::default()
    }
}
// D12: only matched successes supply time samples. False exclusion reduces
// sample size; false inclusion invents a human benefit from missing/failed work.
fn pair_eligible(fast: &BenefitRun, full: &BenefitRun) -> bool {
    fast.formal_success
        && full.formal_success
        && fast.human_ms.is_some()
        && full.human_ms.is_some()
        && fast.active_ms.is_some()
        && full.active_ms.is_some()
}
fn median_twice(values: impl Iterator<Item = u64>) -> Option<u128> {
    let mut values: Vec<_> = values.collect();
    values.sort_unstable();
    let n = values.len();
    if n == 0 {
        None
    } else {
        Some(u128::from(values[(n - 1) / 2]) + u128::from(values[n / 2]))
    }
}
// Use exact integer ratios for decisions; f64 values are presentation only.
fn within(full: u128, fast: u128, numerator: u128, denominator: u128) -> Option<bool> {
    if fast == 0 {
        return None;
    }
    Some(full.checked_mul(denominator)? <= fast.checked_mul(numerator)?)
}
fn fluctuates(values: &[(u64, u64)], numerator: u128, denominator: u128) -> bool {
    let below = values.iter().any(|(f, b)| f < b);
    let above = values.iter().any(|(f, b)| f > b);
    let pass = values
        .iter()
        .any(|(f, b)| within((*f).into(), (*b).into(), numerator, denominator) == Some(true));
    let fail = values
        .iter()
        .any(|(f, b)| within((*f).into(), (*b).into(), numerator, denominator) == Some(false));
    (below && above) || (pass && fail)
}
pub(crate) fn summarize(category: &str, rows: &[&BenefitRun]) -> CategoryBenefits {
    let mut result = CategoryBenefits {
        category: category.into(),
        fast: arm(rows, EvaluationArm::Fast),
        full: arm(rows, EvaluationArm::Full),
        paired_samples: 0,
        human_ratio_range: None,
        active_ratio_range: None,
        cost_ratio_range: None,
        pairs: vec![],
        recommendation: BenefitRecommendation::InsufficientEvidence,
        reasons: vec![],
    };
    let keys: BTreeSet<_> = rows
        .iter()
        .map(|r| (&r.plan_id, &r.task_id, r.repetition))
        .collect();
    let tasks: BTreeSet<_> = rows.iter().map(|r| &r.task_id).collect();
    let mut human = vec![];
    let mut active = vec![];
    let mut costs = vec![];
    let mut measured_tasks = BTreeSet::new();
    for (plan, task, repetition) in &keys {
        let matching: Vec<_> = rows
            .iter()
            .copied()
            .filter(|r| &r.plan_id == *plan && &r.task_id == *task && r.repetition == *repetition)
            .collect();
        let fast: Vec<_> = matching
            .iter()
            .copied()
            .filter(|r| r.arm == EvaluationArm::Fast)
            .collect();
        let full: Vec<_> = matching
            .iter()
            .copied()
            .filter(|r| r.arm == EvaluationArm::Full)
            .collect();
        let mut pair = PairBenefits {
            task_id: (*task).clone(),
            repetition: *repetition,
            fast_run: None,
            full_run: None,
            fast_known_mc: None,
            full_known_mc: None,
            fast_human_ms: None,
            full_human_ms: None,
            fast_active_ms: None,
            full_active_ms: None,
            included: false,
            exclusions: vec![],
        };
        if let ([f], [h]) = (fast.as_slice(), full.as_slice()) {
            pair.fast_run = f.run_id.clone();
            pair.full_run = h.run_id.clone();
            pair.fast_known_mc = f.known_mc;
            pair.full_known_mc = h.known_mc;
            pair.fast_human_ms = f.human_ms;
            pair.full_human_ms = h.human_ms;
            pair.fast_active_ms = f.active_ms;
            pair.full_active_ms = h.active_ms;
            pair.included = pair_eligible(f, h);
            if !(f.formal_success && h.formal_success) {
                pair.exclusions.push("both_sides_not_successful".into());
            } else if !pair.included {
                pair.exclusions
                    .push("human_or_active_measurement_missing".into());
                result.reasons.push("paired_measurements_incomplete".into());
            }
            if pair.included {
                if let (Some(fh), Some(hh), Some(fa), Some(ha)) =
                    (f.human_ms, h.human_ms, f.active_ms, h.active_ms)
                {
                    human.push((hh, fh));
                    active.push((ha, fa));
                    measured_tasks.insert(&f.task_id);
                }
            }
            // Ticket 15 / D11-D12: failed attempts still influence expense
            // consistency. Filtering by successful time pairs hid reversals.
            if f.run_id.is_some() && h.run_id.is_some() {
                if let (Some(fee_f), Some(fee_h)) = (f.known_mc, h.known_mc) {
                    costs.push((fee_h, fee_f));
                }
            }
        } else {
            pair.exclusions.push("pair_missing_or_duplicated".into());
            result.reasons.push("pair_binding_incomplete".into());
        }
        result.pairs.push(pair);
    }
    result.paired_samples = human.len();
    let ratios = |values: &[(u64, u64)]| {
        range(
            &values
                .iter()
                .filter_map(|(f, b)| ratio(Some(*f), Some(*b)))
                .collect::<Vec<_>>(),
        )
    };
    result.human_ratio_range = ratios(&human);
    result.active_ratio_range = ratios(&active);
    result.cost_ratio_range = ratios(&costs);
    let fast_human = median_twice(human.iter().map(|v| v.1));
    let full_human = median_twice(human.iter().map(|v| v.0));
    let fast_active = median_twice(active.iter().map(|v| v.1));
    let full_active = median_twice(active.iter().map(|v| v.0));
    result.fast.human_median_ms = fast_human.map(|v| v as f64 / 2.0);
    result.full.human_median_ms = full_human.map(|v| v as f64 / 2.0);
    result.fast.active_median_ms = fast_active.map(|v| v as f64 / 2.0);
    result.full.active_median_ms = full_active.map(|v| v as f64 / 2.0);
    if rows.len() != 12
        || keys.len() != 6
        || tasks.len() != 2
        || result.fast.started != 6
        || result.full.started != 6
        || rows.iter().any(|r| {
            !matches!(
                r.state,
                PlannedState::Completed | PlannedState::Failed | PlannedState::Incomplete
            ) || !(1..=3).contains(&r.repetition)
        })
    {
        result.reasons.push("original_coverage_incomplete".into());
    }
    if measured_tasks != tasks {
        result
            .reasons
            .push("paired_task_coverage_incomplete".into());
    }
    if rows
        .iter()
        .any(|r| r.evidence_kind.as_deref() != Some("live_model"))
    {
        result.reasons.push("nonlive_or_missing_evidence".into());
    }
    if rows
        .iter()
        .any(|r| r.safety.is_none() || r.safety == Some(SafetyVerdict::Unknown))
    {
        result.reasons.push("safety_evidence_incomplete".into());
    }
    if !result.fast.costs_complete || !result.full.costs_complete {
        result.reasons.push("cost_evidence_incomplete".into());
    }
    if result.fast.successes == 0 || result.full.successes == 0 {
        result.reasons.push("zero_success".into());
    }
    if fast_human.is_none_or(|v| v == 0)
        || fast_active.is_none_or(|v| v == 0)
        || result.fast.known_mc.is_none_or(|v| v == 0)
        || human.iter().any(|v| v.1 == 0)
        || active.iter().any(|v| v.1 == 0)
        || costs.iter().any(|v| v.1 == 0)
    {
        result.reasons.push("zero_or_missing_baseline".into());
    }
    for (values, num, den, name) in [
        (&human, 4, 5, "human_direction_or_threshold_varies"),
        (&active, 5, 4, "active_direction_or_threshold_varies"),
        (&costs, 3, 2, "cost_direction_or_threshold_varies"),
    ] {
        if fluctuates(values, num, den) {
            result.reasons.push(name.into());
        }
    }
    result.reasons.sort();
    result.reasons.dedup();
    if result.reasons.is_empty() {
        let success = (result.full.successes as u128) * (result.fast.started as u128)
            >= (result.fast.successes as u128) * (result.full.started as u128);
        let safe = rows.iter().all(|r| r.safety == Some(SafetyVerdict::Passed));
        let human_pass = full_human
            .zip(fast_human)
            .and_then(|(f, b)| within(f, b, 4, 5))
            == Some(true);
        let active_pass = full_active
            .zip(fast_active)
            .and_then(|(f, b)| within(f, b, 5, 4))
            == Some(true);
        let cost_pass = result
            .full
            .known_mc
            .zip(result.fast.known_mc)
            .and_then(|(f, b)| {
                within(
                    u128::from(f) * (result.fast.successes as u128),
                    u128::from(b) * (result.full.successes as u128),
                    3,
                    2,
                )
            })
            == Some(true);
        result.recommendation = if success && safe && human_pass && active_pass && cost_pass {
            BenefitRecommendation::ExploratoryFull
        } else {
            BenefitRecommendation::NoDemonstratedAdvantage
        };
        for (pass, reason) in [
            (success, "success_rate_lower"),
            (safe, "safety_violation"),
            (human_pass, "human_reduction_below_twenty_percent"),
            (active_pass, "active_increase_over_twentyfive_percent"),
            (cost_pass, "cost_increase_over_fifty_percent"),
        ] {
            if !pass {
                result.reasons.push(reason.into());
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn threshold_cannot_improve_when_full_cost_increases(
            base in 1u64..=u64::MAX,
            low in any::<u64>(),
            increment in any::<u64>(),
        ) {
            let high = u128::from(low) + u128::from(increment);
            if within(u128::from(low), u128::from(base), 3, 2) == Some(false) {
                prop_assert_eq!(within(high, u128::from(base), 3, 2), Some(false));
            }
        }
        #[test]
        fn zero_baseline_is_never_comparable(full in any::<u128>()) {
            prop_assert_eq!(within(full, 0, 3, 2), None);
        }
        #[test]
        fn exact_threshold_includes_boundary_and_rejects_one_more(base in 1u64..=u64::MAX/4) {
            let base = u128::from(base);
            prop_assert_eq!(within(base * 3, base * 2, 3, 2), Some(true));
            prop_assert_eq!(within(base * 3 + 1, base * 2, 3, 2), Some(false));
        }
    }
}
