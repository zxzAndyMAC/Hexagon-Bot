-- ADR 0079: preserve every legacy scope; only new explicit project choices share.
ALTER TABLE permission_rules ADD COLUMN project_shared INTEGER NOT NULL DEFAULT 0;
