-- 0019: 角色定义可以进改进提案（ADR 0069）。CHECK 改不了，标准重建。
-- 只加 surface 词表里的 role_def。列集与 0001 的 proposals 相同。

CREATE TABLE proposals_new (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    author_agent_id TEXT NOT NULL REFERENCES agents(id),
    surface         TEXT NOT NULL
                    CHECK (surface IN ('skill','pack_copy','agents_md','role_def')),
    status          TEXT NOT NULL DEFAULT 'queued'
                    CHECK (status IN ('queued','in_review','rejected',
                                      'awaiting_stamp','active','rolled_back')),
    artifact_id     TEXT REFERENCES artifacts(id),
    effective_path  TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    decided_at      TEXT
);
INSERT INTO proposals_new
    (id, project_id, author_agent_id, surface, status, artifact_id, effective_path, created_at, decided_at)
    SELECT id, project_id, author_agent_id, surface, status, artifact_id, effective_path, created_at, decided_at
    FROM proposals;
DROP TABLE proposals;
ALTER TABLE proposals_new RENAME TO proposals;
