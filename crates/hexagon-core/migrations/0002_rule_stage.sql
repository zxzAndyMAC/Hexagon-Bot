-- 0002: 作用域=activation 的规则绑定到授予时的 stage_run
ALTER TABLE permission_rules ADD COLUMN stage_run_id TEXT;
