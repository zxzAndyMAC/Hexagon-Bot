-- Reliability 16 / D07: an intent is durable before changing the worktree.
-- Historical snapshots keep their unknown digest; never attest current bytes.
ALTER TABLE artifacts ADD COLUMN content_digest TEXT;
CREATE TABLE artifact_materializations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    path TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','complete','aborted')),
    reason TEXT,
    intent_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX artifact_materializations_pending ON artifact_materializations(project_id,path) WHERE state='pending';
