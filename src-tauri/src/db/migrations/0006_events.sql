-- An append-only audit log of notable automation events (e.g. a coding workspace
-- completion). Payload is an opaque JSON blob describing the event.
CREATE TABLE events (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    type         TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    created_at   TEXT NOT NULL
);

CREATE INDEX idx_events_workspace ON events(workspace_id);
