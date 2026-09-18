-- 0004: 用量记到阶段——stage_run_id 随行，支持按阶段分解成本（票 23 验收）。
-- 老行为 NULL：历史账归入「未分阶段」组。
ALTER TABLE usage ADD COLUMN stage_run_id TEXT;
