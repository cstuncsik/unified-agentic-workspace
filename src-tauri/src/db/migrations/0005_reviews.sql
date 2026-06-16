-- A review is an immutable snapshot of a coding worktree's diff at one moment,
-- with a deterministic summary, file list, and heuristic risk flags, plus a
-- mutable verdict status. A coding workspace can accumulate many reviews; the
-- configured test command is captured here but executed later (M9).
CREATE TABLE reviews (
    id                  TEXT PRIMARY KEY NOT NULL,
    workspace_id        TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    coding_workspace_id TEXT NOT NULL REFERENCES coding_workspaces(id) ON DELETE CASCADE,
    status              TEXT NOT NULL DEFAULT 'pending',
    summary             TEXT NOT NULL,
    status_short        TEXT NOT NULL,
    diff_stat           TEXT NOT NULL,
    files_json          TEXT NOT NULL DEFAULT '[]',
    test_command        TEXT,
    test_output         TEXT NOT NULL DEFAULT '',
    risk_notes_json     TEXT NOT NULL DEFAULT '[]',
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE INDEX idx_reviews_workspace ON reviews(workspace_id);
CREATE INDEX idx_reviews_coding_workspace ON reviews(coding_workspace_id);
