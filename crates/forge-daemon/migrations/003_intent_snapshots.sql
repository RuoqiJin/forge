-- Git-native intent version index (lightweight metadata only, no lisp text blob)
CREATE TABLE IF NOT EXISTS intent_snapshots (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_name TEXT NOT NULL,
    git_hash    TEXT NOT NULL DEFAULT '',
    summary     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_intent_snapshots_project ON intent_snapshots(project_name, created_at DESC);
