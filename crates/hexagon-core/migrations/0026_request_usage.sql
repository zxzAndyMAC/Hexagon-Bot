-- D06 / reliability 12: old rows cannot be retroactively classified as calls.
ALTER TABLE usage ADD COLUMN record_kind TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE usage ADD COLUMN request_id TEXT;
ALTER TABLE usage ADD COLUMN purpose TEXT;
ALTER TABLE usage ADD COLUMN request_state TEXT;
ALTER TABLE usage ADD COLUMN price_basis TEXT;
ALTER TABLE usage ADD COLUMN cost_known INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX usage_request_identity ON usage(request_id) WHERE request_id IS NOT NULL;
ALTER TABLE usage ADD COLUMN actual_model TEXT;
ALTER TABLE usage ADD COLUMN prompt_known INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage ADD COLUMN completion_known INTEGER NOT NULL DEFAULT 0;
