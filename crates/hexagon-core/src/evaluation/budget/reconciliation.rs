//! Owner-attested supplier evidence can resolve a legacy usage interpretation.
use super::*;

/// This is an owner assertion after inspecting supplier evidence, not automatic
/// invoice verification. Money is retained in its original currency as evidence;
/// accounting keeps the frozen conservative USD token tariff, never an inferred FX rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplierUsageReconciliation {
    pub request_id: String,
    pub original_receipt_digest: String,
    pub provider_id: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub text_input_output_only: bool,
    pub source_url: String,
    pub source_sha256: String,
    pub supplier_currency: String,
    pub supplier_amount: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingRequest {
    pub request_id: String,
    pub activity_id: Option<String>,
    pub state: String,
    pub receipt_digest: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub known_mc: u64,
    pub held_mc: u64,
    pub reconciliation: Option<SupplierUsageReconciliation>,
}

type UsageFacts = (
    Option<(u64, u64, bool, bool, bool, Option<String>)>,
    bool,
    bool,
    bool,
);

pub(crate) fn requests(root: &Path) -> io::Result<Vec<BillingRequest>> {
    let db = live::paid_db()?;
    let mut q = db.conn().prepare("SELECT q.id,r.run_id,q.state,q.receipt_digest,r.price_json,q.known_mc,q.held_mc,c.evidence_json FROM evaluation_budget_requests q JOIN evaluation_budget_runs r ON r.id=q.run_key JOIN evaluation_budget_pairs p ON p.id=r.pair_id LEFT JOIN evaluation_billing_reconciliations c ON c.request_id=q.id WHERE p.host=?1 AND p.round_id=?2 ORDER BY q.id").map_err(err)?;
    let rows = q
        .query_map(params![canonical(root)?, PAID], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, String>(4)?,
                money(r, 5)?,
                money(r, 6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(err)?;
    rows.map(|row| {
        let (request_id, activity_id, state, receipt_digest, price, known_mc, held_mc, correction) =
            row.map_err(err)?;
        let price: live::LivePrice = serde_json::from_str(&price)?;
        Ok(BillingRequest {
            request_id,
            activity_id,
            state,
            receipt_digest,
            provider_id: price.provider_id,
            model: price.model,
            known_mc,
            held_mc,
            reconciliation: correction.map(|s| serde_json::from_str(&s)).transpose()?,
        })
    })
    .collect()
}

fn valid_evidence(e: &SupplierUsageReconciliation) -> bool {
    let amount = e.supplier_amount.split('.').collect::<Vec<_>>();
    e.text_input_output_only
        && e.source_url.starts_with("https://")
        && e.source_url.len() > 8
        && e.source_sha256.len() == 64
        && e.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        && matches!(e.supplier_currency.as_str(), "CNY" | "USD")
        && (1..=2).contains(&amount.len())
        && amount.iter().all(|part| {
            !part.is_empty() && part.len() <= 18 && part.bytes().all(|b| b.is_ascii_digit())
        })
        && !e.evidence.trim().is_empty()
        && e.evidence.len() <= 65536
}

pub(crate) fn reconcile(
    root: &Path,
    evidence: &SupplierUsageReconciliation,
) -> io::Result<BudgetSummary> {
    let started = std::time::Instant::now();
    if !valid_evidence(evidence) {
        return Err(rejected("supplier_evidence_invalid"));
    }
    let db = live::paid_db()?;
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let row = tx.query_row("SELECT q.state,q.receipt_digest,q.bound_mc,q.known_mc,r.price_json,r.closed,q.confirmed,q.dispatch_started FROM evaluation_budget_requests q JOIN evaluation_budget_runs r ON r.id=q.run_key JOIN evaluation_budget_pairs p ON p.id=r.pair_id WHERE q.id=?1 AND p.host=?2 AND p.round_id=?3", params![evidence.request_id,canonical(root)?,PAID], |r| Ok((r.get::<_,String>(0)?, r.get::<_,Option<String>>(1)?, money(r,2)?,money(r,3)?,r.get::<_,String>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?,r.get::<_,bool>(7)?))).optional().map_err(err)?.ok_or_else(|| rejected("supplier_request_not_found"))?;
    let (state, digest, bound, known, price, closed, confirmed, dispatched) = row;
    if !closed
        || !confirmed
        || !dispatched
        || digest.as_deref() != Some(&evidence.original_receipt_digest)
    {
        return Err(rejected("supplier_request_identity_mismatch"));
    }
    let price: live::LivePrice = serde_json::from_str(&price)?;
    if price.provider_id != evidence.provider_id || price.model != evidence.model {
        return Err(rejected("supplier_model_mismatch"));
    }
    let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM evaluation_budget_receipts WHERE request_id=?1 AND conflict=1)", [&evidence.request_id], |r|r.get(0)).map_err(err)?;
    if conflict {
        return Err(rejected("supplier_conflicting_transport_receipts"));
    }
    let original: String = tx
        .query_row(
            "SELECT receipt_json FROM evaluation_budget_receipts WHERE request_id=?1 AND digest=?2",
            params![evidence.request_id, evidence.original_receipt_digest],
            |r| r.get(0),
        )
        .map_err(err)?;
    let facts: UsageFacts = serde_json::from_str(&original)?;
    if config::digest(&facts)? != evidence.original_receipt_digest {
        return Err(rejected("supplier_receipt_digest_mismatch"));
    }
    let Some((prompt, completion, prompt_reported, completion_reported, unpriced, model)) = facts.0
    else {
        return Err(rejected("supplier_original_usage_missing"));
    };
    // Incident 2026-09-28: legacy parsers marked included cache/reasoning details
    // as extra charges. Only a closed, confirmed, exact-token request may be
    // reinterpreted with owner-attested text-only billing. Rejected alternative:
    // replacing the old receipt or blanket-clearing the round. False negative
    // costs one manual review; false positive permits an unreviewed paid call.
    if !facts.1
        || facts.2
        || !(unpriced || facts.3)
        || !prompt_reported
        || !completion_reported
        || model.as_deref() != Some(&evidence.model)
        || prompt != evidence.prompt_tokens
        || completion != evidence.completion_tokens
    {
        return Err(rejected("supplier_usage_mismatch"));
    }
    let accounted = cost(&price.tariff, prompt, completion)
        .filter(|c| *c <= bound && *c >= known)
        .ok_or_else(|| rejected("supplier_cost_outside_frozen_bound"))?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT evidence_json FROM evaluation_billing_reconciliations WHERE request_id=?1",
            [&evidence.request_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(previous) = previous {
        if serde_json::from_str::<SupplierUsageReconciliation>(&previous)? != *evidence
            || state != "known"
        {
            return Err(rejected("supplier_correction_conflict"));
        }
    } else {
        if state != "unknown" {
            return Err(rejected("supplier_request_not_unknown"));
        }
        tx.execute("INSERT INTO evaluation_billing_reconciliations(request_id,evidence_json,recorded_at_ms,accounted_mc) VALUES (?1,?2,?3,?4)",params![evidence.request_id,serde_json::to_string(evidence)?,sql(super::super::now_ms()?)?,sql(accounted)?]).map_err(err)?;
        tx.execute(
            "UPDATE evaluation_budget_requests SET state='known',known_mc=?2,held_mc=0 WHERE id=?1",
            params![evidence.request_id, sql(accounted)?],
        )
        .map_err(err)?;
    }
    // All currently supported sticky-block causes are request-scoped: unresolved
    // dispatch, conflicting receipts, or over-bound cost. Inspect the ENTIRE paid
    // round, including other hosts. Summary still enforces total/pilot limits.
    let unsafe_round: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM evaluation_budget_requests q JOIN evaluation_budget_runs r ON r.id=q.run_key JOIN evaluation_budget_pairs p ON p.id=r.pair_id WHERE p.round_id=?1 AND (q.state IN ('pending','unknown') OR q.known_mc>q.bound_mc OR EXISTS(SELECT 1 FROM evaluation_budget_receipts c WHERE c.request_id=q.id AND c.conflict=1)))",[PAID],|r|r.get(0)).map_err(err)?;
    tx.execute(
        "UPDATE evaluation_budget_rounds SET blocked=?2 WHERE id=?1",
        params![PAID, unsafe_round],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    crate::diag::note(
        crate::diag::CLASS_HOST,
        false,
        Some(crate::PROJECT_ID),
        None,
        None,
        None,
        "evaluation_supplier_reconciliation",
        "supplier_evidence_recorded",
        started,
    );
    summary(&db, PAID)
}

#[cfg(test)]
pub(crate) fn fixture_blocker(request: &str, kind: Option<&str>) {
    let db = live::paid_db().unwrap();
    db.conn()
        .execute(
            "DELETE FROM evaluation_budget_receipts WHERE request_id='fixture-blocker'",
            [],
        )
        .unwrap();
    db.conn()
        .execute(
            "DELETE FROM evaluation_budget_requests WHERE id='fixture-blocker'",
            [],
        )
        .unwrap();
    if let Some(kind) = kind {
        let state = if matches!(kind, "conflict" | "overbound") {
            "known"
        } else {
            kind
        };
        db.conn().execute("INSERT INTO evaluation_budget_requests(id,run_key,local_request_id,purpose,state,bound_mc,known_mc,held_mc) SELECT 'fixture-blocker',run_key,'fixture-blocker','fixture',?2,1,?3,1 FROM evaluation_budget_requests WHERE id=?1",params![request,state,if kind=="overbound" {2} else {0}]).unwrap();
        if kind == "conflict" {
            db.conn().execute("INSERT INTO evaluation_budget_receipts(request_id,digest,receipt_json,conflict) VALUES ('fixture-blocker','fixture','[]',1)", []).unwrap();
        }
    }
}
