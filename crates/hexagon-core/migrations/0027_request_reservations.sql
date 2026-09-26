-- D06 / reliability 13: local estimates are separate from settled charges.
ALTER TABLE usage ADD COLUMN reserved_mc INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage ADD COLUMN estimated_prompt_tokens INTEGER;
ALTER TABLE usage ADD COLUMN output_limit INTEGER;
