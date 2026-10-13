-- 0060: 工作树确认卡（SessionHost Wayfinder-07 补充 A，2026-10-12 负责人裁决）。
-- CHECK 改不了，标准重建；列集与 0022 相同，只在 kind 词表里加 worktree。
-- 外键由 db.migrate() 在迁移期关闭。worktree 卡不挂 Agent（agent_id 为 NULL），
-- 唯一索引对 NULL 不去重，重复请求由 cards::queued_by_key 在入队前收敛。

CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','publish','recovery','install','grant','stall','worktree')),
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

-- 物理 create/cleanup 的执行意图：git 调用前写入，成功清除；重开时仍为
-- executing 即 worktree_outcome_unknown，只能只读核对收尾。
ALTER TABLE session_hosts ADD COLUMN worktree_intent_json TEXT;
