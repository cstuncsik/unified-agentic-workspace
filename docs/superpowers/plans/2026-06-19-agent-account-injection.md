# Per-session Account Binding + Key Injection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bind an agent terminal to a provider account and inject that account's keychain API key into the session's PTY env, never exposing the key.

**Architecture:** `agent_sessions` gains `account_id`(FK SET NULL)+`model_id`. Adapters gain `provider`/`api_key_env`/`clear_env`. Two testable helpers (`load_session_account` over a `&Connection`, `resolve_session_env` over `&dyn KeyStore`) build the env; `start_agent_session` resolves the key outside the lock and passes it to `pty::spawn`. A picker in AgentsView selects the account; the tab shows it.

**Tech Stack:** Rust + rusqlite + portable-pty; Tauri 2; Vue 3 + Pinia; WebdriverIO.

---

## File Structure

- `src-tauri/src/db/migrations/0011_agent_session_account.sql` (create)
- `src-tauri/src/db/mod.rs` (modify) — register migration 11
- `src-tauri/src/models/agent_session.rs` (modify) — columns, `create()`, `make()`, SET-NULL test
- `src-tauri/src/models/workspace.rs` (modify) — idempotency → 11
- `src-tauri/src/services/agent/mod.rs` (modify) — adapter fields + registry test
- `src-tauri/src/services/agent/pty.rs` (modify) — `env` param + override test
- `src-tauri/src/commands/agent_sessions.rs` (modify) — `load_session_account`, `resolve_session_env`, wire `start_agent_session`
- `src/types/agentSession.ts`, `src/api/agentSessions.ts`, `src/stores/agentSessions.ts`, `src/components/AgentsView.vue` (modify)
- `scripts/run-e2e.sh` (modify) — fake-agent KEY marker
- `e2e/specs/agent-account.e2e.ts` (create)

Run backend tests: `cargo test --manifest-path src-tauri/Cargo.toml`. CI/clippy is Linux (compiles the `OsKeyStore` stub).

---

## Task 1: Migration 0011 + `agent_sessions` columns

**Files:**
- Create: `src-tauri/src/db/migrations/0011_agent_session_account.sql`
- Modify: `src-tauri/src/db/mod.rs` (MIGRATIONS array)
- Modify: `src-tauri/src/models/agent_session.rs`
- Modify: `src-tauri/src/models/workspace.rs:144`

- [ ] **Step 1: Write the migration**

Create `src-tauri/src/db/migrations/0011_agent_session_account.sql`:

```sql
-- Bind an agent session to the provider account whose key it runs under. SET NULL
-- so deleting an account preserves session history (the binding just clears).
-- model_id is a forward-compat seam (per-session model picker is a later slice);
-- it is intentionally unconsumed in this milestone.
ALTER TABLE agent_sessions ADD COLUMN account_id TEXT
    REFERENCES provider_accounts(id) ON DELETE SET NULL;
ALTER TABLE agent_sessions ADD COLUMN model_id TEXT;
CREATE INDEX idx_agent_sessions_account ON agent_sessions(account_id);
```

- [ ] **Step 2: Register migration 11 in `db/mod.rs`**

After the `(10, "provider_accounts", ...)` tuple, add:

```rust
    (
        11,
        "agent_session_account",
        include_str!("migrations/0011_agent_session_account.sql"),
    ),
```

- [ ] **Step 3: Bump idempotency assertion**

In `src-tauri/src/models/workspace.rs` `migrations_are_idempotent`:

```rust
        assert_eq!(version, 11);
```

- [ ] **Step 4: Run idempotency test**

Run: `cargo test --manifest-path src-tauri/Cargo.toml migrations_are_idempotent`
Expected: PASS.

- [ ] **Step 5: Add the columns to the `AgentSession` model**

In `src-tauri/src/models/agent_session.rs`: add to the struct (after `transcript_path`):

```rust
    pub account_id: Option<String>,
    pub model_id: Option<String>,
```

Update `COLUMNS`:

```rust
const COLUMNS: &str = "id, workspace_id, coding_workspace_id, adapter_id, command, status, \
                       exit_code, transcript_path, account_id, model_id, created_at, updated_at";
```

Add to `from_row` (after `transcript_path`):

```rust
        account_id: row.get("account_id")?,
        model_id: row.get("model_id")?,
```

Replace `create` (signature + INSERT):

