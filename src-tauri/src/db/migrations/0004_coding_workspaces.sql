-- A coding workspace is an isolated git worktree + branch created from an
-- attached repository, where implementation work happens before review.
CREATE TABLE coding_workspaces (
    id                   TEXT PRIMARY KEY NOT NULL,
    workspace_id         TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id           TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repository_source_id TEXT NOT NULL REFERENCES repository_sources(id) ON DELETE CASCADE,
    session_id           TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    repo_path            TEXT NOT NULL,
    worktree_path        TEXT NOT NULL,
    branch_name          TEXT NOT NULL,
    base_branch          TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'worktree-created',
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);

CREATE INDEX idx_coding_workspaces_workspace ON coding_workspaces(workspace_id);
CREATE INDEX idx_coding_workspaces_project ON coding_workspaces(project_id);
CREATE INDEX idx_coding_workspaces_repository ON coding_workspaces(repository_source_id);
