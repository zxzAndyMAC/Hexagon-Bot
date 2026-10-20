-- 0065: 发布目标登记（owner-followups-1015 #14，ADR 0093：推送 ≠ 发布）。
-- 项目级：名称、命令行、工作目录（相对项目根，空 = 项目根）、所需凭据名列表（JSON 数组）。
-- 只存凭据「名」，值在钥匙串 `release/<id>/<凭据名>`（credentials.rs 命名约定）；本表进泄漏扫描。
-- id 是随机十六进制而非自增整数：项目 id 是常量（crate::PROJECT_ID），钥匙串全机共享，
-- 自增 id 会让两个项目的 `release/1/NPM_TOKEN` 撞名。被否决：按目标名命名凭据（改名要搬钥匙串条目，
-- 写一半失败会丢唯一一份凭据——同 push::credential 的取舍）。
CREATE TABLE release_targets (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    cwd         TEXT NOT NULL DEFAULT '',
    credentials TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, name)
);
