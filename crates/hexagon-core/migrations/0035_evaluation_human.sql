CREATE TABLE evaluation_human_intervals (
 id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES evaluation_runs(id),
 actor TEXT NOT NULL CHECK(actor IN ('human','scripted')), started_at_ms INTEGER NOT NULL,
 ended_at_ms INTEGER, duration_ms INTEGER, end_reason TEXT,
 fingerprint_before TEXT NOT NULL, fingerprint_after TEXT, guidance_count INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX evaluation_one_owner_attention ON evaluation_human_intervals((1)) WHERE ended_at_ms IS NULL;
CREATE TABLE evaluation_cursors (
 run_id TEXT PRIMARY KEY REFERENCES evaluation_runs(id), cursor_json TEXT NOT NULL CHECK(json_valid(cursor_json)),
 started_at_ms INTEGER NOT NULL, ended_at_ms INTEGER, waiting_since_ms INTEGER,
 waiting_ms INTEGER NOT NULL DEFAULT 0, wait_fingerprint TEXT, unmeasured_changes INTEGER NOT NULL DEFAULT 0
);
