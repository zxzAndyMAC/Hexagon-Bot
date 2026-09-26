-- Reliability 21: durable owner intent survives replacement/SQLite crash gaps.
CREATE TABLE policy_changes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    proposal_id TEXT NOT NULL REFERENCES proposals(id),
    question_id TEXT,
    operation TEXT NOT NULL CHECK(operation IN ('adopt','rollback')),
    before_content TEXT NOT NULL,
    after_content TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','complete','aborted','conflict')),
    UNIQUE(project_id,proposal_id,operation)
);
