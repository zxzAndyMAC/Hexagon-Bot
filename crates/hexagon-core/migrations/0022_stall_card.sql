-- 0022: 失速卡（stall-watch 票 02 / ADR 0074）。CHECK 改不了，标准重建。
-- 列集与 0017 相同，只在 kind 词表里加 stall。外键由 db.migrate() 在迁移期关闭。

CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','publish','recovery','install','grant','stall')),
    payload     TEXT NOT NULL DEFAULT '{}',
    state       TEXT NOT NULL DEFAULT 'queued'
                CHECK (state IN ('queued','answered','expired')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    answered_at TEXT,
    idem_key    TEXT,
    answered_by TEXT
);
INSERT INTO pending_questions_new
    (id, project_id, agent_id, kind, payload, state, created_at, answered_at, idem_key, answered_by)
    SELECT id, project_id, agent_id, kind, payload, state, created_at, answered_at, idem_key, answered_by
    FROM pending_questions;
DROP TABLE pending_questions;
ALTER TABLE pending_questions_new RENAME TO pending_questions;
CREATE UNIQUE INDEX idx_pending_idem ON pending_questions(agent_id, idem_key)
    WHERE idem_key IS NOT NULL AND state = 'queued';
