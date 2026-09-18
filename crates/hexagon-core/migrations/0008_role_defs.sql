-- 0008: 项目级角色定义（票 30）。
-- 预置角色是全局只读底稿；项目内编辑/自建写本表，解析顺序 = 本表行 → 预置同名。
-- 实例级字段仍在原位：agents.model_slot、agent_globs、grants（改后即时生效）。
CREATE TABLE role_defs (
    project_id TEXT NOT NULL REFERENCES projects(id),
    name       TEXT NOT NULL,                    -- 角色名（与 agents.role 对应）
    duty       TEXT NOT NULL DEFAULT '',         -- 一句话职责
    reviewer   TEXT,                             -- 上级角色名；NULL=直达负责人
    model_slot TEXT NOT NULL DEFAULT 'default',
    skills     TEXT NOT NULL DEFAULT '[]',       -- JSON array：技能名列表
    custom     INTEGER NOT NULL DEFAULT 0,       -- 1=自建自定义角色
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (project_id, name)
);
