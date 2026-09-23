-- 0020：同一角色可以有多个 Agent（ADR 0069）。
-- 经验技能按角色名共用一份，不再被 UNIQUE(project_id, role) 挡成「同一个人再写一次」。
-- SQLite 删不了表级 UNIQUE，标准重建。重建期间外键关闭，由 db.migrate() 包 PRAGMA。

CREATE TABLE agents_new (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id),
    role          TEXT NOT NULL,
    model_slot    TEXT,
    status        TEXT NOT NULL DEFAULT 'sleeping'
                  CHECK (status IN ('active', 'sleeping')),
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    decision_slot TEXT
);
INSERT INTO agents_new (id, project_id, role, model_slot, status, created_at, decision_slot)
SELECT id, project_id, role, model_slot, status, created_at, decision_slot FROM agents;
DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;
CREATE INDEX idx_agents_project_role ON agents(project_id, role);
