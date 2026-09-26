-- Reliability 15: null means legacy/unproven, never permission to overwrite.
ALTER TABLE tool_actions ADD COLUMN write_targets_json TEXT;
