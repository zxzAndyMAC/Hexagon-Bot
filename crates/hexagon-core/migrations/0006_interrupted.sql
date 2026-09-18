-- 0006: 崩溃恢复——stage_runs 加 interrupted 态、pending_questions 加 recovery 卡种（票 37）。
-- SQLite 改不了 CHECK，标准重建：new 表 → 拷数据 → drop 旧 → rename。
-- 重建期间外键必须关闭（events/artifacts 引用 stage_runs.id），由 db.migrate() 在事务外包 PRAGMA。

CREATE TABLE stage_runs_new (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL REFERENCES projects(id),
    stage_name   TEXT NOT NULL,
    seq          INTEGER NOT NULL,                    -- 包内序号
    state        TEXT NOT NULL DEFAULT 'pending'
                 CHECK (state IN ('pending','active','done','skipped','waiting_stamp','rejected','interrupted')),
    started_at   TEXT,
    finished_at  TEXT
);
INSERT INTO stage_runs_new SELECT * FROM stage_runs;
DROP TABLE stage_runs;
ALTER TABLE stage_runs_new RENAME TO stage_runs;
CREATE INDEX idx_stage_runs_project ON stage_runs(project_id, seq);

CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','publish','recovery')),
    payload     TEXT NOT NULL DEFAULT '{}',
    state       TEXT NOT NULL DEFAULT 'queued'
                CHECK (state IN ('queued','answered','expired')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    answered_at TEXT
);
INSERT INTO pending_questions_new SELECT * FROM pending_questions;
DROP TABLE pending_questions;
ALTER TABLE pending_questions_new RENAME TO pending_questions;
