-- Governance 07: only owner controls project loading budgets.
CREATE TABLE experience_limits (
    project_id TEXT PRIMARY KEY REFERENCES projects(id),
    entry_chars INTEGER NOT NULL CHECK(entry_chars > 0),
    load_count INTEGER NOT NULL CHECK(load_count > 0),
    load_chars INTEGER NOT NULL CHECK(load_chars >= entry_chars + 128)
);
