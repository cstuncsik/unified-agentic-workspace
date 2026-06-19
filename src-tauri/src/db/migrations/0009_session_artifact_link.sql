-- Back-link: a coding session can be spawned by dispatching an artifact. SET NULL
-- so deleting the artifact keeps the session.
ALTER TABLE sessions ADD COLUMN created_from_artifact_id TEXT
    REFERENCES artifacts(id) ON DELETE SET NULL;
CREATE INDEX idx_sessions_created_from_artifact ON sessions(created_from_artifact_id);
