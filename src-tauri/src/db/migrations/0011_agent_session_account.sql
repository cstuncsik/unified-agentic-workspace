-- Bind an agent session to the provider account whose key it runs under. SET NULL
-- so deleting an account preserves session history (the binding just clears).
-- model_id is a forward-compat seam (per-session model picker is a later slice);
-- it is intentionally unconsumed in this milestone.
ALTER TABLE agent_sessions ADD COLUMN account_id TEXT
    REFERENCES provider_accounts(id) ON DELETE SET NULL;
ALTER TABLE agent_sessions ADD COLUMN model_id TEXT;
CREATE INDEX idx_agent_sessions_account ON agent_sessions(account_id);
