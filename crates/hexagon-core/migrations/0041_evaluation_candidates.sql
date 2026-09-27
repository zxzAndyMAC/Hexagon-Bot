-- D14: only the host owns candidate source bindings; no worker-side receipts.
CREATE TABLE evaluation_candidates (
    proposal_id TEXT PRIMARY KEY REFERENCES proposals(id),
    generation_id TEXT NOT NULL REFERENCES evaluation_generation_contexts(id),
    batch_id TEXT NOT NULL UNIQUE REFERENCES evaluation_batches(id),
    plan_id TEXT NOT NULL UNIQUE REFERENCES evaluation_plans(id),
    source_json TEXT NOT NULL CHECK(json_valid(source_json)),
    fingerprint TEXT NOT NULL
);
-- D14: source operations belong to the host; workers cannot create receipts.
CREATE TABLE evaluation_policy_generations (
    id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL UNIQUE REFERENCES evaluation_generation_contexts(id),
    operation_json TEXT NOT NULL CHECK(json_valid(operation_json)),
    fingerprint TEXT NOT NULL,
    output_text TEXT,
    proposal_id TEXT UNIQUE REFERENCES proposals(id)
);
ALTER TABLE evaluation_budget_plans ADD COLUMN scope TEXT NOT NULL DEFAULT 'scripted_debug' CHECK(scope IN ('scripted_debug','paid_first_round'));
CREATE TABLE evaluation_preflights (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES evaluation_batches(id),
    receipt_json TEXT NOT NULL CHECK(json_valid(receipt_json)),
    fingerprint TEXT NOT NULL
);
-- Comparisons reserve two original arms; a candidate/probe/generation reserves one.
-- Keep the expected count frozen so deleting a row cannot release missing work.
ALTER TABLE evaluation_budget_pairs ADD COLUMN expected_runs INTEGER NOT NULL DEFAULT 2 CHECK(expected_runs IN (1,2));
-- D10: model preflight/generation also own real execution leases. They are not
-- task attempts and must never add rows to heldout coverage.
CREATE TABLE evaluation_leases_v2 (
 kind TEXT NOT NULL CHECK(kind IN ('driver','attention','activity')),
 owner_id TEXT NOT NULL, run_id TEXT NOT NULL, file_name TEXT NOT NULL UNIQUE,
 PRIMARY KEY(kind,owner_id)
);
INSERT INTO evaluation_leases_v2 SELECT * FROM evaluation_leases;
DROP TABLE evaluation_leases;
ALTER TABLE evaluation_leases_v2 RENAME TO evaluation_leases;
