-- SessionHost P1-2: durable one-time continuation requests.
CREATE TABLE IF NOT EXISTS session_triggers (
  trigger_id TEXT NOT NULL,
  session_id TEXT NOT NULL REFERENCES session_hosts(session_id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  project_root TEXT NOT NULL,
  baseline TEXT NOT NULL,
  model_slot TEXT NOT NULL,
  permission_package_json TEXT NOT NULL,
  fire_at INTEGER NOT NULL CHECK (fire_at >= 0),
  trace_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('scheduled','expired','consumed','cancelled')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  consumed_at TEXT,
  PRIMARY KEY (session_id, trigger_id)
);
CREATE INDEX IF NOT EXISTS session_triggers_due
  ON session_triggers(project_id, state, fire_at);
CREATE INDEX IF NOT EXISTS session_triggers_session
  ON session_triggers(session_id, created_at DESC);
