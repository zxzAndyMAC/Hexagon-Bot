-- Owner Q12, 2026-10-01: new remembered grants stay with the requesting role
-- instance. NULL legacy rules retain their historical project scope; execution
-- still enforces the current role's ownership and credential guards.
ALTER TABLE permission_rules ADD COLUMN agent_id TEXT REFERENCES agents(id) ON DELETE CASCADE;