```rust
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    adapter_id: &str,
    command: &str,
    transcript_path: &str,
    account_id: Option<&str>,
    model_id: Option<&str>,
) -> rusqlite::Result<AgentSession> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO agent_sessions
           (id, workspace_id, coding_workspace_id, adapter_id, command, status,
            exit_code, transcript_path, account_id, model_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', NULL, ?6, ?7, ?8, ?9, ?9)",
        params![
            id, workspace_id, coding_workspace_id, adapter_id, command,
            transcript_path, account_id, model_id, now
        ],
    )?;
    Ok(get(conn, id)?.expect("agent session exists immediately after insert"))
}
```

- [ ] **Step 6: Update the `make()` test helper + add `provider_account` import**

In the `#[cfg(test)] mod tests` `use` line, add `provider_account`:

```rust
    use crate::models::{coding_workspace, project, provider_account, repository, workspace};
```

Update `make`:

```rust
    fn make(conn: &Connection, ws: &str, cw: &str) -> AgentSession {
        create(conn, &new_id(), ws, cw, "claude-code", "claude", "/tmp/t.log", None, None).unwrap()
    }
```

- [ ] **Step 7: Add the SET-NULL test**

Add to the `tests` module:

```rust
    #[test]
    fn deleting_account_nulls_session_binding() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let acct_id = new_id();
        provider_account::insert(&conn, &acct_id, &ws, "anthropic", "api-key", "Key", &acct_id)
            .unwrap();
        let s = create(
            &conn,
            &new_id(),
            &ws,
            &cw,
            "claude-code",
            "claude",
            "/tmp/t.log",
            Some(&acct_id),
            Some("some-model"),
        )
        .unwrap();
        assert_eq!(s.account_id.as_deref(), Some(acct_id.as_str()));
        assert_eq!(s.model_id.as_deref(), Some("some-model"));

        provider_account::delete(&conn, &acct_id).unwrap();
        let after = get(&conn, &s.id).unwrap().unwrap();
        assert_eq!(after.account_id, None); // FK SET NULL fired
        assert!(get(&conn, &s.id).unwrap().is_some()); // session survives
    }
```

- [ ] **Step 8: Run agent_session tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent_session`
Expected: PASS (existing + `deleting_account_nulls_session_binding`).

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/db/migrations/0011_agent_session_account.sql src-tauri/src/db/mod.rs \
        src-tauri/src/models/agent_session.rs src-tauri/src/models/workspace.rs
git commit -m "feat(m10b-2a): agent_sessions account_id/model_id columns + SET NULL"
```

---

## Task 2: Adapter descriptor fields

**Files:**
- Modify: `src-tauri/src/services/agent/mod.rs`

- [ ] **Step 1: Add fields to `AgentAdapter`**

In `src-tauri/src/services/agent/mod.rs`, extend the struct (after `args`):

```rust
#[derive(Debug, Clone, Serialize)]
pub struct AgentAdapter {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: Vec<&'static str>,
    /// Provider these accounts must match (None = no API-key account binding).
    pub provider: Option<&'static str>,
    /// Env var the CLI reads its API key from (None = key injection unsupported).
    pub api_key_env: Option<&'static str>,
    /// Higher-precedence ambient vars to blank when injecting, so a stale ambient
    /// credential can't beat the chosen account's key.
    pub clear_env: Vec<&'static str>,
    pub capabilities: AgentCapabilities,
}
```

- [ ] **Step 2: Populate the three adapters**

Replace the `adapters()` vec bodies:

```rust
pub fn adapters() -> Vec<AgentAdapter> {
    vec![
        AgentAdapter {
            id: "claude-code",
            name: "Claude Code",
            program: "claude",
            args: vec![],
            provider: Some("anthropic"),
            api_key_env: Some("ANTHROPIC_API_KEY"),
            clear_env: vec!["ANTHROPIC_AUTH_TOKEN"],
            capabilities: full_capabilities(),
        },
        AgentAdapter {
            id: "codex",
            name: "Codex",
            program: "codex",
            args: vec![],
            provider: Some("openai"),
            api_key_env: Some("OPENAI_API_KEY"),
            clear_env: vec![],
            capabilities: full_capabilities(),
        },
        AgentAdapter {
            id: "gemini",
            name: "Gemini",
            program: "gemini",
            args: vec![],
            provider: None,
            api_key_env: None,
            clear_env: vec![],
            capabilities: full_capabilities(),
        },
    ]
}
```

