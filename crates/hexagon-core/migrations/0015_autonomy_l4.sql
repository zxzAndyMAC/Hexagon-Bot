-- 0015：自治档位扩到 L0–L4，新行默认 L4（票 01 / ADR 0064）。
-- 编号在 0014_message_thinking 之后，两张迁移不能共用 0014。
-- SQLite 改不了 CHECK 和列默认，标准重建。已有行的 autonomy 原样拷贝——
-- 不把老项目从 L0 刷成 L4。默认只作用于之后省略该列的 INSERT。
-- 重建期间外键必须关闭（其余表引用 projects.id），由 db.migrate() 在事务外包 PRAGMA。

CREATE TABLE projects_new (
    id                 TEXT PRIMARY KEY,
    dir                TEXT NOT NULL UNIQUE,
    name               TEXT NOT NULL,
    mode               TEXT NOT NULL CHECK (mode IN ('pack', 'fastpath')),
    pack_name          TEXT,
    pack_copy_version  INTEGER,
    fastpath_agent_id  TEXT,
    autonomy           TEXT NOT NULL DEFAULT 'L4'
                       CHECK (autonomy IN ('L0', 'L1', 'L2', 'L3', 'L4')),
    usage_limit_cents  INTEGER,
    work_branch        TEXT NOT NULL DEFAULT 'hexagon/work',
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    reviewer_mode      TEXT NOT NULL DEFAULT 'shadow'
                       CHECK (reviewer_mode IN ('shadow', 'live'))
);
INSERT INTO projects_new (
    id, dir, name, mode, pack_name, pack_copy_version, fastpath_agent_id,
    autonomy, usage_limit_cents, work_branch, created_at, reviewer_mode
)
SELECT
    id, dir, name, mode, pack_name, pack_copy_version, fastpath_agent_id,
    autonomy, usage_limit_cents, work_branch, created_at, reviewer_mode
FROM projects;
DROP TABLE projects;
ALTER TABLE projects_new RENAME TO projects;
