CREATE TABLE evaluation_generation_contexts (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES evaluation_batches(id),
    context_json TEXT NOT NULL CHECK(json_valid(context_json))
);
CREATE TABLE evaluation_disclosures (
    context_id TEXT NOT NULL REFERENCES evaluation_generation_contexts(id),
    task_key TEXT NOT NULL,
    task_id TEXT NOT NULL,
    batch_id TEXT NOT NULL REFERENCES evaluation_batches(id),
    revealed_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(context_id,task_key)
);
CREATE INDEX evaluation_disclosures_task ON evaluation_disclosures(task_key);
CREATE TABLE evaluation_task_uses (
    run_id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES evaluation_batches(id),
    task_key TEXT NOT NULL,
    task_id TEXT NOT NULL,
    used_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE evaluation_isolation_checks (
    id TEXT PRIMARY KEY,
    report_json TEXT NOT NULL CHECK(json_valid(report_json)),
    checked_at TEXT NOT NULL DEFAULT (datetime('now'))
);
