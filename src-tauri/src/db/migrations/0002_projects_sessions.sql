-- Projects are concrete initiatives inside a workspace.
CREATE TABLE projects (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    mode          TEXT NOT NULL DEFAULT 'research',
    settings_json TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX idx_projects_workspace ON projects(workspace_id);

-- Sessions are individual units of work: chat, document task, coding run,
-- review task, or terminal task.
CREATE TABLE sessions (
    id                TEXT PRIMARY KEY NOT NULL,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id        TEXT REFERENCES projects(id) ON DELETE SET NULL,
    title             TEXT NOT NULL,
    mode              TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'todo',
    summary           TEXT,
    permissions_json  TEXT NOT NULL DEFAULT '{}',
    context_refs_json TEXT NOT NULL DEFAULT '[]',
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE INDEX idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX idx_sessions_project ON sessions(project_id);
CREATE INDEX idx_sessions_workspace_status ON sessions(workspace_id, status);
