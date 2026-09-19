-- 0009: 审查者档位（openworker-borrow 票 05）。shadow=只记录判定；live=allow 免卡执行。
ALTER TABLE projects ADD COLUMN reviewer_mode TEXT NOT NULL DEFAULT 'shadow'
    CHECK (reviewer_mode IN ('shadow', 'live'));
