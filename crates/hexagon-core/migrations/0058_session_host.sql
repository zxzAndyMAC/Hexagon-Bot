-- SessionHost P0-1: one durable host identity for the project's current task chain.
CREATE TABLE IF NOT EXISTS session_hosts (
  session_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  project_root TEXT NOT NULL,
  baseline TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('created','running','waiting_user','paused','reconnecting','completed','failed')),
  current_turn_id TEXT,
  worktree_json TEXT,
  runtime_json TEXT,
  reconnect_attempts INTEGER NOT NULL DEFAULT 0 CHECK (reconnect_attempts >= 0),
  reconnect_budget INTEGER NOT NULL DEFAULT 3 CHECK (reconnect_budget >= 0),
  is_current INTEGER NOT NULL DEFAULT 1 CHECK (is_current IN (0,1)),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  last_branch TEXT,
  last_reason_code TEXT,
  last_elapsed_ms INTEGER NOT NULL DEFAULT 0 CHECK (last_elapsed_ms >= 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS session_hosts_one_current
  ON session_hosts(project_id) WHERE is_current = 1;
CREATE INDEX IF NOT EXISTS session_hosts_project_updated
  ON session_hosts(project_id, updated_at DESC);
