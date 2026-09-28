use super::evaluation_live_tests::{
    attest_price, fixture_configuration, priced_request, ProbeProvider,
};
use super::*;
use crate::evaluation as eval;

struct LegacyUsage;
impl crate::provider::ModelProvider for LegacyUsage {
    fn billing_model(&self) -> Option<&str> {
        Some("fixture-model-v1")
    }
    fn model_meta(&self) -> crate::provider::ModelMeta {
        ProbeProvider { wrong_model: false }.model_meta()
    }
    fn complete(
        &self,
        request: &crate::provider::ChatRequest,
    ) -> Result<crate::provider::ChatResponse, crate::provider::ProviderError> {
        let mut response = ProbeProvider { wrong_model: false }.complete(request)?;
        response.usage.unpriced = true;
        Ok(response)
    }
}

#[test]
fn supplier_reconciliation_preserves_receipt_and_restores_preflight_admission() {
    let _config = fixture_configuration();
    let home = tempfile::tempdir().unwrap();
    let mut wb = Workbench::open_evaluation_host(home.path()).unwrap();
    wb.register_provider("default", Arc::new(LegacyUsage));
    let batch = wb.freeze_evaluation(&priced_request(), None).unwrap();
    attest_price(&wb, &batch);
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "failed");
    let before = wb.evaluation_budget().unwrap();
    assert!(before.blocked);
    let request = wb.evaluation_billing_requests().unwrap().remove(0);
    let correction = eval::SupplierUsageReconciliation {
        request_id: request.request_id.clone(),
        original_receipt_digest: request.receipt_digest.clone().unwrap(),
        provider_id: "fixture".into(), model: "fixture-model-v1".into(),
        prompt_tokens: 10, completion_tokens: 10,
        text_input_output_only: true,
        source_url: "https://fixture.invalid/usage".into(),
        source_sha256: "a".repeat(64),
        supplier_currency: "CNY".into(), supplier_amount: "0.0001".into(),
        evidence: "One request in the supplier export; matching token totals, only text input and output charged.".into(),
    };
    // 2026-09-28: a supplier attestation must not clear another request or
    // reinterpret different token totals. Exercise the owner API, no paid calls.
    use proptest::prelude::*;
    proptest!(|(mutation in 0u8..10, suffix in "[a-z]{1,12}")| {
        let mut invalid = correction.clone();
        match mutation {
            0 => invalid.request_id.push_str(&suffix),
            1 => invalid.original_receipt_digest.push_str(&suffix),
            2 => invalid.provider_id.push_str(&suffix),
            3 => invalid.model.push_str(&suffix),
            4 => invalid.prompt_tokens += suffix.len() as u64,
            5 => invalid.completion_tokens += suffix.len() as u64,
            6 => invalid.text_input_output_only = false,
            7 => invalid.source_sha256 = suffix,
            8 => invalid.supplier_amount = format!("-{}", suffix.len()),
            _ => invalid.supplier_currency = suffix,
        }
        prop_assert!(wb.reconcile_evaluation_billing(&invalid).is_err());
        let unchanged = wb.evaluation_budget().unwrap();
        prop_assert!(unchanged.blocked);
        prop_assert_eq!(unchanged.unknown_mc, before.unknown_mc);
        prop_assert_eq!(unchanged.known_mc, before.known_mc);
    });
    let other = tempfile::tempdir().unwrap();
    let foreign = Workbench::open_evaluation_host(other.path()).unwrap();
    assert!(foreign.reconcile_evaluation_billing(&correction).is_err());
    let after = wb.reconcile_evaluation_billing(&correction).unwrap();
    assert!(!after.blocked);
    assert_eq!(after.unknown_mc, 0);
    assert_eq!(after.known_mc, before.known_mc);
    assert_eq!(after.requests, 1);
    assert_eq!(
        wb.evaluation_billing_requests().unwrap()[0].receipt_digest,
        request.receipt_digest
    );
    assert_eq!(
        wb.evaluation_billing_requests().unwrap()[0]
            .reconciliation
            .as_ref(),
        Some(&correction)
    );
    wb.reconcile_evaluation_billing(&correction).unwrap();
    let mut changed = correction.clone();
    changed.prompt_tokens += 1;
    assert!(wb.reconcile_evaluation_billing(&changed).is_err());
    // Another unresolved request must keep the entire paid round stopped,
    // including on an idempotent replay of this already-corrected receipt.
    for blocker in ["unknown", "pending", "conflict", "overbound"] {
        eval::budget::reconciliation::fixture_blocker(&correction.request_id, Some(blocker));
        assert!(
            wb.reconcile_evaluation_billing(&correction)
                .unwrap()
                .blocked,
            "{blocker}"
        );
    }
    eval::budget::reconciliation::fixture_blocker(&correction.request_id, None);
    assert!(
        !wb.reconcile_evaluation_billing(&correction)
            .unwrap()
            .blocked
    );
    wb.register_provider("default", Arc::new(ProbeProvider { wrong_model: false }));
    assert_eq!(wb.preflight_evaluation(&batch.id).unwrap().state, "passed");
    assert_eq!(wb.evaluation_budget().unwrap().requests, 3);
}
