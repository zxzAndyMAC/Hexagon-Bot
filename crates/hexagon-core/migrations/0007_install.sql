-- 0007: 安装助手——pending_questions 加 install 卡种（票 36）。
-- 同 0006：CHECK 改不了，标准重建；外键由 db.migrate() 在迁移期关闭。

CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','publish','recovery','install')),
    payload     TEXT NOT NULL DEFAULT '{}',
    state       TEXT NOT NULL DEFAULT 'queued'
                CHECK (state IN ('queued','answered','expired')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    answered_at TEXT
);
INSERT INTO pending_questions_new SELECT * FROM pending_questions;
DROP TABLE pending_questions;
ALTER TABLE pending_questions_new RENAME TO pending_questions;
