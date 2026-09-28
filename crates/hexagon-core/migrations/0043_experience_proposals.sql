-- Governance 02: host-owned submission bindings, never model-authored receipts.
CREATE TABLE experience_proposals (
    project_id TEXT NOT NULL REFERENCES projects(id),
    proposal_id TEXT PRIMARY KEY REFERENCES proposals(id),
    payload_json TEXT NOT NULL,
    artifact_digest TEXT NOT NULL
);
