-- 0011: 必问卡幂等键 + 裁决人（openworker-borrow 票 11）。
-- idem_key = "{stage_run}:{round}:{index}:{tool}:{fnv64(canonical_input)}"
-- ——回合内位置序在重放中稳定（provider 的 tool_use id 跨进程不复现）；
-- stage_run 折进键值串（表无此列，也不需要——幂等域本来就是它）。
-- 唯一索引只管 queued：中断重放同调用命中既有卡，不重复弹；
-- 已答卡的复用走「沿用裁决」逻辑（tools::idem_reuse 内查）。
ALTER TABLE pending_questions ADD COLUMN idem_key TEXT;
ALTER TABLE pending_questions ADD COLUMN answered_by TEXT;
CREATE UNIQUE INDEX idx_pending_idem ON pending_questions(agent_id, idem_key)
    WHERE idem_key IS NOT NULL AND state = 'queued';