- [ ] **Step 3: Extend the registry test**

Replace `registry_has_the_three_clis`'s body tail (keep the existing id assertions, add):

```rust
    #[test]
    fn registry_has_the_three_clis() {
        let ids: Vec<_> = adapters().iter().map(|a| a.id).collect();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"gemini"));
        assert!(find_adapter("claude-code").is_some());
        assert!(find_adapter("nope").is_none());

        let claude = find_adapter("claude-code").unwrap();
        assert_eq!(claude.provider, Some("anthropic"));
        assert_eq!(claude.api_key_env, Some("ANTHROPIC_API_KEY"));
        assert_eq!(claude.clear_env, vec!["ANTHROPIC_AUTH_TOKEN"]);

        let codex = find_adapter("codex").unwrap();
        assert_eq!(codex.provider, Some("openai"));
        assert_eq!(codex.api_key_env, Some("OPENAI_API_KEY"));

        // Gemini has no creatable account in this slice -> no key binding.
        let gemini = find_adapter("gemini").unwrap();
        assert_eq!(gemini.provider, None);
        assert_eq!(gemini.api_key_env, None);
    }
```

- [ ] **Step 4: Run + commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml services::agent`
Expected: PASS.

```bash
git add src-tauri/src/services/agent/mod.rs
git commit -m "feat(m10b-2a): adapter provider/api_key_env/clear_env fields"
```

---

## Task 3: `pty::spawn` env injection

**Files:**
- Modify: `src-tauri/src/services/agent/pty.rs`

- [ ] **Step 1: Add the `env` parameter**

In `src-tauri/src/services/agent/pty.rs`, change `spawn`'s signature and add the env loop after the `TERM` line:

```rust
pub fn spawn(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    cols: u16,
    rows: u16,
) -> Result<Spawned, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("failed to open pty: {e}"))?;

    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    for (key, value) in env {
        cmd.env(key, value);
    }
    // ...unchanged below...
```

- [ ] **Step 1b: Keep the build green — update the one caller to pass `&[]`**

In `src-tauri/src/commands/agent_sessions.rs`, update the existing spawn call (Task 5 will change `&[]` → `&env`):

```rust
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &[], cols, rows)?;
```

- [ ] **Step 2: Update the existing spawn test call + add the override test**

In `pty.rs` tests, update `spawn_runs_a_command_in_a_pty_and_exits`'s call to pass `&[]`:

```rust
        let mut spawned = spawn("sh", &["-c", "printf RUNOK"], &dir, &[], 80, 24)
            .expect("spawn sh in pty");
```

Add the override test:

```rust
    #[test]
    fn spawn_env_overrides_inherited_parent_var() {
        // The child inherits the parent env; an injected var of the same name must
        // win (the security-critical property for key injection).
        std::env::set_var("UAW_SPAWN_PROBE", "PARENT_LEAK");
        let dir = std::env::temp_dir();
        let mut spawned = spawn(
            "sh",
            &["-c", "printf %s \"$UAW_SPAWN_PROBE\""],
            &dir,
            &[("UAW_SPAWN_PROBE".to_string(), "INJECTED".to_string())],
            80,
            24,
        )
        .expect("spawn sh in pty");
        let mut out: Vec<u8> = Vec::new();
        pump(spawned.reader, |chunk| out.extend_from_slice(chunk));
        spawned.child.wait().expect("child waits");
        std::env::remove_var("UAW_SPAWN_PROBE");
        assert_eq!(String::from_utf8_lossy(&out), "INJECTED");
    }
```

- [ ] **Step 3: Run the pty tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::pty`
Expected: PASS (the Step 1b `&[]` caller update keeps the crate compiling), incl. `spawn_env_overrides_inherited_parent_var`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/agent/pty.rs
git commit -m "feat(m10b-2a): pty::spawn env injection + override test"
```

---

## Task 4: `load_session_account` + `resolve_session_env`

**Files:**
- Modify: `src-tauri/src/commands/agent_sessions.rs` (add helpers + tests + imports)

- [ ] **Step 1: Add imports**

At the top of `src-tauri/src/commands/agent_sessions.rs`, add:

```rust
use crate::models::provider_account::{self, ProviderAccount};
use crate::services::agent::AgentAdapter;
use crate::services::keystore::{self, KeyStore};
```

(The file already imports `crate::models::agent_session`, `crate::models::{coding_workspace, event}`, `crate::services::agent::{self, pty}`.)

- [ ] **Step 2: Add the two helpers (module level, before the `#[cfg(test)]`)**

