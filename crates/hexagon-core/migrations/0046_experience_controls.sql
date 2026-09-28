-- Governance 09: host stop takes effect before optional file synchronization.
CREATE TABLE experience_controls (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    skill TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','complete','conflict')),
    reason TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
