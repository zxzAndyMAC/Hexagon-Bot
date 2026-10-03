-- Owner 2026-10-01 Q2/Q5: the owner chooses a versioned visual direction.
-- Revisions keep their actual preview bytes/hashes, not mutable file pointers.
CREATE TABLE design_directions (
    project_id TEXT NOT NULL REFERENCES projects(id),
    revision INTEGER NOT NULL,
    question_id TEXT,
    state TEXT NOT NULL CHECK(state IN ('pending','selected','existing')),
    options_json TEXT NOT NULL DEFAULT '[]',
    selected_option TEXT,
    existing_guidance TEXT,
    submitted_by TEXT,
    selected_at TEXT,
    PRIMARY KEY(project_id, revision)
);