```rust
/// Load and workspace-scope-validate the chosen account. Connection-only — call
/// UNDER the lock. `None` -> no account; an account in another workspace or a
/// missing id -> a fixed opaque error.
pub fn load_session_account(
    conn: &Connection,
    account_id: Option<&str>,
    workspace_id: &str,
) -> Result<Option<ProviderAccount>, String> {
    let Some(account_id) = account_id else {
        return Ok(None);
    };
    match provider_account::get(conn, account_id) {
        Ok(Some(account)) if account.workspace_id == workspace_id => Ok(Some(account)),
        _ => Err("Selected account is not available in this workspace".into()),
    }
}

/// Build the PTY environment for a session. Reads the keychain — call OUTSIDE the
/// connection lock. Every error is a fixed, secret-free string; the key only ever
/// appears as the VALUE of the adapter's api_key_env.
pub fn resolve_session_env(
    adapter: &AgentAdapter,
    account: Option<&ProviderAccount>,
    store: &dyn KeyStore,
) -> Result<Vec<(String, String)>, String> {
    let Some(account) = account else {
        return Ok(Vec::new()); // no account -> inherit ambient env (legacy behavior)
    };
    let Some(api_key_env) = adapter.api_key_env else {
        return Err("This agent does not support API key accounts".into());
    };
    if adapter.provider != Some(account.provider.as_str()) {
        return Err("Selected account does not match this agent's provider".into());
    }
    let key = match store.get(&account.keychain_ref) {
        Ok(Some(key)) => key,
        Ok(None) => return Err("Stored key for this account is missing".into()),
        Err(_) => return Err("Failed to load the account key".into()),
    };
    let mut env = vec![(api_key_env.to_string(), key)];
    for clear in &adapter.clear_env {
        env.push((clear.to_string(), String::new()));
    }
    Ok(env)
}
```

- [ ] **Step 3: Add tests (new `#[cfg(test)]` module at the end of the file)**

```rust
#[cfg(test)]
mod account_env_tests {
    use super::*;
    use crate::models::workspace;
    use crate::services::agent::find_adapter;
    use crate::services::keystore::FileKeyStore;

    const SENTINEL: &str = "SENTINEL_KEY_abc123";

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn temp_store() -> FileKeyStore {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-env-test-{}", new_id()));
        FileKeyStore::new(d)
    }

    fn account(conn: &Connection, ws: &str, provider: &str) -> ProviderAccount {
        let id = new_id();
        provider_account::insert(conn, &id, ws, provider, "api-key", "Acct", &id).unwrap()
    }

    #[test]
    fn no_account_yields_empty_env() {
        let claude = find_adapter("claude-code").unwrap();
        let store = temp_store();
        assert!(resolve_session_env(&claude, None, &store).unwrap().is_empty());
    }

    #[test]
    fn matching_account_injects_key_and_clears_collisions() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        let store = temp_store();
        store.set(&acct.keychain_ref, SENTINEL).unwrap();

        let claude = find_adapter("claude-code").unwrap();
        let env = resolve_session_env(&claude, Some(&acct), &store).unwrap();

        // Key present, ONLY as the value of api_key_env.
        assert!(env
            .iter()
            .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v == SENTINEL));
        assert!(env.iter().all(|(k, _)| k != SENTINEL));
        // Higher-precedence ambient var blanked.
        assert!(env
            .iter()
            .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v.is_empty()));
    }

    #[test]
    fn provider_mismatch_is_rejected_without_leak() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let openai_acct = account(&conn, &ws, "openai");
        let store = temp_store();
        store.set(&openai_acct.keychain_ref, SENTINEL).unwrap();

        let claude = find_adapter("claude-code").unwrap();
        let err = resolve_session_env(&claude, Some(&openai_acct), &store).unwrap_err();
        assert_eq!(err, "Selected account does not match this agent's provider");
        assert!(!err.contains(SENTINEL));
    }

    #[test]
    fn adapter_without_key_env_rejects_account() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        let store = temp_store();
        store.set(&acct.keychain_ref, SENTINEL).unwrap();

        let gemini = find_adapter("gemini").unwrap();
        let err = resolve_session_env(&gemini, Some(&acct), &store).unwrap_err();
        assert_eq!(err, "This agent does not support API key accounts");
        assert!(!err.contains(SENTINEL));
    }

    #[test]
    fn missing_key_fails_closed() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic"); // key never stored
        let store = temp_store();

        let claude = find_adapter("claude-code").unwrap();
        let err = resolve_session_env(&claude, Some(&acct), &store).unwrap_err();
        assert_eq!(err, "Stored key for this account is missing");
    }

    #[test]
    fn load_session_account_scopes_to_workspace() {
        let conn = migrated_conn();
        let ws_a = workspace::create(&conn, "A", "mixed").unwrap().id;
        let ws_b = workspace::create(&conn, "B", "mixed").unwrap().id;
        let acct = account(&conn, &ws_a, "anthropic");

        assert!(load_session_account(&conn, None, &ws_a).unwrap().is_none());
        assert!(load_session_account(&conn, Some(&acct.id), &ws_a)
            .unwrap()
            .is_some());
        // Account belongs to ws_a, not ws_b -> rejected.
        assert!(load_session_account(&conn, Some(&acct.id), &ws_b).is_err());
        // Nonexistent id -> rejected.
        assert!(load_session_account(&conn, Some("nope"), &ws_a).is_err());
    }
}
```

