-- D11: host receipts and one supplemental plan per original pair.
CREATE TABLE evaluation_service_failures (
 run_id TEXT NOT NULL REFERENCES evaluation_runs(id),
 request_id TEXT NOT NULL,
 status INTEGER NOT NULL CHECK(status BETWEEN 100 AND 599),
 batch_fingerprint TEXT NOT NULL,
 receipt_fingerprint TEXT NOT NULL,
 PRIMARY KEY(run_id,request_id)
);
CREATE TABLE evaluation_supplements (
 original_plan TEXT NOT NULL REFERENCES evaluation_plans(id),
 original_position INTEGER NOT NULL CHECK(original_position>=0),
 supplemental_plan TEXT NOT NULL UNIQUE REFERENCES evaluation_plans(id),
 PRIMARY KEY(original_plan,original_position)
);
ALTER TABLE usage ADD COLUMN http_status INTEGER CHECK(http_status BETWEEN 100 AND 599);
