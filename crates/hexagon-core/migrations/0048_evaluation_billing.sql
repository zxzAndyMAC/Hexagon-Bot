-- 2026-09-28: append a supplier-backed interpretation, never rewrite transport receipts.
CREATE TABLE evaluation_billing_reconciliations (
 request_id TEXT PRIMARY KEY REFERENCES evaluation_budget_requests(id),
 evidence_json TEXT NOT NULL CHECK(json_valid(evidence_json)),
 recorded_at_ms INTEGER NOT NULL,
 accounted_mc INTEGER NOT NULL CHECK(accounted_mc>=0)
);
