-- 2026-10-01 ticket 06: old remembered command shapes authorize offline foreground only.
ALTER TABLE permission_rules ADD COLUMN network_allowed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE permission_rules ADD COLUMN background_allowed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE permission_rules ADD COLUMN session_name TEXT;
