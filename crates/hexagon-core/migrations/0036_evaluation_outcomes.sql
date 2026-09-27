ALTER TABLE evaluation_runs ADD COLUMN execution_fingerprint TEXT;
CREATE TABLE evaluation_outcomes (
 id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES evaluation_runs(id),
 observation_json TEXT NOT NULL CHECK(json_valid(observation_json))
);
CREATE TABLE evaluation_readonly_checks (
 id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES evaluation_runs(id),
 acceptance_json TEXT NOT NULL CHECK(json_valid(acceptance_json))
);