- [ ] **Step 4: Run the helper tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml account_env_tests`
Expected: PASS (6 tests). (The crate compiles because Task 3 Step 1b already updated the spawn call site; the new helpers are unused in non-test code until Task 5 — `cargo test` only warns, doesn't fail.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2a): load_session_account + resolve_session_env helpers"
```

---

## Task 5: Wire `start_agent_session`

**Files:**
- Modify: `src-tauri/src/commands/agent_sessions.rs` (the `start_agent_session` command)

- [ ] **Step 1: Add the `account_id` param**

Change the signature (add `account_id` after `adapter_id`):

```rust
#[tauri::command]
pub fn start_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    adapter_id: String,
    account_id: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<AgentSession, String> {
```

- [ ] **Step 2: Resolve the store + account + env**

Replace the adapter-find + worktree-resolve block with:

```rust
    let Some(adapter) = agent::find_adapter(&adapter_id) else {
        return Err(format!("Unknown agent adapter '{adapter_id}'"));
    };

    let store = keystore::resolve();

    // Resolve the worktree + workspace AND load/validate the chosen account under
    // one lock, then release before any keychain IO or spawn.
    let (workspace_id, worktree_path, account) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        let account = load_session_account(&conn, account_id.as_deref(), &cw.workspace_id)?;
        (cw.workspace_id, cw.worktree_path, account)
    };

    // Resolve the account's key from the keychain (no lock held) and build the env.
    let env = resolve_session_env(&adapter, account.as_ref(), store.as_ref())?;
    let account_row_id = account.as_ref().map(|a| a.id.as_str());
```

- [ ] **Step 3: Pass `env` to spawn**

Change the spawn call:

```rust
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &env, cols, rows)?;
```

- [ ] **Step 4: Record `account_id` on the row + event**

Change the `agent_session::create` call (add the two new args):

```rust
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &program,
            &transcript_str,
            account_row_id,
            None,
        )
        .map_err(|e| e.to_string())?
```

Change the `session.started` payload:

```rust
        let payload = serde_json::json!({
            "agent_session_id": id,
            "adapter_id": adapter.id,
            "account_id": account_row_id,
        })
        .to_string();
```

- [ ] **Step 5: Build + run the full backend suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (all, incl. Task 3's `spawn_env_overrides_inherited_parent_var` and Task 4's `account_env_tests`).

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2a): inject account key into session PTY; record account_id"
```

---

## Task 6: Frontend — types, api, store, picker, tab header

**Files:**
- Modify: `src/types/agentSession.ts`, `src/api/agentSessions.ts`, `src/stores/agentSessions.ts`, `src/components/AgentsView.vue`

- [ ] **Step 1: Types**

In `src/types/agentSession.ts`: add to `AgentAdapter`:

```ts
  provider: string | null;
```

Add to `AgentSession` (after `transcript_path`):

```ts
  account_id: string | null;
  model_id: string | null;
