-- Hexagon-Bot 初始 schema。
-- 实体对照 spec「逻辑表」：team 实体由 agents 集合表达（team_id = project_id），不单列表。

-- 项目：一个本地目录 + 一支勾选团队 + 一条在跑的流程或快速通道
CREATE TABLE projects (
    id                 TEXT PRIMARY KEY,
    dir                TEXT NOT NULL UNIQUE,          -- 本地目录绝对路径
    name               TEXT NOT NULL,
    mode               TEXT NOT NULL CHECK (mode IN ('pack', 'fastpath')),
    pack_name          TEXT,                          -- 流程包名；快速通道为 NULL
    pack_copy_version  INTEGER,                       -- 运行实例钉住的包副本版本
    fastpath_agent_id  TEXT,                          -- 快速通道指定角色
    autonomy           TEXT NOT NULL DEFAULT 'L0'
                       CHECK (autonomy IN ('L0', 'L1', 'L2')),
    usage_limit_cents  INTEGER,                       -- 项目用量上限（分），NULL=不限
    work_branch        TEXT NOT NULL DEFAULT 'hexagon/work',
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Agent：角色在项目中的实例（绑定模型槽/授权/路径归属在独立表）
CREATE TABLE agents (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    role        TEXT NOT NULL,                        -- 预置角色名（CONTEXT.md 十个）
    model_slot  TEXT,                                 -- 模型配置名（凭据按名引用）
    status      TEXT NOT NULL DEFAULT 'sleeping'
                CHECK (status IN ('active', 'sleeping')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, role)
);

-- 授权注册表：Agent → MCP 服务 / 技能包 名单（项目内有效）
CREATE TABLE grants (
    id          TEXT PRIMARY KEY,
    agent_id    TEXT NOT NULL REFERENCES agents(id),
    kind        TEXT NOT NULL CHECK (kind IN ('mcp', 'skill')),
    name        TEXT NOT NULL,                        -- 服务名或技能包名
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (agent_id, kind, name)
);

-- 阶段运行：阶段指针 = 项目当前 state='active' 的行；回拨/重跑产生新行
CREATE TABLE stage_runs (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL REFERENCES projects(id),
    stage_name   TEXT NOT NULL,
    seq          INTEGER NOT NULL,                    -- 包内序号
    state        TEXT NOT NULL DEFAULT 'pending'
                 CHECK (state IN ('pending','active','done','skipped','waiting_stamp','rejected')),
    started_at   TEXT,
    finished_at  TEXT
);
CREATE INDEX idx_stage_runs_project ON stage_runs(project_id, seq);

-- 产物：.hexagon/ 下的可交接对象，元数据头解析入库
CREATE TABLE artifacts (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    path            TEXT NOT NULL,                    -- .hexagon/ 内相对路径
    kind            TEXT NOT NULL,                    -- 规格/接口说明/复审意见/测试记录/改进提案/打回/…
    tier            TEXT NOT NULL                     -- 校验档
                    CHECK (tier IN ('parse', 'skeleton', 'freeform')),
    stage_run_id    TEXT REFERENCES stage_runs(id),
    author_agent_id TEXT REFERENCES agents(id),
    version         INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'valid'
                    CHECK (status IN ('valid','superseded','stamped','pending')),
    upstream_id     TEXT REFERENCES artifacts(id),    -- 上游产物指针
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, path, version)
);
CREATE INDEX idx_artifacts_project ON artifacts(project_id, kind, status);

-- 轨迹事件：只追加不可变；id 自增即严格单调事件序
CREATE TABLE events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   TEXT NOT NULL REFERENCES projects(id),
    stage_run_id TEXT REFERENCES stage_runs(id),
    agent_id     TEXT REFERENCES agents(id),
    kind         TEXT NOT NULL,                       -- 事件类型枚举（见 events.rs）
    payload      TEXT NOT NULL DEFAULT '{}',          -- JSON
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_events_project ON events(project_id, id);
CREATE INDEX idx_events_kind ON events(project_id, kind, id);

-- 负责人/Agent 自由文本消息；@ 点名与 # 路径指针解析成结构化 token
CREATE TABLE messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    author      TEXT NOT NULL,                        -- 'owner' 或 agent_id
    body        TEXT NOT NULL,
    tokens      TEXT NOT NULL DEFAULT '[]',           -- JSON：[{type:'mention'|'path', value}]
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_messages_project ON messages(project_id, id);

-- 形状化权限记忆（区别于 grants 的「授权」）：工具+形+作用域；安全网动作永不入库
CREATE TABLE permission_rules (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    tool        TEXT NOT NULL,
    shape       TEXT NOT NULL,                        -- 命令/路径形，如 'npm install *'
    domain      TEXT,                                 -- 网络规则绑域名
    effect      TEXT NOT NULL CHECK (effect IN ('allow','deny')),
    scope       TEXT NOT NULL CHECK (scope IN ('activation','project')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_perm_rules ON permission_rules(project_id, tool);

-- 用量账本：token 三计（prompt/completion/工具输出）+ 本地估算成本（毫分）
CREATE TABLE usage (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id           TEXT NOT NULL REFERENCES projects(id),
    agent_id             TEXT REFERENCES agents(id),
    model                TEXT,
    prompt_tokens        INTEGER NOT NULL DEFAULT 0,
    completion_tokens    INTEGER NOT NULL DEFAULT 0,
    tool_output_tokens   INTEGER NOT NULL DEFAULT 0,
    cost_millicents      INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_usage_project ON usage(project_id, agent_id);

-- 改进提案（Harness-RSI）：队列态机，非聊天
CREATE TABLE proposals (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    author_agent_id TEXT NOT NULL REFERENCES agents(id),
    surface         TEXT NOT NULL                     -- 生效面白名单
                    CHECK (surface IN ('skill','pack_copy','agents_md')),
    status          TEXT NOT NULL DEFAULT 'queued'
                    CHECK (status IN ('queued','in_review','rejected',
                                      'awaiting_stamp','active','rolled_back')),
    artifact_id     TEXT REFERENCES artifacts(id),
    effective_path  TEXT,                             -- 生效文件（版本化）
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    decided_at      TEXT
);

-- 待答问题：必问/盖章/升级/发布确认的挂起队列（负责人离开=挂起不拒）
CREATE TABLE pending_questions (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','publish')),
    payload     TEXT NOT NULL DEFAULT '{}',
    state       TEXT NOT NULL DEFAULT 'queued'
                CHECK (state IN ('queued','answered','expired')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    answered_at TEXT
);
