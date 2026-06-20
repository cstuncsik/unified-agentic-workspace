-- Distinguishes interactive PTY sessions ('pty') from headless Claude Agent SDK
-- runs ('sdk') so the frontend picks the right view without re-deriving from the
-- live adapter registry. Existing rows default to 'pty'.
ALTER TABLE agent_sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'pty';
