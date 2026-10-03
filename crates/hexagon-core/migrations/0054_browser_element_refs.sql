-- Owner-selected browser context remains typed and separate from message body.
-- Pixel bytes stay in the existing clearable private screenshot store.
ALTER TABLE messages ADD COLUMN element_refs TEXT NOT NULL DEFAULT '[]';