```

- [ ] **Step 2: API**

In `src/api/agentSessions.ts`, change `startAgentSession`:

```ts
export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  accountId: string | null,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", {
    codingWorkspaceId,
    adapterId,
    accountId,
    cols,
    rows,
  });
}
```

- [ ] **Step 3: Store**

In `src/stores/agentSessions.ts`, change `start`:

```ts
  async function start(
    codingWorkspaceId: string,
    adapterId: string,
    accountId: string | null,
    cols: number,
    rows: number,
  ) {
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, accountId, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }
```

- [ ] **Step 4: AgentsView script — store, refs, computeds, watch, openTerminal**

In `src/components/AgentsView.vue` `<script setup>`:

Add the import + store (after the existing stores):

```ts
import { useProviderAccountsStore } from "../stores/providerAccounts";
// ...
const providerAccounts = useProviderAccountsStore();
```

Add the ref (after `newAdapterId`):

```ts
const newAccountId = ref("");
```

Add helpers/computeds (after `adapterLabel`):

```ts
const adapterProvider = (id: string) => store.adapters.find((a) => a.id === id)?.provider ?? null;
const adapterSupportsAccounts = computed(() => adapterProvider(newAdapterId.value) !== null);
const accountOptions = computed(() =>
  providerAccounts.list.filter((a) => a.provider === adapterProvider(newAdapterId.value)),
);
const accountLabel = (id: string | null) =>
  id ? (providerAccounts.list.find((a) => a.id === id)?.display_name ?? "") : "";

// Reset the chosen account when the adapter changes (its provider — and thus the
// valid accounts — differ); a stale account would fail the provider check.
watch(newAdapterId, () => {
  newAccountId.value = "";
});
```

In the existing `watch(() => workspaces.currentId, ...)` callback, add after `newWorktreeId.value = "";`:

```ts
    newAccountId.value = "";
```

Change `openTerminal`'s `store.start` call:

```ts
    await store.start(newWorktreeId.value, newAdapterId.value, newAccountId.value || null, 80, 24);
```

- [ ] **Step 5: AgentsView template — account select + tab header**

Add the account select **after** the adapter `<select>` (before the New-terminal button):

```html
        <select
          v-if="adapterSupportsAccounts"
          v-model="newAccountId"
          class="re-select"
          data-size="sm"
          aria-label="Provider account"
        >
          <option value="">Default (no key)</option>
          <option v-for="acct in accountOptions" :key="acct.id" :value="acct.id">
            {{ acct.display_name }}
          </option>
        </select>
```

Change the pane header (`agents__termhead`) muted span to show the bound account:

```html
          <span class="muted">
            {{ t.session.command }} · {{ t.session.status }}
            <template v-if="accountLabel(t.session.account_id)">
              · {{ accountLabel(t.session.account_id) }}
            </template>
          </span>
```

- [ ] **Step 6: Typecheck + build**

Run: `pnpm build`
Expected: vue-tsc + vite succeed.

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/types/agentSession.ts src/api/agentSessions.ts src/stores/agentSessions.ts src/components/AgentsView.vue
git commit -m "feat(m10b-2a): account picker + bound-account header in AgentsView"
```

---

## Task 7: Fake-agent marker + e2e

**Files:**
- Modify: `scripts/run-e2e.sh` (the `/tmp/uaw-fake-agent` heredoc)
- Create: `e2e/specs/agent-account.e2e.ts`

- [ ] **Step 1: Edit the fake agent to emit a boolean KEY marker**

In `scripts/run-e2e.sh`, replace the fake-agent heredoc body so it prints `KEY:set`/`KEY:unset` from env PRESENCE (never the value), keeping `AGENT-READY` + `exec cat`:

```bash
cat >/tmp/uaw-fake-agent <<'AGENT'
#!/usr/bin/env bash
if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${OPENAI_API_KEY:-}" ]; then
  printf 'KEY:set\n'
else
  printf 'KEY:unset\n'
fi
printf 'AGENT-READY\n'
exec cat
AGENT
chmod +x /tmp/uaw-fake-agent
```

- [ ] **Step 2: Write the e2e spec**

Create `e2e/specs/agent-account.e2e.ts`:

