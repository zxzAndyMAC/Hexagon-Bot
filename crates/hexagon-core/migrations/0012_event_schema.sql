-- 0012: 事件 schema 版本列（aisuite 研究补丁）。payload 的 JSON 形状
-- 会随事件语义演化——迁移只管表结构，管不了 payload 内部格式；
-- 行级版本戳让将来回放/导出能区分「这行是哪个时代写的」。
-- 对齐 aisuite TRACE_SCHEMA_VERSION；此刻加几乎免费，事后补很贵。
ALTER TABLE events ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;
