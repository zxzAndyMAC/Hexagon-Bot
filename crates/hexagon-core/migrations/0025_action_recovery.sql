-- D05 / reliability 11: immutable guarantee captured before the first effect.
CREATE TABLE action_retry_contracts (
    action_id TEXT PRIMARY KEY REFERENCES tool_actions(id),
    contract_version TEXT NOT NULL,
    stable_key TEXT NOT NULL UNIQUE,
    expires_at_ms INTEGER NOT NULL,
    retry_authorized INTEGER NOT NULL DEFAULT 0
);
-- Owner resolution is distinct from an observed execution outcome.
CREATE TABLE action_resolutions (
    action_id TEXT PRIMARY KEY REFERENCES tool_actions(id),
    decision TEXT NOT NULL CHECK(decision IN ('abandoned','new_attempt')),
    reason TEXT NOT NULL,
    actor TEXT NOT NULL,
    next_action_id TEXT UNIQUE REFERENCES tool_actions(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
