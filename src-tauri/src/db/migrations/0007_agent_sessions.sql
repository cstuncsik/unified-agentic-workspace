-- An agent session is one run of an interactive agent CLI (Claude Code, Codex,
-- Gemini) in a PTY against a coding workspace's worktree. Raw terminal output is
-- streamed to a transcript file referenced here.
CREATE TABLE agent_sessions (
    id                   TEXT PRIMARY KEY NOT NULL,
    workspace_id         TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    coding_workspace_id  TEXT NOT NULL REFERENCES coding_workspaces(id) ON DELETE CASCADE,
    adapter_id           TEXT NOT NULL,
    command              TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'running',
    exit_code            INTEGER,
    transcript_path      TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX idx_agent_sessions_coding_workspace ON agent_sessions(coding_workspace_id);
CREATE INDEX idx_agent_sessions_workspace ON agent_sessions(workspace_id);
