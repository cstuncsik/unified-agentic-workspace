# Provider Accounts + OS Keychain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user register per-workspace provider API-key accounts (Anthropic, OpenAI) with the secret in the OS keychain and only metadata in SQLite; the frontend never receives a raw key.

**Architecture:** A `provider_accounts` SQLite table (metadata only) + a `KeyStore` trait with an `OsKeyStore` (macOS `keyring`, stub elsewhere) and a `#[cfg(debug_assertions)]` `FileKeyStore` for dev/e2e. Create/delete go through testable inner helpers that take `&dyn KeyStore`; commands map every keystore/DB error to a fixed, secret-free string because the UI renders backend errors verbatim. A Vue `ProvidersView` does add/list/delete.

**Tech Stack:** Rust + rusqlite + `keyring` v3 (macOS only, `apple-native`); Tauri 2 commands; Vue 3 + Pinia; WebdriverIO e2e.

---

## File Structure

**Backend (Rust, `src-tauri/src/`):**
- `db/migrations/0010_provider_accounts.sql` (create) — schema
- `db/mod.rs` (modify) — register migration 10
- `models/provider_account.rs` (create) — model + queries + tests
- `models/mod.rs` (modify) — `pub mod provider_account;`
- `services/keystore/mod.rs` (create) — `KeyStore` trait, `KeyStoreError`, `FileKeyStore`, `OsKeyStore`, `resolve()`
- `services/mod.rs` (modify) — `pub mod keystore;`
- `Cargo.toml` (modify) — macOS-only `keyring` dependency
- `commands/provider_accounts.rs` (create) — create/list/delete + inner helpers + tests
- `commands/mod.rs` (modify) — `pub mod provider_accounts;`
- `commands/workspaces.rs` (modify) — `delete_workspace` keychain cleanup
- `models/workspace.rs` (modify) — bump idempotency test to `10`
- `lib.rs` (modify) — register the 3 new commands in `invoke_handler!`

**Frontend (`src/`):**
- `types/providerAccount.ts` (create)
- `api/providerAccounts.ts` (create)
- `stores/providerAccounts.ts` (create)
- `components/ProvidersView.vue` (create)
- `App.vue` (modify) — `ActiveView`, nav button, view branch, workspace watch

**E2E:**
- `wdio.conf.ts` (modify) — `UAW_KEYSTORE_DIR`
- `e2e/specs/providers.e2e.ts` (create)

Run all backend tests with `cargo test --manifest-path src-tauri/Cargo.toml`. The CI/clippy gate is Linux (so it compiles the `OsKeyStore` *stub* + no `keyring`); local `cargo test` here is macOS (compiles the real `OsKeyStore`). `OsKeyStore` is never *invoked* in any test — tests use `FileKeyStore` — it only needs to compile.

---

## Task 1: Migration + `ProviderAccount` model

**Files:**
- Create: `src-tauri/src/db/migrations/0010_provider_accounts.sql`
- Modify: `src-tauri/src/db/mod.rs:53-55` (add migration tuple after the `0009` entry)
- Create: `src-tauri/src/models/provider_account.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Modify: `src-tauri/src/models/workspace.rs:144` (idempotency assertion `9` → `10`)

- [ ] **Step 1: Write the migration SQL**

Create `src-tauri/src/db/migrations/0010_provider_accounts.sql`:

```sql
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
```

- [ ] **Step 2: Register the migration in `db/mod.rs`**

In `src-tauri/src/db/mod.rs`, add the tuple to the `MIGRATIONS` array immediately after the `0009` entry (so the array ends `...0009 entry), (10, ...)]`):

```rust
    (
        10,
        "provider_accounts",
        include_str!("migrations/0010_provider_accounts.sql"),
    ),
```

- [ ] **Step 3: Bump the idempotency assertion**

In `src-tauri/src/models/workspace.rs`, in `migrations_are_idempotent`, change the final assertion:

```rust
        assert_eq!(version, 10);
```

- [ ] **Step 4: Run the idempotency test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml migrations_are_idempotent`
Expected: PASS (migration applies cleanly and is idempotent).

- [ ] **Step 5: Write the `ProviderAccount` model with failing tests**

Create `src-tauri/src/models/provider_account.rs`:

```rust
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

/// Metadata for a stored provider credential. The secret itself is NOT here — it
/// lives in the OS keychain under `keychain_ref`. This struct is the only thing
/// serialized to the frontend, so it can never carry a key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub id: String,
    pub workspace_id: String,
    pub provider: String,
    pub auth_mode: String,
    pub display_name: String,
    pub keychain_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str =
    "id, workspace_id, provider, auth_mode, display_name, keychain_ref, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<ProviderAccount> {
    Ok(ProviderAccount {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        provider: row.get("provider")?,
        auth_mode: row.get("auth_mode")?,
        display_name: row.get("display_name")?,
        keychain_ref: row.get("keychain_ref")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> rusqlite::Result<Vec<ProviderAccount>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM provider_accounts WHERE workspace_id = ?1 \
         ORDER BY created_at ASC, display_name ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<ProviderAccount>> {
    let sql = format!("SELECT {COLUMNS} FROM provider_accounts WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Insert a metadata row. The caller generates `id`/`keychain_ref` (they are equal)
/// and is responsible for having stored the secret in the keystore first.
pub fn insert(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    provider: &str,
    auth_mode: &str,
    display_name: &str,
    keychain_ref: &str,
) -> rusqlite::Result<ProviderAccount> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO provider_accounts
         (id, workspace_id, provider, auth_mode, display_name, keychain_ref, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![id, workspace_id, provider, auth_mode, display_name, keychain_ref, now],
    )?;
    Ok(get(conn, id)?.expect("provider account exists immediately after insert"))
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM provider_accounts WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Enables PRAGMA foreign_keys = ON so cascade behavior is real (the
    // workspace.rs helper does NOT — mirror project.rs instead).
    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn make_workspace(conn: &Connection) -> String {
        crate::models::workspace::create(conn, "WS", "mixed")
            .unwrap()
            .id
    }

    #[test]
    fn insert_then_list_and_get() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        let acct = insert(&conn, &id, &ws, "anthropic", "api-key", "Work key", &id).unwrap();
        assert_eq!(acct.provider, "anthropic");
        assert_eq!(acct.auth_mode, "api-key");
        assert_eq!(acct.keychain_ref, id);

        let listed = list_by_workspace(&conn, &ws).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(get(&conn, &id).unwrap().unwrap().id, id);
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn delete_removes_row() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        insert(&conn, &id, &ws, "openai", "api-key", "Key", &id).unwrap();
        assert!(delete(&conn, &id).unwrap());
        assert!(list_by_workspace(&conn, &ws).unwrap().is_empty());
        assert!(!delete(&conn, &id).unwrap());
    }

    #[test]
    fn deleting_workspace_cascades_accounts() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        insert(&conn, &id, &ws, "anthropic", "api-key", "Key", &id).unwrap();
        crate::models::workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &id).unwrap().is_none());
    }
}
```

- [ ] **Step 6: Register the module**

In `src-tauri/src/models/mod.rs`, add (keep alphabetical-ish ordering, after `project`):

```rust
pub mod provider_account;
```

- [ ] **Step 7: Run the model tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml models::provider_account`
Expected: PASS (3 tests).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/db/migrations/0010_provider_accounts.sql src-tauri/src/db/mod.rs \
        src-tauri/src/models/provider_account.rs src-tauri/src/models/mod.rs \
        src-tauri/src/models/workspace.rs
git commit -m "feat(m10b-1): provider_accounts table + model"
```

---

## Task 2: `KeyStore` trait, `FileKeyStore`, `OsKeyStore`, resolver

**Files:**
- Create: `src-tauri/src/services/keystore/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/Cargo.toml` (after the `[dependencies]` block)

- [ ] **Step 1: Add the macOS-only `keyring` dependency**

In `src-tauri/Cargo.toml`, after the existing `[dependencies]` block (after the `portable-pty = "0.9"` line), add a new target section:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
keyring = { version = "3", features = ["apple-native"] }
```

`keyring` v3 has no default features; `apple-native` pulls only the Apple Keychain backend (no secret-service / openssl / dbus). On Linux/Windows the crate is not pulled at all.

- [ ] **Step 2: Write the keystore module with failing tests**

Create `src-tauri/src/services/keystore/mod.rs`:

```rust
//! Secret storage abstraction. The production backend is the OS keychain
//! (`OsKeyStore`, macOS only for now); dev/e2e use a plaintext `FileKeyStore`
//! gated behind `debug_assertions` so a release binary can never select it.
//!
//! Contract for every impl:
//! - `get` on a missing ref returns `Ok(None)` (not an error).
//! - `delete` on a missing ref returns `Ok(())` (idempotent).
//! - `set` overwrites an existing ref (last write wins).

/// Opaque keystore error. Its contents are never surfaced to the frontend — the
/// command layer maps any failure to a fixed, secret-free string.
#[derive(Debug)]
pub enum KeyStoreError {
    Backend(String),
}

pub trait KeyStore: Send + Sync {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError>;
    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError>;
    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError>;
}

// ---- OS keychain backend (production) -------------------------------------

const SERVICE: &str = "io.n8n.uaw";

#[cfg(target_os = "macos")]
pub struct OsKeyStore;

#[cfg(target_os = "macos")]
impl OsKeyStore {
    pub fn new() -> Self {
        OsKeyStore
    }
}

