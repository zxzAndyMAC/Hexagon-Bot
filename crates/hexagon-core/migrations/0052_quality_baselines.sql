-- Owner autonomy 04, 2026-10-01: source manifests are host state. Keep large
-- snapshots out of timeline payloads; the small event remains their audit key.
CREATE TABLE quality_baselines (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    event_id INTEGER NOT NULL UNIQUE REFERENCES events(id),
    snapshot_json TEXT NOT NULL,
    PRIMARY KEY (project_id, event_id)
);
