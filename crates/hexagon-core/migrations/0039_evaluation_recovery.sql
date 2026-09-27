-- D10: ownership is an OS lease, never a reusable PID or a timeout guess.
CREATE TABLE evaluation_leases (
 kind TEXT NOT NULL CHECK(kind IN ('driver','attention')),
 owner_id TEXT NOT NULL, run_id TEXT NOT NULL, file_name TEXT NOT NULL UNIQUE,
 PRIMARY KEY(kind,owner_id)
);
CREATE TABLE evaluation_reconciliations (
 run_id TEXT NOT NULL, kind TEXT NOT NULL, observed_at_ms INTEGER NOT NULL,
 PRIMARY KEY(run_id,kind)
);
ALTER TABLE evaluation_controls ADD COLUMN recovery_unknown INTEGER NOT NULL DEFAULT 0 CHECK(recovery_unknown IN (0,1));
