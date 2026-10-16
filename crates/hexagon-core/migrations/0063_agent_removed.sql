-- 0063: 「移出团队」软删（ui-redesign gap D2，2026-10-16）。
-- agents 行被事件、消息、产物、待决等十余张表外键引用，轨迹是业务事实，硬删会断历史；
-- 改为记 removed_at：名单 / 点名 / 激活读面一律滤掉，按 id 回看历史仍能取到角色名。
-- 被否决：ON DELETE CASCADE（连轨迹一起删）、移到占位项目（按 project_id 读的历史面会丢）。
ALTER TABLE agents ADD COLUMN removed_at TEXT;
