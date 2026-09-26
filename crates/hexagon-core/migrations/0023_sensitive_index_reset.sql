-- reliability 03 / D03: old embeddings were made without the content-read policy.
-- Rebuild only derived search state. Historical messages/events remain facts;
-- this migration does not claim to scrub previous disclosures from history.
DELETE FROM code_chunks;
DELETE FROM code_files;
