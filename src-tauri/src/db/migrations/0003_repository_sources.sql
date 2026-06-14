-- Repository sources are local git repositories attached to a workspace,
-- the basis for coding sessions and isolated worktrees.
CREATE TABLE repository_sources (
    id             TEXT PRIMARY KEY NOT NULL,
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id     TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name           TEXT NOT NULL,
    local_path     TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    enabled        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX idx_repository_sources_workspace ON repository_sources(workspace_id);
CREATE INDEX idx_repository_sources_project ON repository_sources(project_id);
