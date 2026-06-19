# Milestone 10b-1 — Provider Accounts + OS Keychain

## Goal

Let a user register provider API-key accounts (Anthropic, OpenAI) per workspace,
storing the **secret in the OS keychain** and only metadata in SQLite. The
frontend ever sees an opaque `account_id` + display name — never a raw key. This
is the credential-storage half of M10b; the Node-sidecar Agent SDK adapter +
per-session model/account picker that *consume* a stored key are **M10b-2**.

Done when a user can add / list / delete a provider account, the key is written to
and removed from the OS keychain, and a Rust-level test proves the key never
appears in the command's return value, the DB row, or any error string.

This spec folds in a multi-discipline design review (3 critical, 12 important,
11 minor). The most consequential corrections from that review are called out
inline (**[review]**).

## Decisions

- **Storage**: secret → OS keychain via the `keyring` crate; metadata row →
  SQLite `provider_accounts`. The row stores an opaque `keychain_ref` (= the
  account's UUID `id`), never the key.
- **Keychain abstraction**: a `KeyStore` trait with two impls — `OsKeyStore`
  (production) and `FileKeyStore` (tests/dev only). The create/delete helpers take
  `&dyn KeyStore` so unit tests inject a `FileKeyStore` with **zero env reliance**
  **[review: injectability]**.
- **`keyring` is a macOS-only dependency** **[review: build — corrected]**. The
  reviewer's "pin `sync-secret-service` + `vendored` for the Linux e2e build" was
  based on `keyring` v3's feature layout; `keyring` is now v4 (a `keyring-core`
  rewrite). Rather than chase Linux secret-service C-deps for a backend we never
  invoke, we depend on `keyring` **only on macOS** (the user's platform) with the
  `apple-native` feature, and stub `OsKeyStore` on every other target. The Linux
  Docker e2e build therefore never compiles `keyring` at all (no `libdbus`, no
  feature-name risk) and selects `FileKeyStore`. Real Linux/Windows keychain
  wiring is a documented later seam (the roadmap lists them; not needed to ship
  the macOS slice).
- **No silent prod plaintext downgrade** **[review: critical-adjacent]**.
  `FileKeyStore` and the `UAW_KEYSTORE_DIR` env switch are compiled **only under
  `#[cfg(debug_assertions)]`**. The e2e binary is built with `tauri build --debug`
  (`target/debug/uaw`), so `debug_assertions` is true there and the file backend is
  available; a **release** binary physically cannot select it and always uses
  `OsKeyStore`.
- **Opaque errors** **[review: CRITICAL]**. The whole app renders raw backend
  errors verbatim (`toast.error(String(e))`, `<p class="error">{{ store.error }}</p>`).
  The provider-account commands therefore map **every** keystore/DB failure to a
  **fixed, secret-free** message and never pass a raw `keyring`/`rusqlite` error
  through to the frontend.

## Data model — migration `0010_provider_accounts.sql`

```sql
-- A provider account is an API credential for an LLM provider, scoped to a
-- workspace (no project_id — accounts are workspace-global by design). The secret
-- itself lives in the OS keychain under `keychain_ref`; this row holds only
-- metadata. auth_mode is a forward-compat seam (OAuth is a later M10 follow-up);
-- in this slice it is always 'api-key'.
CREATE TABLE provider_accounts (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    provider     TEXT NOT NULL,                    -- 'anthropic' | 'openai'
    auth_mode    TEXT NOT NULL DEFAULT 'api-key',
    display_name TEXT NOT NULL,
    keychain_ref TEXT NOT NULL,                    -- opaque; equals id; never the key
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE INDEX idx_provider_accounts_workspace ON provider_accounts(workspace_id);
```

Register as migration `(10, "provider_accounts", include_str!(...))` in
`db/mod.rs` and bump `workspace.rs::migrations_are_idempotent` to assert
`version == 10`.

`ProviderAccount` model (`models/provider_account.rs`) mirrors `project.rs`:
`COLUMNS`, `from_row`, `list_by_workspace(conn, workspace_id)`, `get(conn, id)`,
`insert(conn, workspace_id, provider, auth_mode, display_name, keychain_ref)`,
`delete(conn, id) -> bool`. **The struct has no key field** — `serde` can only ever
serialize metadata.

## Keychain abstraction — `services/keystore/`

```rust
// services/keystore/mod.rs
pub trait KeyStore: Send + Sync {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError>;
    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError>;
    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError>; // idempotent
}

/// Opaque internal error. Its Display NEVER includes the secret. Command-layer
/// code still maps it to a fixed string before it can reach the frontend.
#[derive(Debug)]
pub enum KeyStoreError { Backend(String) }
```

Contract for **all** impls **[review: idempotency / FileKeyStore edges]**:
- `get` on a missing ref → `Ok(None)` (not an error).
- `delete` on a missing ref → `Ok(())` (idempotent — "not found" is success).
- `set` overwrites an existing ref (last write wins).

### `OsKeyStore` (production)

Service constant = the bundle identifier `io.n8n.uaw`; the keyring **account**
field = exactly `key_ref` (the UUID), **never** the provider/display_name
**[review: namespacing]**.

```rust
#[cfg(target_os = "macos")]
mod os_macos {
    // keyring v3, apple-native (Apple Keychain).
    // Entry::new(SERVICE, key_ref):
    //   set    -> entry.set_password(secret)
    //   get    -> match entry.get_password() { Ok(s)=>Some, Err(NoEntry)=>None, Err(e)=>Backend }
    //   delete -> match entry.delete_password() { Ok|Err(NoEntry)=>Ok, Err(e)=>Backend }
}

#[cfg(not(target_os = "macos"))]
mod os_stub {
    // Non-macOS (incl. the Linux Docker e2e host): no OS keychain wired yet.
    // Every method returns KeyStoreError::Backend(..). NEVER invoked in dev/e2e,
    // which select FileKeyStore via UAW_KEYSTORE_DIR; present only so the crate
    // compiles on Linux without pulling `keyring`.
}
```

`Cargo.toml`:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
keyring = { version = "3", features = ["apple-native"] }
```
(`keyring` v3 has **no default features**; `apple-native` is the only backend
pulled — no secret-service, no openssl, no dbus.)

### `FileKeyStore` (tests/dev only — `#[cfg(debug_assertions)]`)

A plaintext JSON-per-ref store rooted at a directory. Used by unit tests (temp
dir) and the e2e harness (`UAW_KEYSTORE_DIR`). Writes one file per `key_ref`;
`get` missing → `Ok(None)`; `delete` missing → `Ok(())`; `set` truncates.

### Resolver — `services/keystore/mod.rs::resolve()`

```rust
pub fn resolve() -> Box<dyn KeyStore> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("UAW_KEYSTORE_DIR") {
        return Box::new(FileKeyStore::new(dir));
    }
    Box::new(OsKeyStore::new())
}
```
Mirrors `resolve_program` (agent/mod.rs:74) but the env branch is **debug-gated**
**[review]**. A release build has no `UAW_KEYSTORE_DIR` branch and no
`FileKeyStore` type at all.

## Backend commands — `commands/provider_accounts.rs`

Allowed providers: `const PROVIDERS: &[&str] = &["anthropic", "openai"];`

### create

```rust
// Testable core — no Tauri State, no env. Unit tests pass a FileKeyStore directly.
pub fn create_provider_account_inner(
    conn: &Connection,
    store: &dyn KeyStore,
    workspace_id: &str,
    provider: &str,
    display_name: &str,
    api_key: &str,
) -> Result<ProviderAccount, String>
```
Order of operations **[review: leak / cleanup]**:
1. Validate: `provider` ∈ whitelist (else `"Unknown provider"`); `display_name`
   trimmed non-empty (else `"Account name is required"`); `api_key` trimmed
   non-empty (else `"API key is required"`); workspace exists (else
   `"Workspace not found"`). Validation runs **before** any key handling, so a
   rejected request never touches the keychain.
2. `id = new_id()`, `keychain_ref = id`.
3. `store.set(&keychain_ref, api_key)` — on `Err`, return the **fixed** string
   `"Failed to store key"` (the raw `KeyStoreError` is dropped, never surfaced).
4. `provider_account::insert(...)` — on `Err`, **roll back the keychain entry**
   (`store.delete(&keychain_ref)`, best-effort) then return `"Failed to save account"`.
5. Return the `ProviderAccount` (metadata only).

The `#[tauri::command] create_provider_account` is a thin wrapper: lock
`Mutex<Connection>`, `let store = keystore::resolve();`, call the inner fn. **It
takes `api_key: String` but the success payload is the key-less `ProviderAccount`.**

### list / delete

- `list_provider_accounts(state, workspace_id) -> Vec<ProviderAccount>` — metadata
  only (mirrors `list_workspaces`).
- `delete_provider_account(state, id)`: look up the row (for `keychain_ref`),
  `store.delete(keychain_ref)` (idempotent), then `provider_account::delete`.
  Errors map to fixed strings.

### `delete_workspace` keychain cleanup **[review: orphan ordering]**

`commands/workspaces.rs::delete_workspace` currently is one line relying on
`ON DELETE CASCADE`, which removes the rows but **not** the keychain entries —
leaving orphaned, now-unreferenceable secrets. New flow:

```rust
pub fn delete_workspace(state, id) -> Result<bool, String> {
    let store = keystore::resolve();
    let refs = { // short lock: read refs, release
        let conn = state.lock()...;
        provider_account::list_by_workspace(&conn, &id)?.iter().map(|a| a.keychain_ref).collect()
    };
    for r in &refs { store.delete(r); } // idempotent, best-effort; keychain pass BEFORE the row delete
    let conn = state.lock()...;
    workspace::delete(&conn, &id)  // cascade removes the rows
}
```
The keychain pass runs **before** the row cascade so a mid-failure never strands a
`keychain_ref` we can no longer enumerate. `delete` is idempotent, so a re-run is
safe.

## Error handling

- Every command maps keystore/DB errors to one of a small fixed set:
  `"Unknown provider"`, `"Account name is required"`, `"API key is required"`,
  `"Workspace not found"`, `"Failed to store key"`, `"Failed to save account"`,
  `"Failed to delete account"`. **No raw `keyring`/`rusqlite` error string is ever
  returned** — this is the mechanically-enforced no-leak invariant, given the app
  renders `String(e)` verbatim.

## Frontend — `ProvidersView`

- `types/providerAccount.ts` (`ProviderAccount` — id, workspace_id, provider,
  auth_mode, display_name, created_at, updated_at; **no key**),
  `api/providerAccounts.ts` (`listProviderAccounts`, `createProviderAccount`,
  `deleteProviderAccount`), `stores/providerAccounts.ts`.
- **Store** uses the monotonic `loadToken` + `list = []`-before-await guard exactly
  like `repositories.ts` **[review: cross-workspace leak]**. `create` does not
  optimistically push a stale row; it reloads or appends only if the token still
  matches.
- `components/ProvidersView.vue`:
  - Form: a provider `<select>` (Anthropic / OpenAI), a display-name input, an API
    key input. Each control gets an `aria-label` (`"Provider"`, `"Account display
    name"`, `"API key"`) **[review: a11y/e2e conventions]**; the key input is
    `type="password"` `autocomplete="new-password"`. Buttons use `data-variant`
    (brand/ghost/danger) — **not `data-tone`**, which in this codebase is only for
    badges/validation-messages **[review: DS]**.
  - The key is held in a local `ref`, sent on submit, then **cleared in a `finally`**
    (success or failure) and on `onUnmounted` **[review: secret hygiene]**. It is
    never put in the store, a log, or an error string.
  - List rows (`data-testid="provider-row"`): display_name · provider · auth_mode;
    a **Remove** button → `confirm('Remove account "<name>"? Its stored API key is
    deleted from the keychain.', 'Remove account', 'Remove')` (matches `useConfirm`
    signature; the dialog's confirm button reads "Remove").
  - States: loading / `.error` (`{{ store.error }}`) / empty ("No provider accounts
    yet — add one to use API-based agents").
- `App.vue`: add `"providers"` to `ActiveView`, a **Providers** sidebar button
  (after Agents) with `:aria-current`, a `<ProvidersView v-else-if="activeView ===
  'providers'" />` branch, and `providerAccounts.load(workspaceId)` in the
  workspace watch.

## Security

- Secret lives only in the OS keychain (macOS) / temp `FileKeyStore` (debug e2e).
  The DB stores only `keychain_ref` (opaque UUID). `ProviderAccount` has no key
  field, so no command return value or event can carry it.
- All command errors are fixed opaque strings → the `String(e)`-rendering UI can
  never display a key embedded in a backend error.
- `FileKeyStore` + the env switch are `#[cfg(debug_assertions)]`; a release build
  cannot select plaintext storage.
- All SQL parameterized; `keychain_ref` is the keyring account, never user text.

## Testing

### Rust (the security proof lives here — **[review: e2e can't introspect storage]**)

`commands/provider_accounts.rs` and `services/keystore` tests using a
`FileKeyStore` over a temp dir + a `migrated_conn` that enables
`PRAGMA foreign_keys = ON` (the `project.rs` helper, **not** `workspace.rs`'s)
**[review: FK cascade]**:

- `key_stored_in_keystore_not_db_or_payload`: create → returned struct has no key;
  `store.get(keychain_ref) == Some(api_key)`; the row contains only metadata.
- `sentinel_key_never_appears_in_any_error`: drive create through **every** failure
  branch (unknown provider, empty name, empty key, a forced `store.set` failure, a
  forced insert failure) with `api_key = "SENTINEL_SECRET_123"`; assert no returned
  `Err` string contains the sentinel.
- `serialized_account_has_no_key_field`: `serde_json::to_string(&account)` contains
  neither a key field nor the sentinel.
- `provider_whitelist_rejects_unknown_and_writes_no_key`,
  `rejects_empty_name`, `rejects_empty_or_whitespace_key` (each asserts
  `store.get` stays `None` — validation precedes key handling).
- `insert_failure_rolls_back_keychain` (forced insert error → `store.get` None).
- `delete_account_removes_key_and_row`; `list_is_metadata_only`.
- `delete_workspace_removes_all_account_keys`: workspace with N accounts (keys set
  in the file backend) → delete path → rows gone **and** `store.get` None for every
  `keychain_ref`.
- `KeyStore` round-trip: `set/get/delete`, `get_missing_returns_none`,
  `delete_missing_is_ok`, `overwrite_last_wins`.
- `resolver_selects_filekeystore_when_env_set` (debug-only test).
- Bump `migrations_are_idempotent` to `version == 10`.

### e2e — `e2e/specs/providers.e2e.ts`

`wdio.conf.ts beforeSession`: add
`process.env.UAW_KEYSTORE_DIR = path.join(sessionDir, "keystore")` alongside the
other four overrides, so each spec gets a hermetic file backend (the debug binary
honors it) **[review: env wiring]**.

Flow (UI only — no storage introspection): open **Providers** → add an Anthropic
account (provider, name, key) → assert the row appears with the display name and
provider, and that **no key field / key value is rendered** in the list → **Remove**
it (confirm dialog via `data-testid="confirm-dialog"`) → assert the row is gone.
Dedicated nothing-special fixture (no repo needed). Uses scoped selectors per the
combined-selector gotcha.

## Out of scope (M10b-2 and later)

- The Node-sidecar Claude Agent SDK adapter and the per-session model/account
  picker (the consumers of `keystore::resolve().get(keychain_ref)`).
- OAuth / Google / Copilot auth modes (the `auth_mode` column is the seam).
- Real Linux/Windows native keychain backends (`OsKeyStore` is macOS-real,
  stub-elsewhere; e2e uses `FileKeyStore`).
- Editing an account's key in place (delete + re-add for now).
