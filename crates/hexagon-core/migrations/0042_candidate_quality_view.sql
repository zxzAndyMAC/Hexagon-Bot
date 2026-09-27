-- D14/18: last quality assessment is a read-model hint, never adoption authority.
-- The owner adoption boundary re-reads original evidence under its write guard.
ALTER TABLE evaluation_candidates ADD COLUMN assessment_json TEXT CHECK(assessment_json IS NULL OR json_valid(assessment_json));
ALTER TABLE evaluation_candidates ADD COLUMN assessment_fingerprint TEXT;
