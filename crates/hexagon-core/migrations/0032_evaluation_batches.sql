CREATE TABLE evaluation_batches (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES evaluation_batches(id),
    frozen_json TEXT NOT NULL CHECK(json_valid(frozen_json)),
    fingerprint TEXT NOT NULL,
    verification_json TEXT NOT NULL CHECK(json_valid(verification_json))
);
