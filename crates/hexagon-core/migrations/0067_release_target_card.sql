-- 0067: 登记发布目标卡（owner-followups-1015 #16，ADR 0093 后续裁决：聊天里要求登记，Agent 只拟稿，
-- 落到「登记发布目标」卡由负责人亲手批准；托管不代批）。新卡种 release_target。
-- CHECK 改不了，标准重建；列集同 0066，原样复制。外键由 db.migrate() 在迁移期关闭。
-- 被否决：复用 release 卡种加 sub——托管可确认 release 卡，同卡种会让托管路由按 sub 分叉。
CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','push','release','release_target','recovery','install','grant','stall','worktree')),
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