#[cfg(target_os = "macos")]
impl KeyStore for OsKeyStore {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref)
            .map_err(|e| KeyStoreError::Backend(e.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|e| KeyStoreError::Backend(e.to_string()))
    }

    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref)
            .map_err(|e| KeyStoreError::Backend(e.to_string()))?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyStoreError::Backend(e.to_string())),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref)
            .map_err(|e| KeyStoreError::Backend(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyStoreError::Backend(e.to_string())),
        }
    }
}

// Non-macOS: no native keychain wired yet. Present only so the crate compiles on
// Linux (Docker e2e) / Windows; never invoked there because dev/e2e select
// FileKeyStore via UAW_KEYSTORE_DIR. `_` prefixes avoid unused-var lints.
#[cfg(not(target_os = "macos"))]
pub struct OsKeyStore;

#[cfg(not(target_os = "macos"))]
impl OsKeyStore {
    pub fn new() -> Self {
        OsKeyStore
    }
}

#[cfg(not(target_os = "macos"))]
impl KeyStore for OsKeyStore {
    fn set(&self, _key_ref: &str, _secret: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Backend(
            "OS keychain not available on this platform".into(),
        ))
    }
    fn get(&self, _key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        Err(KeyStoreError::Backend(
            "OS keychain not available on this platform".into(),
        ))
    }
    fn delete(&self, _key_ref: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Backend(
            "OS keychain not available on this platform".into(),
        ))
    }
}

// ---- File backend (dev/e2e only) ------------------------------------------

#[cfg(debug_assertions)]
pub struct FileKeyStore {
    dir: std::path::PathBuf,
}

#[cfg(debug_assertions)]
impl FileKeyStore {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        FileKeyStore { dir }
    }

    fn path(&self, key_ref: &str) -> std::path::PathBuf {
        // key_ref is a generated UUID (safe filename); store one file per ref.
        self.dir.join(key_ref)
    }
}

