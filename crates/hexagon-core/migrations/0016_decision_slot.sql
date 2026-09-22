-- 0016: 项目经理的可选决策槽（票 08 / ADR 0065）。
-- 编号在 0014_message_thinking 与 0015_autonomy_l4 之后。
-- NULL 或空 = 没配，封闭选择改走 agents.model_slot（主对话模型）。
-- 配了则只接「派给谁 / 先不派活」，聊天回复仍走 model_slot。
-- 不把两个槽合成一列：决策模型不生成聊天文本。
ALTER TABLE agents ADD COLUMN decision_slot TEXT;
