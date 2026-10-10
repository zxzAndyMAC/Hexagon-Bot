-- test3/17 (spec 托管委托, Q13–Q21, ADR 0086): project delegation is a
-- separate switch beside the three approval modes. One row per project; an
-- absent or unreadable row means OFF. Generation only grows, so a decision
-- prepared under an older grant can never commit after off/on or a restart.
CREATE TABLE IF NOT EXISTS project_delegations (
  project_id TEXT PRIMARY KEY REFERENCES projects(id),
  delegation_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 1),
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  changed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Correction rounds per business decision. Keyed by a stable business
-- identity, not a card id or error text, so retries, restarts and off/on
-- never reset it.
CREATE TABLE IF NOT EXISTS managed_budgets (
  project_id TEXT NOT NULL,
  decision_key TEXT NOT NULL,
  rounds INTEGER NOT NULL DEFAULT 0 CHECK (rounds >= 0),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (project_id, decision_key)
);
