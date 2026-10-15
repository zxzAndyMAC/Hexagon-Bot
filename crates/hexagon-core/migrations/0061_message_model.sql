-- 0061: 消息记录发出时实际用的模型（gap B9，ui-redesign 2026-10-15）。
-- 之前消息头显示的是成员「现在」的模型（TeamRow.model），换模型后旧消息会被错标。
-- 只有 Agent 回复写这一列（provider.billing_model()，物理模型 id）；负责人消息、宿主消息、
-- 迁移前的旧消息为 NULL——界面回退到当前模型并标注「当前」，不冒充当时的模型。
ALTER TABLE messages ADD COLUMN model TEXT;