#[cfg(debug_assertions)]
impl KeyStore for FileKeyStore {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError> {
        std::fs::write(self.path(key_ref), secret).map_err(|e| KeyStoreError::Backend(e.to_string()))
    }

    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        match std::fs::read_to_string(self.path(key_ref)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(KeyStoreError::Backend(e.to_string())),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError> {
        match std::fs::remove_file(self.path(key_ref)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(KeyStoreError::Backend(e.to_string())),
        }
    }
}

// ---- Resolver -------------------------------------------------------------

/// Production uses the OS keychain. In a debug build only, `UAW_KEYSTORE_DIR`
/// selects a plaintext file backend (dev + e2e). A release build has neither the
/// env branch nor the `FileKeyStore` type, so it can never downgrade to plaintext.
pub fn resolve() -> Box<dyn KeyStore> {
    #[cfg(debug_assertions)]
    {
        if let Some(dir) = std::env::var_os("UAW_KEYSTORE_DIR") {
            return Box::new(FileKeyStore::new(dir));
        }
    }
    Box::new(OsKeyStore::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-keystore-test-{}", crate::util::new_id()));
        d
    }

    #[test]
    fn file_store_set_get_delete_round_trip() {
        let store = FileKeyStore::new(temp_dir());
        store.set("ref-1", "sekret").unwrap();
        assert_eq!(store.get("ref-1").unwrap(), Some("sekret".to_string()));
        store.delete("ref-1").unwrap();
        assert_eq!(store.get("ref-1").unwrap(), None);
    }

    #[test]
    fn file_store_get_missing_returns_none() {
        let store = FileKeyStore::new(temp_dir());
        assert_eq!(store.get("nope").unwrap(), None);
    }

    #[test]
    fn file_store_delete_missing_is_ok() {
        let store = FileKeyStore::new(temp_dir());
        assert!(store.delete("nope").is_ok());
    }

    #[test]
    fn file_store_overwrite_last_wins() {
        let store = FileKeyStore::new(temp_dir());
        store.set("r", "first").unwrap();
        store.set("r", "second").unwrap();
        assert_eq!(store.get("r").unwrap(), Some("second".to_string()));
    }

    #[test]
    fn resolver_selects_file_backend_when_env_set() {
        let dir = temp_dir();
        std::env::set_var("UAW_KEYSTORE_DIR", &dir);
        let store = resolve();
        // Behavioral proof it is the file backend: a set writes into `dir`.
        store.set("probe", "value").unwrap();
        assert!(dir.join("probe").exists());
        std::env::remove_var("UAW_KEYSTORE_DIR");
    }
}
```

> Note: `keyring` v3's delete method is `delete_credential()`. If the resolved patch version still exposes only `delete_password()`, swap the one call — the macOS compiler resolves it immediately. `keyring::Error::NoEntry` is the missing-entry variant in v3.

- [ ] **Step 3: Register the module**

In `src-tauri/src/services/mod.rs`, add:

```rust
pub mod keystore;
```

- [ ] **Step 4: Run the keystore tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml services::keystore`
Expected: PASS (5 tests). (On macOS the real `OsKeyStore` compiles but is not invoked.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/keystore/mod.rs src-tauri/src/services/mod.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(m10b-1): KeyStore trait + FileKeyStore + macOS OsKeyStore + resolver"
```

---

## Task 3: Provider-account commands (create / list / delete)

**Files:**
- Create: `src-tauri/src/commands/provider_accounts.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs:36+` (register 3 commands in `generate_handler!`)

- [ ] **Step 1: Write the commands module with failing tests**

Create `src-tauri/src/commands/provider_accounts.rs`:

```rust
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::provider_account::{self, ProviderAccount};
use crate::models::workspace;
use crate::services::keystore::{self, KeyStore};
use crate::util::new_id;

/// Providers a user may register in this slice.
const PROVIDERS: &[&str] = &["anthropic", "openai"];

/// Testable core: validate, store the secret, insert the row, roll back the
/// keychain entry on insert failure. Takes `&dyn KeyStore` so tests inject a
/// FileKeyStore with no env reliance. Every error is a FIXED, secret-free string
/// because the frontend renders backend errors verbatim.
pub fn create_provider_account_inner(
    conn: &Connection,
    store: &dyn KeyStore,
    workspace_id: &str,
    provider: &str,
    display_name: &str,
    api_key: &str,
) -> Result<ProviderAccount, String> {
    // 1. Validate BEFORE touching the keychain.
    if !PROVIDERS.contains(&provider) {
        return Err("Unknown provider".into());
    }
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err("Account name is required".into());
    }
    if api_key.trim().is_empty() {
        return Err("API key is required".into());
    }
    match workspace::get(conn, workspace_id) {
        Ok(Some(_)) => {}
        Ok(None) => return Err("Workspace not found".into()),
        Err(_) => return Err("Failed to save account".into()),
    }

    // 2. Identifiers. keychain_ref == id (opaque UUID).
    let id = new_id();
    let keychain_ref = id.clone();

    // 3. Store the secret. Drop the raw error — never surface it.
    if store.set(&keychain_ref, api_key).is_err() {
        return Err("Failed to store key".into());
    }

    // 4. Insert metadata; roll back the keychain entry on failure.
    match provider_account::insert(
        conn,
        &id,
        workspace_id,
        provider,
        "api-key",
        display_name,
        &keychain_ref,
    ) {
        Ok(account) => Ok(account),
        Err(_) => {
            let _ = store.delete(&keychain_ref);
            Err("Failed to save account".into())
        }
    }
}

#[tauri::command]
pub fn create_provider_account(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    provider: String,
    display_name: String,
    api_key: String,
) -> Result<ProviderAccount, String> {
    let store = keystore::resolve();
    let conn = state.lock().map_err(|e| e.to_string())?;
    create_provider_account_inner(
        &conn,
        store.as_ref(),
        &workspace_id,
        &provider,
        &display_name,
        &api_key,
    )
}

#[tauri::command]
pub fn list_provider_accounts(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<ProviderAccount>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    provider_account::list_by_workspace(&conn, &workspace_id).map_err(|_| "Failed to load accounts".into())
}

/// Testable core for delete: remove the keychain entry (idempotent) then the row.
pub fn delete_provider_account_inner(
    conn: &Connection,
    store: &dyn KeyStore,
    id: &str,
) -> Result<bool, String> {
    if let Some(account) = provider_account::get(conn, id).map_err(|_| "Failed to delete account")? {
        // Idempotent: a missing keychain entry is success.
        if store.delete(&account.keychain_ref).is_err() {
            return Err("Failed to delete account".into());
        }
    }
    provider_account::delete(conn, id).map_err(|_| "Failed to delete account".into())
}

#[tauri::command]
pub fn delete_provider_account(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<bool, String> {
    let store = keystore::resolve();
    let conn = state.lock().map_err(|e| e.to_string())?;
    delete_provider_account_inner(&conn, store.as_ref(), &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keystore::FileKeyStore;

    const SENTINEL: &str = "SENTINEL_SECRET_123";

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn temp_store() -> FileKeyStore {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-cmd-test-{}", new_id()));
        FileKeyStore::new(d)
    }

    fn make_ws(conn: &Connection) -> String {
        crate::models::workspace::create(conn, "WS", "mixed").unwrap().id
    }

    #[test]
    fn key_stored_in_keystore_not_db_or_payload() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let acct =
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "Work", SENTINEL).unwrap();
        // Returned struct carries only metadata.
        assert_eq!(acct.provider, "anthropic");
        assert_eq!(acct.auth_mode, "api-key");
        // Secret is in the keystore under keychain_ref...
        assert_eq!(store.get(&acct.keychain_ref).unwrap(), Some(SENTINEL.to_string()));
        // ...and the serialized account never contains it.
        let json = serde_json::to_string(&acct).unwrap();
        assert!(!json.contains(SENTINEL));
        assert!(!json.to_lowercase().contains("api_key"));
        assert!(!json.to_lowercase().contains("\"key\""));
    }

    #[test]
    fn sentinel_key_never_appears_in_any_error() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);

