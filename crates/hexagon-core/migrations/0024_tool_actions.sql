-- reliability 08 / D05: approval, intent and outcome share a durable identity.
CREATE TABLE tool_actions (
    id TEXT PRIMARY KEY,
    identity TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL REFERENCES projects(id),
    agent_id TEXT NOT NULL,
    stage_run_id TEXT,
    request_id INTEGER,
    tool_call_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    input_json TEXT NOT NULL,
    input_digest TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','authorized','executing','succeeded','failed','denied','unknown')),
    question_id TEXT,
    output_json TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX tool_actions_chain ON tool_actions(project_id,agent_id,state);
