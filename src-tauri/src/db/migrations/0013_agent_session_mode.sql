-- Per-session SDK permission mode ("plan" | "edit"); NULL for PTY sessions, which
-- have no SDK permission concept. Drives the completion review affordance.
ALTER TABLE agent_sessions ADD COLUMN mode TEXT;