        // Each failure branch, all carrying the sentinel as the key.
        let cases = [
            create_provider_account_inner(&conn, &store, &ws, "evil-provider", "n", SENTINEL),
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "   ", SENTINEL),
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "n", "   "),
            create_provider_account_inner(&conn, &store, "no-such-ws", "anthropic", "n", SENTINEL),
        ];
        for case in cases {
            if let Err(msg) = case {
                assert!(!msg.contains(SENTINEL), "error leaked the key: {msg}");
            }
        }
    }

    #[test]
    fn whitelist_rejects_unknown_and_writes_no_key() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let err = create_provider_account_inner(&conn, &store, &ws, "openrouter", "n", SENTINEL)
            .unwrap_err();
        assert_eq!(err, "Unknown provider");
        assert!(provider_account::list_by_workspace(&conn, &ws).unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_name_and_empty_key() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        assert_eq!(
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "  ", SENTINEL)
                .unwrap_err(),
            "Account name is required"
        );
        assert_eq!(
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "n", "   ")
                .unwrap_err(),
            "API key is required"
        );
    }

    #[test]
    fn delete_account_removes_key_and_row() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let acct =
            create_provider_account_inner(&conn, &store, &ws, "openai", "K", SENTINEL).unwrap();
        assert!(delete_provider_account_inner(&conn, &store, &acct.id).unwrap());
        assert_eq!(store.get(&acct.keychain_ref).unwrap(), None);
        assert!(provider_account::get(&conn, &acct.id).unwrap().is_none());
        // Idempotent: deleting again is Ok and returns false (no row).
        assert!(!delete_provider_account_inner(&conn, &store, &acct.id).unwrap());
    }

    #[test]
    fn list_is_metadata_only() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        create_provider_account_inner(&conn, &store, &ws, "anthropic", "A", SENTINEL).unwrap();
        let json = serde_json::to_string(&provider_account::list_by_workspace(&conn, &ws).unwrap())
            .unwrap();
        assert!(!json.contains(SENTINEL));
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/commands/mod.rs`, add (after `projects`):

```rust
pub mod provider_accounts;
```

- [ ] **Step 3: Register the commands in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ ... ]`, add after the workspaces block (near line 41):

```rust
            commands::provider_accounts::list_provider_accounts,
            commands::provider_accounts::create_provider_account,
            commands::provider_accounts::delete_provider_account,
```

- [ ] **Step 4: Run the command tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::provider_accounts`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/provider_accounts.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(m10b-1): provider-account create/list/delete commands with opaque errors"
```

---

## Task 4: `delete_workspace` keychain cleanup

**Files:**
- Modify: `src-tauri/src/commands/workspaces.rs:1-6` (imports) and `:57-61` (`delete_workspace`)

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/commands/provider_accounts.rs` `tests` module a test for a reusable cleanup helper (the helper lets us test without Tauri `State`):

```rust
    #[test]
    fn workspace_cleanup_removes_all_account_keys() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let a = create_provider_account_inner(&conn, &store, &ws, "anthropic", "A", SENTINEL).unwrap();
        let b = create_provider_account_inner(&conn, &store, &ws, "openai", "B", SENTINEL).unwrap();

        super::cleanup_workspace_keys(&conn, &store, &ws);

        assert_eq!(store.get(&a.keychain_ref).unwrap(), None);
        assert_eq!(store.get(&b.keychain_ref).unwrap(), None);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::provider_accounts::tests::workspace_cleanup_removes_all_account_keys`
Expected: FAIL — `cleanup_workspace_keys` not found.

- [ ] **Step 3: Add the cleanup helper**

In `src-tauri/src/commands/provider_accounts.rs` (module level, before the `#[cfg(test)]`), add:

```rust
/// Best-effort: delete every stored key for a workspace's accounts BEFORE the
/// rows are removed (by cascade), so a `keychain_ref` is never stranded. `delete`
/// is idempotent so re-runs are safe.
pub fn cleanup_workspace_keys(conn: &Connection, store: &dyn KeyStore, workspace_id: &str) {
    if let Ok(accounts) = provider_account::list_by_workspace(conn, workspace_id) {
        for account in accounts {
            let _ = store.delete(&account.keychain_ref);
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::provider_accounts::tests::workspace_cleanup_removes_all_account_keys`
Expected: PASS.

- [ ] **Step 5: Wire the cleanup into `delete_workspace`**

