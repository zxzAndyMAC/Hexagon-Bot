CREATE TABLE evaluation_plans (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES evaluation_batches(id),
    kind TEXT NOT NULL,
    plan_json TEXT NOT NULL CHECK(json_valid(plan_json)),
    fingerprint TEXT NOT NULL,
    UNIQUE(batch_id,kind)
);
CREATE TABLE evaluation_plan_runs (
    plan_id TEXT NOT NULL REFERENCES evaluation_plans(id),
    position INTEGER NOT NULL CHECK(position>=0),
    state TEXT NOT NULL CHECK(state IN ('planned','started','completed','incomplete','failed','not_run')),
    run_id TEXT UNIQUE,
    reason TEXT,
    PRIMARY KEY(plan_id,position)
);
