-- 0018: 现成仓库第一次成为项目时的开场分析（票 17 / ADR 0067）。
-- skip = 空目录（走一句话简报）或本迁移之前就存在的项目（再次打开不重跑）。
-- pending = 非空目录刚建成，进工作台后还没分析。
-- running = 这次分析正在叫模型。崩溃留在 running 也不再重跑：
--   漏一次分析花一次人工，每次打开都再打模型是未审的重复调用。偏向不重跑。
-- done = 分析或「没有接话人」的说明已经写进时间线。
-- intake_draft = 还没落盘的 AGENTS.md 草案。NULL = 没有草案（已有说明，或还没分析）。
ALTER TABLE projects ADD COLUMN opening_intake TEXT NOT NULL DEFAULT 'skip';
ALTER TABLE projects ADD COLUMN intake_draft TEXT;
