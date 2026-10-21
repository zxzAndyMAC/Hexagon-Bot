-- 0066: 发布确认卡（owner-followups-1015 #15，ADR 0093：「发布」留给执行已登记发布命令的新功能）。
-- 新卡种 release：负责人对已登记的发布目标发起发布，必问、永不记忆；批准后用登记的命令与凭据执行。
-- CHECK 改不了，标准重建；列集同 0064，原样复制。外键由 db.migrate() 在迁移期关闭。
-- 被否决：复用 push 卡种加 sub——推送可「允许本项目」、发布永不记忆，同卡种会让记住规则按 sub 分叉，
-- 漏判一处就是一次没人看过的发布。
CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','push','release','recovery','install','grant','stall','worktree')),
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
