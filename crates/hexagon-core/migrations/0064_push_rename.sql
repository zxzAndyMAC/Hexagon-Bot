-- 0064: 「远程发布」改名「推送」（ADR 0093，2026-10-17 负责人裁决）。
-- 旧实现本来就是 git push -u <remote> <baseline>；「发布」留给执行已登记发布命令的新功能，
-- 所以旧数据整体改名，不新开卡种并存（并存会让历史里同一件事有两种叫法）。
-- 被否决：只在读面做别名映射——每个按 kind 查询的地方都得记得别名，漏一处就丢历史。
--
-- 1) 卡种 publish → push：CHECK 改不了，标准重建；列集同 0060，复制时映射旧行。
--    外键由 db.migrate() 在迁移期关闭。
CREATE TABLE pending_questions_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id),
    agent_id    TEXT REFERENCES agents(id),
    kind        TEXT NOT NULL
                CHECK (kind IN ('permission','stamp','escalation','push','recovery','install','grant','stall','worktree')),
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
    SELECT id, project_id, agent_id,
           CASE kind WHEN 'publish' THEN 'push' ELSE kind END,
           -- 卡面警告是入队时写进载荷的原文，旧卡跟着改称呼。
           CASE WHEN kind = 'publish' AND json_valid(payload)
                     AND json_extract(payload, '$.warning') = '远程发布不可逆：代码与产物将推送到项目外部'
                THEN json_set(payload, '$.warning', '推送不可逆：代码与产物将推送到项目外部')
                ELSE payload END,
           state, created_at, answered_at, idem_key, answered_by
    FROM pending_questions;
DROP TABLE pending_questions;
ALTER TABLE pending_questions_new RENAME TO pending_questions;
CREATE UNIQUE INDEX idx_pending_idem ON pending_questions(agent_id, idem_key)
    WHERE idem_key IS NOT NULL AND state = 'queued';

-- 2) 轨迹事件 publish_* → push_*（EventKind::Push*）。
UPDATE events SET kind = 'push_requested' WHERE kind = 'publish_requested';
UPDATE events SET kind = 'push_confirmed' WHERE kind = 'publish_confirmed';
UPDATE events SET kind = 'push_rejected'  WHERE kind = 'publish_rejected';
UPDATE events SET kind = 'push_failed'    WHERE kind = 'publish_failed';

-- 3) 托管裁决记录（System 子类 managed_decision）里的卡种 / 动作 / 结果同步改名，
--    托管报告的证据分类与结果判定（report::classify / result_of）按新名读回。
UPDATE events
SET payload = json_set(payload,
        '$.card_kind', 'push',
        '$.action', CASE json_extract(payload, '$.action')
                        WHEN 'publish' THEN 'push'
                        ELSE json_extract(payload, '$.action') END,
        '$.outcome', CASE json_extract(payload, '$.outcome')
                        WHEN 'published' THEN 'pushed'
                        WHEN 'not_published' THEN 'not_pushed'
                        ELSE json_extract(payload, '$.outcome') END)
WHERE kind = 'system'
  AND json_valid(payload)
  AND json_extract(payload, '$.kind') = 'managed_decision'
  AND json_extract(payload, '$.card_kind') = 'publish';

-- 4) 托管预算键 publish:<remote>:<commit> → push:...：改名前后同一笔推送共用一份预算，
--    升级不重置轮数。
UPDATE managed_budgets
SET decision_key = 'push:' || substr(decision_key, length('publish:') + 1)
WHERE decision_key LIKE 'publish:%';

-- 旧凭据 publish/<remote> 在系统钥匙串里，迁移碰不到：由 push::credential 兼容读取。