In `src-tauri/src/commands/workspaces.rs`, replace the `delete_workspace` command (and add the keystore import at the top, after the existing `use crate::models::workspace...` line):

```rust
use crate::services::keystore;
```

```rust
#[tauri::command]
pub fn delete_workspace(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let store = keystore::resolve();
    // Remove keychain entries for this workspace's provider accounts BEFORE the
    // row cascade, so no keychain_ref is left orphaned. Short lock for the read.
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        crate::commands::provider_accounts::cleanup_workspace_keys(&conn, store.as_ref(), &id);
    }
    let conn = state.lock().map_err(|e| e.to_string())?;
    workspace::delete(&conn, &id).map_err(|e| e.to_string())
}
```

- [ ] **Step 6: Run the full backend suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (all existing + new tests). Also run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and fix any lint.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/workspaces.rs src-tauri/src/commands/provider_accounts.rs
git commit -m "feat(m10b-1): delete workspace keychain entries before row cascade"
```

---

## Task 5: Frontend types + API + store

**Files:**
- Create: `src/types/providerAccount.ts`
- Create: `src/api/providerAccounts.ts`
- Create: `src/stores/providerAccounts.ts`

- [ ] **Step 1: Create the type**

Create `src/types/providerAccount.ts`:

```ts
/** Metadata for a stored provider credential. The API key is NEVER part of this
 *  type — it lives in the OS keychain; the frontend only ever sees metadata. */
export interface ProviderAccount {
  id: string;
  workspace_id: string;
  provider: string;
  auth_mode: string;
  display_name: string;
  keychain_ref: string;
  created_at: string;
  updated_at: string;
}
```

- [ ] **Step 2: Create the API layer**

Create `src/api/providerAccounts.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { ProviderAccount } from "../types/providerAccount";

export function listProviderAccounts(workspaceId: string): Promise<ProviderAccount[]> {
  return invoke<ProviderAccount[]>("list_provider_accounts", { workspaceId });
}

export interface CreateProviderAccountInput {
  workspaceId: string;
  provider: string;
  displayName: string;
  apiKey: string;
}

export function createProviderAccount(
  input: CreateProviderAccountInput,
): Promise<ProviderAccount> {
  return invoke<ProviderAccount>("create_provider_account", { ...input });
}

export function deleteProviderAccount(id: string): Promise<boolean> {
  return invoke<boolean>("delete_provider_account", { id });
}
```

- [ ] **Step 3: Create the store**

Create `src/stores/providerAccounts.ts`:

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import type { ProviderAccount } from "../types/providerAccount";
import * as api from "../api/providerAccounts";
import type { CreateProviderAccountInput } from "../api/providerAccounts";

export const useProviderAccountsStore = defineStore("providerAccounts", () => {
  const list = ref<ProviderAccount[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has switched workspaces.
  let loadToken = 0;

  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    try {
      const rows = await api.listProviderAccounts(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(input: CreateProviderAccountInput) {
    const token = loadToken;
    const account = await api.createProviderAccount(input);
    if (token !== loadToken) return account;
    list.value.push(account);
    return account;
  }

  async function remove(id: string) {
    await api.deleteProviderAccount(id);
    list.value = list.value.filter((a) => a.id !== id);
  }

  return { list, loading, error, load, create, remove };
});
```

- [ ] **Step 4: Typecheck**

Run: `pnpm e2e:typecheck` (or `pnpm tsc --noEmit`)
Expected: PASS (no type errors).

- [ ] **Step 5: Commit**

```bash
git add src/types/providerAccount.ts src/api/providerAccounts.ts src/stores/providerAccounts.ts
git commit -m "feat(m10b-1): provider-account frontend types, api, store"
```

---

## Task 6: `ProvidersView` + App wiring

**Files:**
- Create: `src/components/ProvidersView.vue`
- Modify: `src/App.vue` (import, `ActiveView`, workspace watch, nav button, view branch)

- [ ] **Step 1: Create the view**

Create `src/components/ProvidersView.vue`:

```vue
<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProviderAccountsStore } from "../stores/providerAccounts";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";

const workspaces = useWorkspacesStore();
const accounts = useProviderAccountsStore();
const toast = useToast();
const { confirm } = useConfirm();

const provider = ref("anthropic");
const displayName = ref("");
const apiKey = ref("");
const submitting = ref(false);

const canAdd = computed(() => displayName.value.trim() !== "" && apiKey.value.trim() !== "");

function clearKey() {
  apiKey.value = "";
}
onUnmounted(clearKey);

async function add() {
  const name = displayName.value.trim();
  const key = apiKey.value.trim();
  if (!name || !key || !workspaces.currentId) return;
  submitting.value = true;
  try {
    await accounts.create({
      workspaceId: workspaces.currentId,
      provider: provider.value,
      displayName: name,
      apiKey: key,
    });
    displayName.value = "";
    toast.success("Account added");
  } catch (e) {
    toast.error(String(e));
  } finally {
    // Clear the secret on success AND failure.
    clearKey();
    submitting.value = false;
  }
}

async function removeAccount(id: string, name: string) {
  const confirmed = await confirm(
    `Remove account "${name}"? Its stored API key is deleted from the keychain.`,
    "Remove account",
    "Remove",
  );
  if (!confirmed) return;
  try {
    await accounts.remove(id);
    toast.success("Account removed");
  } catch (e) {
    toast.error(String(e));
  }
}

const providerLabel = (p: string) => (p === "anthropic" ? "Anthropic" : p === "openai" ? "OpenAI" : p);
</script>

<template>
  <section>
    <h2 class="view-title">Providers</h2>
    <h3 class="section-title">API Key Accounts</h3>

    <form class="attach" @submit.prevent="add">
      <select v-model="provider" class="re-select" aria-label="Provider">
        <option value="anthropic">Anthropic</option>
        <option value="openai">OpenAI</option>
      </select>
      <input
        v-model="displayName"
        class="re-input"
        type="text"
        placeholder="Account name"
        aria-label="Account display name"
      />
      <input
        v-model="apiKey"
        class="re-input attach__key"
        type="password"
        autocomplete="new-password"
        placeholder="API key"
        aria-label="API key"
      />
      <button class="re-button" data-variant="brand" type="submit" :disabled="submitting || !canAdd">
        Add account
      </button>
    </form>

    <p v-if="accounts.loading" class="muted">Loading accounts…</p>
    <p v-else-if="accounts.error" class="error">{{ accounts.error }}</p>
    <p v-else-if="accounts.list.length === 0" class="muted">
      No provider accounts yet — add one to use API-based agents.
    </p>
    <ul v-else class="rows">
      <li
        v-for="account in accounts.list"
        :key="account.id"
        class="re-card"
        data-testid="provider-row"
      >
        <span class="acct__main">
          <span class="acct__name">{{ account.display_name }}</span>
          <span class="acct__meta">{{ providerLabel(account.provider) }} · {{ account.auth_mode }}</span>
        </span>
        <button
          type="button"
          class="re-button"
          data-variant="danger"
          data-size="sm"
          @click="removeAccount(account.id, account.display_name)"
        >
          Remove
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.view-title {
  margin: 0 0 0.25rem;
  font-size: 1.2rem;
}

.section-title {
  margin: 0 0 1rem;
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--re-color-text-muted);
}

.attach {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
}

.attach__key {
  flex: 1;
  min-width: 16rem;
}

.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.rows .re-card {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
}

.acct__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.acct__meta {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-danger-text);
}
</style>
```

- [ ] **Step 2: Wire into `App.vue` — import + store**

In `src/App.vue` `<script setup>`, add the import alongside the other view imports (near line 18):

```ts
import ProvidersView from "./components/ProvidersView.vue";
```

Add the store. Find where the other stores are instantiated (e.g. `const reviews = useReviewsStore();`) and add:

```ts
import { useProviderAccountsStore } from "./stores/providerAccounts";
// ...
const providerAccounts = useProviderAccountsStore();
```

- [ ] **Step 3: Extend `ActiveView` and the workspace watch**

In `src/App.vue`, extend the `ActiveView` union (after `"agents"`):

```ts
type ActiveView =
  | "inbox"
  | "projects"
  | "artifacts"
  | "sources"
  | "coding"
  | "reviews"
  | "board"
  | "agents"
  | "providers";
```

In the `watch(() => workspaces.currentId, ...)` block, add after `reviews.load(workspaceId);`:

```ts
      providerAccounts.load(workspaceId);
```

- [ ] **Step 4: Add the nav button and view branch**

In the template, add a nav button after the **Agents** button (mirror the Board button markup):

```html
        <button
          class="re-button"
          data-variant="ghost"
          :aria-current="activeView === 'providers' ? 'page' : undefined"
          type="button"
          @click="activeView = 'providers'"
        >
          Providers
        </button>
```

And add the view branch after `<AgentsView ... />`:

```html
        <ProvidersView v-else-if="activeView === 'providers'" />
```

- [ ] **Step 5: Typecheck + build the frontend**

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/ProvidersView.vue src/App.vue
git commit -m "feat(m10b-1): ProvidersView add/list/remove + sidebar nav"
```

---

## Task 7: E2E harness wiring + spec

**Files:**
- Modify: `wdio.conf.ts:51-57` (beforeSession — add `UAW_KEYSTORE_DIR`)
- Create: `e2e/specs/providers.e2e.ts`

- [ ] **Step 1: Add the hermetic keystore dir to the e2e env**

In `wdio.conf.ts`, inside `beforeSession`, add after the `UAW_TRANSCRIPTS_DIR` line:

```ts
    process.env.UAW_KEYSTORE_DIR = path.join(sessionDir, "keystore");
