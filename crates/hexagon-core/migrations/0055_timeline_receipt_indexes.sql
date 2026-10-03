-- Issue 15 (2026-10-02): 100 old action lookups against 50k receipts took
-- 1939ms with only project/kind/id. The expression index removes the repeated
-- forward-history JSON scan; no duplicate durable state or write-side projector.
CREATE INDEX idx_events_action_receipt
ON events(project_id, agent_id, kind, json_extract(payload, '$.action_id'), id)
WHERE json_type(payload, '$.action_id') = 'text';

CREATE INDEX idx_events_tool_stream
ON events(project_id, agent_id, CAST(json_extract(payload, '$.seq') AS TEXT), id)
WHERE kind = 'tool_called';
