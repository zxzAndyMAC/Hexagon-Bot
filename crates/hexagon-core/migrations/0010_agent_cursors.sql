-- 0010: per-agent 事件游标（openworker-borrow 票 10）。brief 组装按
-- 「上次读到的位置之后」取增量，休眠唤醒天然不漏不重。
CREATE TABLE agent_cursors (
    agent_id       TEXT PRIMARY KEY REFERENCES agents(id),
    last_event_id  INTEGER NOT NULL DEFAULT 0
);
