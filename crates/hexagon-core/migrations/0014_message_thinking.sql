-- 0014: 消息上的思考文本（hands-free 票 06）。
-- 模型给出的推理原文。NULL = 没给，时间线不渲染思考行，也不编造。
-- 不进下一轮请求：turn 在入史前剥掉 ContentBlock::Thinking。
ALTER TABLE messages ADD COLUMN thinking TEXT;