```

This makes the debug binary select the `FileKeyStore` so e2e never touches the real OS keychain.

- [ ] **Step 2: Write the e2e spec**

Create `e2e/specs/providers.e2e.ts`:

```ts
import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 10b-1 end-to-end: add a provider account, see it listed (with no key
 * rendered), and remove it. The key is stored in the file-backed keystore the
 * debug binary selects via UAW_KEYSTORE_DIR — the UI only ever shows metadata.
 */
describe("provider accounts", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("adds an account and lists it without exposing the key", async () => {
    await (await $("button*=Providers")).click();

    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "anthropic");
    await (await $('[aria-label="Account display name"]')).setValue("My Anthropic");
    await (await $('[aria-label="API key"]')).setValue("sk-ant-e2e-SECRET-key");
    await (await $("button*=Add account")).click();

    const row = await $('[data-testid="provider-row"]');
    await row.waitForExist({ timeout: 10_000 });
    await browser.waitUntil(
      async () => (await textOf('[data-testid="provider-row"]')).includes("My Anthropic"),
      { timeout: 10_000, timeoutMsg: "expected the account row to show its name" },
    );

    // The raw key must never be rendered anywhere in the list.
    const rowText = await textOf('[data-testid="provider-row"]');
    expect(rowText).not.toContain("sk-ant-e2e-SECRET-key");
    expect(rowText).toContain("Anthropic");
  });

  it("removes the account", async () => {
    await (await $('[data-testid="provider-row"] button*=Remove')).click();
    const confirmDialog = await $('[data-testid="confirm-dialog"]');
    await confirmDialog.waitForDisplayed({ timeout: 5_000 });
    await confirmDialog.$("button*=Remove").click();

    await browser.waitUntil(async () => (await $$('[data-testid="provider-row"]').length) === 0, {
      timeout: 10_000,
      timeoutMsg: "expected the account row to be removed",
    });
  });
});
```

> Selector note: `[data-testid="provider-row"] button*=Remove` is a valid descendant selector (an element selector followed by a partial-text selector); the invalid form is `[attr] button*=Text` where `[attr]` is a bare attribute. Both forms here begin with an element/attribute pair that wdio parses correctly. If wdio rejects it, scope it: `const row = await $('[data-testid="provider-row"]'); await row.$("button*=Remove").click();`

- [ ] **Step 3: Add the spec to the typecheck/run set if needed**

Confirm `e2e/specs/providers.e2e.ts` is matched by the wdio `specs` glob (it lives beside the other specs, so it is). Run the typecheck:

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 4: Run the providers e2e locally (optional, slow) or defer to Docker**

Run (after `pnpm e2e:build`): `pnpm e2e --spec e2e/specs/providers.e2e.ts`
Expected: 2 passing tests. (The full Docker e2e in the finishing step is authoritative.)

- [ ] **Step 5: Commit**

```bash
git add wdio.conf.ts e2e/specs/providers.e2e.ts
git commit -m "test(m10b-1): provider accounts e2e + hermetic keystore dir"
```

---

## Final verification

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` — all green.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — clean (no dead code: every `pub fn` is used by a command or test).
- [ ] `pnpm e2e:typecheck` — clean.
- [ ] `pnpm e2e:docker` — full Linux e2e (compiles the `OsKeyStore` stub + `FileKeyStore`, runs `providers.e2e.ts` and the existing specs).
- [ ] Manual smoke (macOS): add an Anthropic account, confirm it appears in Keychain Access under service `io.n8n.uaw`, remove it, confirm the keychain entry is gone.

---

## Notes on the review findings folded in

- **Opaque errors (CRITICAL):** all command errors are a fixed set; raw `keyring`/`rusqlite` errors are dropped (Task 3) — proven by `sentinel_key_never_appears_in_any_error`.
- **Key non-exposure is a Rust assertion (CRITICAL):** `key_stored_in_keystore_not_db_or_payload` + `serialized account has no key`; the e2e only checks the UI doesn't render the key.
- **keyring build (CRITICAL, corrected):** macOS-only dep + non-macOS stub; Linux Docker never compiles `keyring`.
- **No release plaintext downgrade:** `FileKeyStore` + env branch are `#[cfg(debug_assertions)]` (Task 2).
- **Injectable keystore:** `create_/delete_provider_account_inner(&dyn KeyStore)` (Tasks 3-4).
- **Idempotent cleanup ordering:** `cleanup_workspace_keys` before the row cascade; `delete` treats missing as success (Tasks 2, 4).
- **wdio env + store loadToken + secret hygiene + a11y/DS + confirm contract:** Tasks 5-7.
