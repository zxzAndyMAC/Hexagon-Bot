-- 0003: 产物版本内容入库——此前只有当前版落盘 .hexagon/<path>，旧版本内容被覆盖无法 diff。
-- 老行 content 为 NULL：artifact_content 回退读盘；新交付起每版内容随行存。
ALTER TABLE artifacts ADD COLUMN content TEXT;
