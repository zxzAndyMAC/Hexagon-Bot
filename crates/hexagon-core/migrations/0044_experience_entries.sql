-- Governance 03: file contents do not grant their own activation authority.
CREATE TABLE experience_entries (
    project_id TEXT NOT NULL REFERENCES projects(id),
    skill TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active','revoked','superseded')),
    record_json TEXT NOT NULL,
    invalidated INTEGER NOT NULL DEFAULT 0,
    proposal_id TEXT NOT NULL REFERENCES proposals(id),
    PRIMARY KEY(project_id,skill,entry_id)
);
CREATE TABLE experience_operations (
    proposal_id TEXT PRIMARY KEY REFERENCES proposals(id),
    project_id TEXT NOT NULL REFERENCES projects(id),
    author TEXT NOT NULL REFERENCES agents(id),
    state TEXT NOT NULL CHECK(state IN ('pending','complete','conflict')),
    intent_json TEXT NOT NULL
);
