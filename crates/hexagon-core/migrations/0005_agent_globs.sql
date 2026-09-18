-- 0005: 路径归属入库——RoleDef.globs 在建项目时写入，ctx_for 加载进 owned_globs。
-- 无 globs 行的 Agent 维持原行为（空集 = 不做归属检查）；有行则越权写要问负责人。
CREATE TABLE agent_globs (
    agent_id TEXT NOT NULL REFERENCES agents(id),
    glob     TEXT NOT NULL,
    UNIQUE (agent_id, glob)
);
CREATE INDEX idx_agent_globs ON agent_globs(agent_id);
