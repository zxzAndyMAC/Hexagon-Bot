//! D08: shared reservations; fixture accounting never spends the paid authority.
use super::{config, err, plan, EvaluationResult};
use crate::{
    db::Db,
    provider::{ModelProvider, Usage},
    tools::ToolContext,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
const DEBUG: &str = "scripted_debug";
const PAID: &str = "paid_first_round";
const TOTAL: u64 = 20_000_000;
const PILOT: u64 = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugPrice {
    pub prompt_per_1k_mc: u64,
    pub completion_per_1k_mc: u64,
    pub prompt_bound: u64,
    pub output_bound: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetRunSummary {
    pub host: String,
    pub plan_id: String,
    pub position: u64,
    pub workspace: Option<String>,
    pub key: String,
    pub run_id: Option<String>,
    pub closed: bool,
    pub requests: u64,
    pub confirmed_requests: u64,
    pub known_mc: u64,
    pub unknown_mc: u64,
    pub in_flight_mc: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSummary {
    pub evidence_kind: String,
    pub limit_mc: u64,
    pub pilot_limit_mc: u64,
    pub known_mc: u64,
    pub reserved_mc: u64,
    pub unknown_mc: u64,
    pub in_flight_mc: u64,
    pub available_mc: u64,
    pub pilot_exposure_mc: u64,
    pub requests: u64,
    pub confirmed_requests: u64,
    pub blocked: bool,
    pub runs: Vec<BudgetRunSummary>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    host: String,
    run_key: String,
    scope: String,
}

fn rejected(reason: &str) -> io::Error {
    crate::diag::note(
        crate::diag::CLASS_REJECT,
        true,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_budget",
        reason,
        std::time::Instant::now(),
    );
    err(format!("evaluation budget: {reason}"))
}
fn cost(price: &DebugPrice, prompt: u64, completion: u64) -> Option<u64> {
    // D08: round upward and reject arithmetic overflow. False refusal costs
    // another verified price; a wrapped small value spends unapproved money.
    let p = u128::from(prompt).checked_mul(u128::from(price.prompt_per_1k_mc))?;
    let c = u128::from(completion).checked_mul(u128::from(price.completion_per_1k_mc))?;
    u64::try_from(p.checked_add(c)?.checked_add(999)? / 1000).ok()
}
fn fits(known: u64, held: u64, needed: u64, limit: u64) -> bool {
    u128::from(known) + u128::from(held) + u128::from(needed) <= u128::from(limit)
}
fn bound(price: &DebugPrice) -> io::Result<u64> {
    if price.prompt_bound == 0 || price.output_bound == 0 {
        return Err(rejected("missing_request_bound"));
    }
    cost(price, price.prompt_bound, price.output_bound)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| rejected("price_overflow"))
}
fn sql(value: impl TryInto<i64>) -> io::Result<i64> {
    value
        .try_into()
        .map_err(|_| rejected("integer_out_of_range"))
}
fn money(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn canonical(root: &Path) -> io::Result<String> {
    Ok(root.canonicalize()?.to_string_lossy().into_owned())
}
fn paid_path() -> io::Result<PathBuf> {
    // D08 review: changing CLI HOST must not reset the approved first round.
    // This authority is independent of provider-config and evaluation HOST.
    let home = std::env::var_os("HOME").ok_or_else(|| rejected("owner_home_missing"))?;
    Ok(PathBuf::from(home)
        .join(".config/hexagon/evaluations/task-benefit-evaluation-2026-09-first.db"))
}
pub(crate) fn paid_summary() -> io::Result<BudgetSummary> {
    let path = paid_path()?;
    if !path.exists() {
        return Ok(empty(PAID));
    }
    summary(&Db::open(path).map_err(err)?, PAID)
}
fn empty(scope: &str) -> BudgetSummary {
    BudgetSummary {
        evidence_kind: scope.into(),
        limit_mc: TOTAL,
        pilot_limit_mc: PILOT,
        known_mc: 0,
        reserved_mc: 0,
        unknown_mc: 0,
        in_flight_mc: 0,
        available_mc: TOTAL,
        pilot_exposure_mc: 0,
        requests: 0,
        confirmed_requests: 0,
        blocked: false,
        runs: vec![],
    }
}

pub(crate) fn enable_debug(db: &Db, plan_id: &str, price: &DebugPrice) -> io::Result<()> {
    bound(price)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let existing: Option<String> = db
        .conn()
        .query_row(
            "SELECT price_json FROM evaluation_budget_plans WHERE plan_id=?1",
            [plan_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(json) = existing {
        if serde_json::from_str::<DebugPrice>(&json)? != *price {
            return Err(rejected("debug_price_frozen"));
        }
        tx.commit().map_err(err)?;
        return Ok(());
    }
    let plan = plan::read(db, plan_id)?;
    if plan
        .entries
        .iter()
        .any(|e| e.state != plan::PlannedState::Planned)
    {
        return Err(rejected("budget_must_precede_execution"));
    }
    let batch = config::read(db, &plan.batch_id)?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_budget_rounds(id,total_mc,pilot_mc) VALUES (?1,?2,?3)",params![DEBUG,sql(batch.request.limits.total_mc)?,sql(batch.request.limits.pilot_mc)?]).map_err(err)?;
    db.conn()
        .execute(
            "INSERT INTO evaluation_budget_plans(plan_id,price_json) VALUES (?1,?2)",
            params![plan_id, serde_json::to_string(price)?],
        )
        .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(())
}

pub(crate) fn before_claim(db: &Db, root: &Path, plan_id: &str) -> io::Result<()> {
    // D08: caller owns the same IMMEDIATE transaction as plan claim.
    if db.conn().is_autocommit() {
        return Err(rejected("reservation_requires_claim_transaction"));
    }
    let price: Option<String> = db
        .conn()
        .query_row(
            "SELECT price_json FROM evaluation_budget_plans WHERE plan_id=?1",
            [plan_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let Some(price) = price else {
        return Ok(());
    };
    let plan = plan::read(db, plan_id)?;
    if plan
        .entries
        .iter()
        .any(|e| e.state == plan::PlannedState::Started)
    {
        return Err(rejected("run_already_active"));
    }
    let Some(next) = plan
        .entries
        .iter()
        .find(|e| e.state == plan::PlannedState::Planned)
    else {
        return Ok(());
    };
    let batch = config::read(db, &plan.batch_id)?;
    reserve_pair(db, DEBUG, root, &plan, next, &batch.request.limits, &price)
}

fn reserve_pair(
    db: &Db,
    scope: &str,
    root: &Path,
    plan: &plan::EvaluationPlan,
    next: &plan::PlannedRun,
    limits: &config::EvaluationLimits,
    price: &str,
) -> io::Result<()> {
    let host = canonical(root)?;
    let plan_id = &plan.id;
    let pair = config::digest(&(scope, &host, plan_id, &next.task_id, next.repetition))?;
    let exists: bool = db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM evaluation_budget_pairs WHERE id=?1)",
            [&pair],
            |r| r.get(0),
        )
        .map_err(err)?;
    if !exists {
        let report = summary(db, scope)?;
        let needed = limits
            .run_mc
            .checked_mul(2)
            .ok_or_else(|| rejected("pair_allowance_overflow"))?;
        let pilot = plan.kind == plan::PlanKind::Pilot;
        if report.blocked
            || !fits(
                report.known_mc,
                report.reserved_mc,
                needed,
                report.limit_mc.min(limits.total_mc),
            )
            || (pilot
                && !fits(
                    report.pilot_exposure_mc,
                    0,
                    needed,
                    report.pilot_limit_mc.min(limits.pilot_mc),
                ))
        {
            return Err(rejected("pair_budget_unavailable"));
        }
        db.conn().execute("INSERT INTO evaluation_budget_pairs(id,round_id,host,plan_id,pilot,allowance_mc) VALUES (?1,?2,?3,?4,?5,?6)",params![pair,scope,host,plan_id,pilot,sql(needed)?]).map_err(err)?;
        let entries: Vec<_> = plan
            .entries
            .iter()
            .filter(|e| e.task_id == next.task_id && e.repetition == next.repetition)
            .collect();
        if entries.len() != 2 {
            return Err(rejected("pair_is_not_two_arms"));
        }
        for entry in entries {
            let key = config::digest(&(&pair, entry.position))?;
            db.conn().execute("INSERT INTO evaluation_budget_runs(id,pair_id,position,allowance_mc,request_limit,price_json) VALUES (?1,?2,?3,?4,?5,?6)",params![key,pair,sql(entry.position)?,sql(limits.run_mc)?,sql(limits.requests)?,price]).map_err(err)?;
        }
    }
    Ok(())
}

pub(crate) fn bind_debug(db: &Db, host: &Path, workspace: &Path, run_id: &str) -> io::Result<()> {
    let key:Option<String>=db.conn().query_row("SELECT b.id FROM evaluation_budget_runs b JOIN evaluation_budget_pairs p ON p.id=b.pair_id JOIN evaluation_plan_runs r ON r.plan_id=p.plan_id AND r.position=b.position WHERE r.run_id=?1",[run_id],|r|r.get(0)).optional().map_err(err)?;
    let Some(key) = key else {
        let required: bool = db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_plan_runs r JOIN evaluation_budget_plans b ON b.plan_id=r.plan_id WHERE r.run_id=?1)", [run_id], |r| r.get(0)).map_err(err)?;
        return if required {
            Err(rejected("reserved_worker_missing"))
        } else {
            Ok(())
        };
    };
    let worker = canonical(workspace)?;
    let changed=db.conn().execute("UPDATE evaluation_budget_runs SET run_id=?2,workspace=?3 WHERE id=?1 AND run_id IS NULL AND closed=0",params![key,run_id,worker]).map_err(err)?;
    if changed != 1 {
        return Err(rejected("worker_already_bound"));
    }
    std::fs::create_dir_all(workspace.join(".hexagon"))?;
    std::fs::write(
        workspace.join(".hexagon/evaluation-budget.json"),
        serde_json::to_vec(&Binding {
            host: canonical(host)?,
            run_key: key,
            scope: DEBUG.into(),
        })?,
    )?;
    Ok(())
}
pub(crate) fn finish(db: &Db, run: &EvaluationResult) -> io::Result<()> {
    // D08 / 2026-09-27 CLI regression: macOS /var and /private/var name
    // the same copy. Comparing the display path left terminal allowances held.
    let workspace = canonical(Path::new(&run.workspace))?;
    db.conn()
        .execute(
            "UPDATE evaluation_budget_runs SET closed=1 WHERE run_id=?1 AND workspace=?2",
            params![run.id, workspace],
        )
        .map_err(err)?;
    Ok(())
}

pub(crate) fn summary(db: &Db, scope: &str) -> io::Result<BudgetSummary> {
    let limits: Option<(u64, u64, bool)> = db
        .conn()
        .query_row(
            "SELECT total_mc,pilot_mc,blocked FROM evaluation_budget_rounds WHERE id=?1",
            [scope],
            |r| Ok((money(r, 0)?, money(r, 1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?;
    let Some((total, pilot, blocked)) = limits else {
        return Ok(empty(scope));
    };
    let mut result = empty(scope);
    result.limit_mc = total;
    result.pilot_limit_mc = pilot;
    result.blocked = blocked;
    let pairs = {
        let mut q=db.conn().prepare("SELECT id,pilot,allowance_mc,host,plan_id FROM evaluation_budget_pairs WHERE round_id=?1 ORDER BY id").map_err(err)?;
        let rows = q
            .query_map([scope], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, bool>(1)?,
                    money(r, 2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        rows
    };
    for (pair, is_pilot, allowance, host, plan_id) in pairs {
        let runs = {
            let mut q=db.conn().prepare("SELECT id,run_id,closed,position,workspace FROM evaluation_budget_runs WHERE pair_id=?1 ORDER BY position").map_err(err)?;
            let rows = q
                .query_map([&pair], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, bool>(2)?,
                        money(r, 3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            rows
        };
        let mut known = 0u64;
        let mut held = 0u64;
        let mut all_closed = runs.len() == 2;
        for (key, run_id, closed, position, workspace) in runs {
            let (n,confirmed,k,u,p):(u64,u64,u64,u64,u64)=db.conn().query_row("SELECT COUNT(*),COALESCE(SUM(confirmed),0),COALESCE(SUM(known_mc),0),COALESCE(SUM(CASE WHEN state='unknown' THEN held_mc ELSE 0 END),0),COALESCE(SUM(CASE WHEN state='pending' THEN held_mc ELSE 0 END),0) FROM evaluation_budget_requests WHERE run_key=?1 AND state!='not_sent'",[&key],|r|Ok((money(r,0)?,money(r,1)?,money(r,2)?,money(r,3)?,money(r,4)?))).map_err(err)?;
            known = known
                .checked_add(k)
                .ok_or_else(|| rejected("ledger_overflow"))?;
            held = held
                .checked_add(u)
                .and_then(|v| v.checked_add(p))
                .ok_or_else(|| rejected("ledger_overflow"))?;
            all_closed &= closed;
            result.unknown_mc = result
                .unknown_mc
                .checked_add(u)
                .ok_or_else(|| rejected("ledger_overflow"))?;
            result.in_flight_mc = result
                .in_flight_mc
                .checked_add(p)
                .ok_or_else(|| rejected("ledger_overflow"))?;
            result.requests += n;
            result.confirmed_requests += confirmed;
            result.runs.push(BudgetRunSummary {
                host: host.clone(),
                plan_id: plan_id.clone(),
                position,
                workspace,
                key,
                run_id,
                closed,
                requests: n,
                confirmed_requests: confirmed,
                known_mc: k,
                unknown_mc: u,
                in_flight_mc: p,
            });
        }
        let exposure = known
            .checked_add(held)
            .ok_or_else(|| rejected("ledger_overflow"))?;
        let exposure = if all_closed {
            exposure
        } else {
            exposure.max(allowance)
        };
        result.known_mc = result
            .known_mc
            .checked_add(known)
            .ok_or_else(|| rejected("ledger_overflow"))?;
        result.reserved_mc = result
            .reserved_mc
            .checked_add(exposure - known)
            .ok_or_else(|| rejected("ledger_overflow"))?;
        if is_pilot {
            result.pilot_exposure_mc = result
                .pilot_exposure_mc
                .checked_add(exposure)
                .ok_or_else(|| rejected("ledger_overflow"))?;
        }
    }
    result.available_mc = result
        .limit_mc
        .saturating_sub(result.known_mc)
        .saturating_sub(result.reserved_mc);
    result.blocked |= !fits(result.known_mc, result.reserved_mc, 0, result.limit_mc)
        || result.pilot_exposure_mc > result.pilot_limit_mc;
    Ok(result)
}

struct Settlement {
    known: u64,
    held: u64,
    state: &'static str,
    blocked: Option<&'static str>,
    calculated: Option<u64>,
}
fn settlement(
    price: &DebugPrice,
    bound: u64,
    usage: Option<&Usage>,
    not_sent: bool,
    unpriced: bool,
) -> Settlement {
    let calculated = usage.and_then(|u| {
        cost(
            price,
            if u.prompt_reported {
                u.prompt_tokens
            } else {
                0
            },
            if u.completion_reported {
                u.completion_tokens
            } else {
                0
            },
        )
    });
    let overflow = !not_sent && usage.is_some() && calculated.is_none_or(|c| c > i64::MAX as u64);
    let known = if not_sent || overflow {
        0
    } else {
        calculated.unwrap_or(0)
    };
    let extra = unpriced || usage.is_some_and(|u| u.unpriced);
    let complete = not_sent
        || (!overflow
            && !extra
            && usage.is_some_and(|u| u.prompt_reported && u.completion_reported));
    // D08 cost model: false refusal needs reconciliation; false clearance
    // spends unreviewed money. Missing/overflow evidence keeps the bound.
    Settlement {
        known,
        held: if complete {
            0
        } else {
            bound.saturating_sub(known)
        },
        state: if not_sent {
            "not_sent"
        } else if complete {
            "known"
        } else {
            "unknown"
        },
        blocked: if overflow {
            Some("usage_arithmetic_overflow")
        } else if known > bound {
            Some("actual_cost_exceeded_bound")
        } else if extra && !not_sent {
            Some("unpriced_billing_dimension")
        } else {
            None
        },
        calculated,
    }
}

pub(crate) struct RequestGuard {
    db: Db,
    id: String,
    price: DebugPrice,
    dispatched: bool,
    project: String,
    agent: String,
    activation: Option<String>,
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        // An unwound dispatched request keeps its full bound. Before dispatch,
        // this stack frame can prove no network call was made; crash recovery13
        // separately requires proof that no live guard remains.
        let state = if self.dispatched {
            "unknown"
        } else {
            "not_sent"
        };
        let _=self.db.conn().execute("UPDATE evaluation_budget_requests SET state=?2,held_mc=CASE WHEN ?3 THEN held_mc ELSE 0 END WHERE id=?1 AND state='pending'",params![self.id,state,self.dispatched]);
    }
}
impl RequestGuard {
    pub(crate) fn dispatch(&mut self) -> io::Result<()> {
        self.db.conn().execute("UPDATE evaluation_budget_requests SET dispatch_started=1 WHERE id=?1 AND state='pending'",[&self.id]).map_err(err)?;
        self.dispatched = true;
        Ok(())
    }
    pub(crate) fn settle(
        &self,
        usage: Option<&Usage>,
        response: bool,
        not_sent: bool,
        unpriced: bool,
    ) -> io::Result<()> {
        let facts = (
            usage.map(|u| {
                (
                    u.prompt_tokens,
                    u.completion_tokens,
                    u.prompt_reported,
                    u.completion_reported,
                    u.unpriced,
                )
            }),
            response,
            not_sent,
            unpriced,
        );
        let receipt = config::digest(&facts)?;
        let receipt_json = serde_json::to_string(&facts)?;
        let tx = rusqlite::Transaction::new_unchecked(
            self.db.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(err)?;
        let (state, bound, previous): (String, u64, Option<String>) = self
            .db
            .conn()
            .query_row(
                "SELECT state,bound_mc,receipt_digest FROM evaluation_budget_requests WHERE id=?1",
                [&self.id],
                |r| Ok((r.get(0)?, money(r, 1)?, r.get(2)?)),
            )
            .map_err(err)?;
        let decision = settlement(&self.price, bound, usage, not_sent, unpriced);
        let Settlement {
            known,
            held,
            state: new_state,
            blocked,
            calculated,
        } = decision;
        if previous.as_deref() == Some(&receipt) {
            tx.commit().map_err(err)?;
            return Ok(());
        }
        let conflict = state != "pending";
        self.db.conn().execute("INSERT OR IGNORE INTO evaluation_budget_receipts(request_id,digest,receipt_json,cost_text,conflict) VALUES (?1,?2,?3,?4,?5)",params![self.id,receipt,receipt_json,calculated.map(|c|c.to_string()),conflict]).map_err(err)?;
        if conflict {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project),
                Some(&self.agent),
                self.activation.as_deref(),
                None,
                "evaluation_budget_settlement",
                &format!("conflicting_receipt:{}", self.id),
                std::time::Instant::now(),
            );
            // D08: keep both receipts and the largest known lower bound; a
            // contradictory supplier fact cannot resurrect released budget.
            self.db.conn().execute("UPDATE evaluation_budget_requests SET state='unknown',known_mc=MAX(known_mc,?2),held_mc=MAX(held_mc,bound_mc) WHERE id=?1",params![self.id,sql(known)?]).map_err(err)?;
            self.db
                .conn()
                .execute("UPDATE evaluation_budget_rounds SET blocked=1", [])
                .map_err(err)?;
            tx.commit().map_err(err)?;
            return Err(rejected("conflicting_usage_receipt"));
        }
        self.db.conn().execute("UPDATE evaluation_budget_requests SET state=?2,known_mc=?3,held_mc=?4,confirmed=?5,receipt_digest=?6 WHERE id=?1",params![self.id,new_state,sql(known)?,sql(held)?,response || usage.is_some_and(|u|u.prompt_reported || u.completion_reported),receipt]).map_err(err)?;
        if let Some(reason) = blocked {
            crate::diag::note(
                crate::diag::CLASS_REJECT,
                true,
                Some(&self.project),
                Some(&self.agent),
                self.activation.as_deref(),
                None,
                "evaluation_budget_settlement",
                &format!("{reason}:{}", self.id),
                std::time::Instant::now(),
            );
            self.db
                .conn()
                .execute("UPDATE evaluation_budget_rounds SET blocked=1", [])
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(())
    }
}

pub(crate) fn admit(
    ctx: &ToolContext,
    provider: &dyn ModelProvider,
    local_id: &str,
    purpose: &str,
) -> io::Result<Option<RequestGuard>> {
    let marker = ctx.repo_root.join(".hexagon/evaluation-worker");
    if !marker.exists() {
        return Ok(None);
    }
    let path = ctx.repo_root.join(".hexagon/evaluation-budget.json");
    if !path.exists() {
        return if provider.is_scripted() {
            Ok(None)
        } else {
            Err(rejected("evaluation_binding_required"))
        };
    }
    if path.symlink_metadata()?.is_symlink() {
        return Err(rejected("binding_is_symlink"));
    }
    let binding: Binding = serde_json::from_slice(&std::fs::read(path)?)?;
    if binding.scope != DEBUG {
        return Err(rejected("live_evaluation_not_enabled"));
    }
    if !provider.is_scripted() {
        return Err(rejected("real_provider_in_debug_budget"));
    }
    let host = Path::new(&binding.host);
    if canonical(host)? != binding.host {
        return Err(rejected("noncanonical_authority"));
    }
    let db = Db::open(host.join(".hexagon/state.db")).map_err(err)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let (workspace,run_id,allowance,limit,price,closed):(Option<String>,Option<String>,u64,u64,String,bool)=db.conn().query_row("SELECT workspace,run_id,allowance_mc,request_limit,price_json,closed FROM evaluation_budget_runs WHERE id=?1",[&binding.run_key],|r|Ok((r.get(0)?,r.get(1)?,money(r,2)?,money(r,3)?,r.get(4)?,r.get(5)?))).map_err(err)?;
    if workspace.as_deref() != Some(canonical(&ctx.repo_root)?.as_str())
        || run_id.is_none()
        || closed
    {
        return Err(rejected("worker_binding_not_current"));
    }
    let price: DebugPrice = serde_json::from_str(&price)?;
    let needed = bound(&price)?;
    let (n,known,held):(u64,u64,u64)=db.conn().query_row("SELECT COUNT(*),COALESCE(SUM(known_mc),0),COALESCE(SUM(held_mc),0) FROM evaluation_budget_requests WHERE run_key=?1 AND state!='not_sent'",[&binding.run_key],|r|Ok((money(r,0)?,money(r,1)?,money(r,2)?))).map_err(err)?;
    if summary(&db, DEBUG)?.blocked || n >= limit || !fits(known, held, needed, allowance) {
        return Err(rejected("run_budget_unavailable"));
    }
    let id = config::digest(&(&binding.run_key, local_id))?;
    db.conn().execute("INSERT INTO evaluation_budget_requests(id,run_key,local_request_id,purpose,state,bound_mc,held_mc) VALUES (?1,?2,?3,?4,'pending',?5,?5)",params![id,binding.run_key,local_id,purpose,sql(needed)?]).map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(Some(RequestGuard {
        db,
        id,
        price,
        dispatched: false,
        project: ctx.project_id.clone(),
        agent: ctx.agent_id.clone(),
        activation: ctx.stage_run_id.clone(),
    }))
}

/// Isolated authority fixture for the already-approved Workbench test seam.
/// This cannot redirect or initialize the user's real paid authority.
#[cfg(test)]
pub(crate) fn reserve_paid_fixture(
    host: &Db,
    root: &Path,
    authority: &Path,
    plan_id: &str,
    price: &DebugPrice,
) -> io::Result<BudgetSummary> {
    bound(price)?;
    let plan = plan::read(host, plan_id)?;
    let next = plan
        .entries
        .iter()
        .find(|e| e.state == plan::PlannedState::Planned)
        .ok_or_else(|| rejected("fixture_plan_empty"))?;
    let batch = config::read(host, &plan.batch_id)?;
    let db = Db::open(authority).map_err(err)?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    db.conn().execute("INSERT OR IGNORE INTO evaluation_budget_rounds(id,total_mc,pilot_mc) VALUES (?1,?2,?3)",params![PAID,sql(TOTAL)?,sql(PILOT)?]).map_err(err)?;
    reserve_pair(
        &db,
        PAID,
        root,
        &plan,
        next,
        &batch.request.limits,
        &serde_json::to_string(price)?,
    )?;
    let result = summary(&db, PAID)?;
    tx.commit().map_err(err)?;
    Ok(result)
}

#[cfg(test)]
pub(crate) fn redeliver_fixture(root: &Path, run_key: &str, usage: &Usage) -> io::Result<()> {
    let db = Db::open(root.join(".hexagon/state.db")).map_err(err)?;
    let (id,price):(String,String)=db.conn().query_row("SELECT q.id,r.price_json FROM evaluation_budget_requests q JOIN evaluation_budget_runs r ON r.id=q.run_key WHERE r.id=?1 ORDER BY q.rowid LIMIT 1",[run_key],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
    let guard = RequestGuard {
        db,
        id,
        price: serde_json::from_str(&price)?,
        dispatched: true,
        project: crate::PROJECT_ID.into(),
        agent: "fixture".into(),
        activation: None,
    };
    guard.settle(Some(usage), true, false, false)
}

pub(crate) fn stop_reason(root: &Path) -> io::Result<Option<&'static str>> {
    let path = root.join(".hexagon/evaluation-budget.json");
    if !path.exists() {
        return Ok(None);
    }
    if path.symlink_metadata()?.is_symlink() {
        return Err(rejected("binding_is_symlink"));
    }
    let binding: Binding = serde_json::from_slice(&std::fs::read(path)?)?;
    let db = Db::open_current(Path::new(&binding.host).join(".hexagon/state.db")).map_err(err)?;
    let (limit, allowance, closed): (u64, u64, bool) = db
        .conn()
        .query_row(
            "SELECT request_limit,allowance_mc,closed FROM evaluation_budget_runs WHERE id=?1",
            [&binding.run_key],
            |r| Ok((money(r, 0)?, money(r, 1)?, r.get(2)?)),
        )
        .map_err(err)?;
    // D09: a reservation already owns admission, but is not yet a dispatch.
    // Counting it here used to cancel the legal last request before send.
    // admit still counts ALL reservations atomically to forbid request 81.
    let (count,exposure):(u64,u64)=db.conn().query_row("SELECT COUNT(*),COALESCE(SUM(known_mc+held_mc),0) FROM evaluation_budget_requests WHERE run_key=?1 AND state!='not_sent' AND dispatch_started=1",[&binding.run_key],|r|Ok((money(r,0)?,money(r,1)?))).map_err(err)?;
    Ok(stop_at_boundary(
        closed,
        summary(&db, &binding.scope)?.blocked,
        count,
        limit,
        exposure,
        allowance,
    ))
}

// False positives defer work; false negatives admit unreviewed budget excess.
fn stop_at_boundary(
    closed: bool,
    blocked: bool,
    count: u64,
    limit: u64,
    exposure: u64,
    allowance: u64,
) -> Option<&'static str> {
    if closed {
        Some("budget_run_closed")
    } else if blocked {
        Some("shared_budget_blocked")
    } else if count >= limit {
        Some("request_limit")
    } else if exposure >= allowance {
        Some("run_budget_limit")
    } else {
        None
    }
}

/// D10: a dead execution owner never proves an admitted request was not sent.
/// Recover only the pending classification, retaining its full conservative hold.
// D10 / ticket 13: deleting a required binding must not masquerade as an
// unbudgeted run and strand pending reservations outside reconciliation.
pub(crate) fn validate_recovery_binding(
    db: &Db,
    host: &Path,
    run: &EvaluationResult,
) -> io::Result<()> {
    let stored: Option<(String, String)> = db
        .conn()
        .query_row(
            "SELECT id,workspace FROM evaluation_budget_runs WHERE run_id=?1",
            [&run.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    let path = Path::new(&run.workspace).join(".hexagon/evaluation-budget.json");
    match stored {
        Some((key, workspace)) => {
            if path.symlink_metadata()?.is_symlink() {
                return Err(err("budget binding is an alias"));
            }
            let binding: Binding = serde_json::from_slice(&std::fs::read(path)?)?;
            if binding.scope != DEBUG
                || binding.host != canonical(host)?
                || binding.run_key != key
                || workspace != canonical(Path::new(&run.workspace))?
            {
                return Err(err("budget recovery identity mismatch"));
            }
        }
        None => {
            let required:bool=db.conn().query_row("SELECT EXISTS(SELECT 1 FROM evaluation_plan_runs r JOIN evaluation_budget_plans b ON b.plan_id=r.plan_id WHERE r.run_id=?1)",[&run.id],|r|r.get(0)).map_err(err)?;
            if required || path.symlink_metadata().is_ok() {
                return Err(err("budget recovery association missing"));
            }
        }
    }
    Ok(())
}

pub(crate) fn recover_abandoned(db: &Db, host: &Path, workspace: &Path) -> io::Result<()> {
    let path = workspace.join(".hexagon/evaluation-budget.json");
    if !path.exists() {
        return Ok(());
    }
    if path.symlink_metadata()?.is_symlink() {
        return Err(rejected("binding_is_symlink"));
    }
    let binding: Binding = serde_json::from_slice(&std::fs::read(path)?)?;
    if binding.scope != DEBUG || binding.host != canonical(host)? {
        return Err(rejected("unverified_recovery_authority"));
    }
    let stored: String = db
        .conn()
        .query_row(
            "SELECT workspace FROM evaluation_budget_runs WHERE id=?1",
            [&binding.run_key],
            |r| r.get(0),
        )
        .map_err(err)?;
    if stored != canonical(workspace)? {
        return Err(rejected("recovery_workspace_mismatch"));
    }
    db.conn().execute("UPDATE evaluation_budget_requests SET state='unknown',held_mc=MAX(bound_mc,held_mc) WHERE run_key=?1 AND state='pending'",[&binding.run_key]).map_err(err)?;
    db.conn().execute("UPDATE evaluation_budget_rounds SET blocked=1 WHERE id=?1 AND EXISTS(SELECT 1 FROM evaluation_budget_requests WHERE run_key=?2 AND state='unknown')",params![binding.scope,binding.run_key]).map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn stopped_boundary_cannot_reopen_by_increasing_work(count in any::<u64>(),limit in any::<u64>(),exposure in any::<u64>(),allowance in any::<u64>(),extra in any::<u64>()) {
            if stop_at_boundary(false,false,count,limit,exposure,allowance).is_some() {
                prop_assert!(stop_at_boundary(false,false,count.saturating_add(extra),limit,exposure.saturating_add(extra),allowance).is_some());
            }
            prop_assert!(stop_at_boundary(true,false,count,limit,exposure,allowance).is_some());
            prop_assert!(stop_at_boundary(false,true,count,limit,exposure,allowance).is_some());
        }

        #[test]
        fn reservations_never_wrap_into_available_money(known in any::<u64>(), held in any::<u64>(), needed in any::<u64>(), limit in any::<u64>()) {
            if fits(known,held,needed,limit) {
                prop_assert!(fits(known,held,0,limit));
                prop_assert!(fits(0,held,needed,limit));
                prop_assert!(fits(known,0,needed,limit));
                prop_assert!(fits(known,held,needed,u64::MAX));
            }
            if known>limit || held>limit || needed>limit {prop_assert!(!fits(known,held,needed,limit));}
        }
        #[test]
        fn missing_counts_never_release_unaccounted_exposure(bound in 1u64..500001, known_tokens in 0u64..500001) {
            let price=DebugPrice{prompt_per_1k_mc:1000,completion_per_1k_mc:1000,prompt_bound:1,output_bound:1};
            let usage=Usage{prompt_tokens:known_tokens,prompt_reported:true,..Default::default()};
            let outcome=settlement(&price,bound,Some(&usage),false,false);
            prop_assert_eq!(outcome.state,"unknown");
            prop_assert!(outcome.known+outcome.held>=bound);
            prop_assert_eq!(outcome.known,known_tokens);
            if known_tokens>bound { prop_assert!(outcome.blocked.is_some()); }
            let unsent=settlement(&price,bound,None,true,false);
            prop_assert_eq!((unsent.known,unsent.held),(0,0));
            prop_assert_eq!(unsent.state,"not_sent");
        }
        #[test]
        fn fractional_costs_round_up_without_losing_money(tokens in 1u64..1000000, rate in 1u64..1000000) {
            let price=DebugPrice{prompt_per_1k_mc:rate,completion_per_1k_mc:0,prompt_bound:tokens,output_bound:1};
            let charged=cost(&price,tokens,0).unwrap();
            prop_assert!(u128::from(charged)*1000>=u128::from(tokens)*u128::from(rate));
            prop_assert!(u128::from(charged)*1000<u128::from(tokens)*u128::from(rate)+1000);
        }
    }
}