```ts
import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const KEY_VALUE = "sk-ant-e2e-SECRET-do-not-print";
const REPO = "/tmp/fixture-repo-acct";

// Text of the currently-visible terminal (multiple stay mounted via v-show).
const visibleTermText = () =>
  browser.execute(() => {
    const terms = Array.from(
      document.querySelectorAll('[data-testid="agent-terminal"]'),
    ) as HTMLElement[];
    const vis = terms.find((t) => t.offsetParent !== null) ?? terms[terms.length - 1];
    return vis ? (vis.textContent ?? "") : "";
  });

const accountOptionTexts = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll('[aria-label="Provider account"] option')).map((o) =>
      (o.textContent ?? "").trim(),
    ),
  );

/**
 * Milestone 10b-2a: bind a provider account to an agent terminal and inject its
 * key into the PTY env. The fake agent prints KEY:set / KEY:unset (boolean, never
 * the value), so injection is proven without ever exposing the key.
 */
describe("agent account injection", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (a: string[]) => execFileSync("git", ["-C", REPO, ...a], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "a@uaw.local"]);
    git(["config", "user.name", "UAW"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# acct fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project, repo, worktree, and an Anthropic account", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AcctProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AcctFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AcctProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AcctFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/acct");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });

    await (await $("button*=Providers")).click();
    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "anthropic");
    await (await $('[aria-label="Account display name"]')).setValue("My Anthropic");
    await (await $('[aria-label="API key"]')).setValue(KEY_VALUE);
    await (await $("button*=Add account")).click();
    await (await $('[data-testid="provider-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("injects the bound account key into the terminal env (never the value)", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/acct");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("My Anthropic");
    await (await $("button*=New terminal")).click();

    await (await $('[data-testid="agent-terminal"]')).waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await visibleTermText()).includes("KEY:set"), {
      timeout: 15_000,
      timeoutMsg: "expected KEY:set (the injected account key reached the agent env)",
    });
    // The raw key value must NEVER appear in the terminal/transcript.
    expect(await visibleTermText()).not.toContain(KEY_VALUE);
  });

  it("filters accounts by adapter and omits the key when none is selected", async () => {
    // Codex (openai) must NOT offer the anthropic account.
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Codex");
    expect(await accountOptionTexts()).not.toContain("My Anthropic");

    // Claude Code offers it; pick Default (no key) and launch -> KEY:unset.
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    expect(await accountOptionTexts()).toContain("My Anthropic");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("Default (no key)");
    await (await $("button*=New terminal")).click();

    await browser.waitUntil(async () => (await visibleTermText()).includes("KEY:unset"), {
      timeout: 15_000,
      timeoutMsg: "expected KEY:unset for a Default (no account) session",
    });
  });
});
```

- [ ] **Step 3: Typecheck the e2e**

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add scripts/run-e2e.sh e2e/specs/agent-account.e2e.ts
git commit -m "test(m10b-2a): fake-agent KEY marker + account injection e2e"
```

---

## Final verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` — all green (incl. SET-NULL, `account_env_tests`, spawn override).
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — clean.
- [ ] `pnpm build` + `pnpm e2e:typecheck` — clean.
- [ ] `pnpm e2e:docker` — all specs pass (existing agent-terminal specs unaffected by the fake-agent change; new `agent-account.e2e.ts` passes).
- [ ] Manual (macOS): bind a real Anthropic account to a `claude` terminal; confirm the CLI uses the API key (it prompts once to approve), and the tab header shows the account name.

## Review findings folded in (traceability)

- Gemini → `provider: None` (no dead-end picker) — Task 2.
- Neutralize `ANTHROPIC_AUTH_TOKEN` on inject — Task 2 (`clear_env`) + Task 4.
- Fail-closed: no-key-env/provider-mismatch/missing-key/wrong-workspace → opaque `Err` — Task 4.
- `account=None` → empty env → legacy behavior — Task 4 (`no_account_yields_empty_env`).
- Opaque key-path errors + key only as api_key_env value — Task 4 tests.
- Lock discipline: account-load folded into the worktree lock; `keystore::resolve()` hoisted; key resolved after release — Task 5.
- Override-proving spawn test — Task 3.
- SET-NULL + FK-on helper (already on) — Task 1.
- Trust indicator: tab header account + `session.started` `account_id` — Task 5/6.
- Frontend: `|| null`, reset on adapter+workspace change, conditional select, defensive label — Task 6.
- Fake-agent boolean marker + negative filter assertion + no-key-value guard — Task 7.
