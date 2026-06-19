-- A provider account is an API credential for an LLM provider, scoped to a
-- workspace (no project_id — accounts are workspace-global by design). The secret
-- itself lives in the OS keychain under `keychain_ref`; this row holds only
-- metadata. auth_mode is a forward-compat seam (OAuth is a later follow-up); in
-- this slice it is always 'api-key'.
CREATE TABLE provider_accounts (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,
    auth_mode    TEXT NOT NULL DEFAULT 'api-key',
    display_name TEXT NOT NULL,
    keychain_ref TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE INDEX idx_provider_accounts_workspace ON provider_accounts(workspace_id);
