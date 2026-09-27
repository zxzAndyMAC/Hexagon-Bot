-- D09/D10: execution control is independent of task success and operator attention.
CREATE TABLE evaluation_controls (
 run_id TEXT PRIMARY KEY REFERENCES evaluation_runs(id),
 state TEXT NOT NULL CHECK(state IN ('running','waiting_human','paused','stopping','interrupted','ended')),
 reason TEXT,
 active_limit_ms INTEGER NOT NULL CHECK(active_limit_ms>0 AND active_limit_ms<=1800000),
 phase_started_ms INTEGER,
 active_frozen_ms INTEGER, stopped_at_ms INTEGER, cleanup_ended_ms INTEGER,
 automatic_ms INTEGER NOT NULL DEFAULT 0 CHECK(automatic_ms>=0)
);
