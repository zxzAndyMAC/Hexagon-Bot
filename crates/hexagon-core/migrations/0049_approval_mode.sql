-- Owner decision 2026-10-01: access approval is independent of coordination
-- autonomy. Existing projects also start restricted; no silent broad grant.
ALTER TABLE projects ADD COLUMN approval_mode TEXT NOT NULL DEFAULT 'restricted'
    CHECK (approval_mode IN ('restricted', 'assisted', 'broad'));
