-- 0062: 开场草案「先不写」改为软删（gap B10，ui-redesign 2026-10-15）。
-- gap B1 的丢弃直接把 intake_draft 置 NULL，点错就找不回来；现在把正文挪进这一列，
-- 「已丢弃 · 撤销」toast 调 restore_intake_draft 原样挪回。确认写入时一并清空。
ALTER TABLE projects ADD COLUMN intake_draft_discarded TEXT;
