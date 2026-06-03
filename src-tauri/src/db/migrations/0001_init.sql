-- Workspaces are the top-level environment boundary in UAW.
CREATE TABLE workspaces (
    id            TEXT PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'mixed',
    settings_json TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
