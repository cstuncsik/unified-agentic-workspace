# Cross-platform keystore backends — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable the OS-keychain keystore on Windows (Credential Manager) and Linux (Secret Service) so provider API keys can be stored cross-platform — removing the only macOS-only gate in the backend.

**Architecture:** The `keyring` v3 crate is already cross-platform; the macOS `keyring::Entry`-based `OsKeyStore` impl works verbatim on all three OSes. Enable the per-OS backends in `Cargo.toml` (Linux uses the pure-Rust `async-secret-service` + `crypto-rust` path — no C build deps), widen the impl's `cfg`, add a dataless `NoBackend` error for the "no keychain available" case, and prove a real non-macOS backend works via a cross-process Windows CI round-trip.

**Tech Stack:** Rust, the `keyring` v3 crate (apple-native / windows-native / async-secret-service+crypto-rust), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-06-24-cross-platform-keystore-design.md`

**Conventions:** Branch `cstuncsik/cross-platform-keystore`. Every commit ends with the trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` (omitted from the brief commands below — always append it). Rust from `src-tauri/` (`cargo build`/`cargo test`). The implementer's machine is macOS, so the macOS path is verified locally; the Linux compile is proven by the existing Docker e2e build (it compiles the app on Ubuntu) and the Windows path by the Task 4 CI job.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src-tauri/Cargo.toml` | per-OS `keyring` deps + features | Modify |
| `src-tauri/src/services/keystore/mod.rs` | widen `OsKeyStore` cfg; `KeyStoreError` enum + keyring-error mapping; cross-process probe tests; doc | Modify |
| `src-tauri/src/commands/provider_accounts.rs` | map `NoBackend` → "no keychain" on set/delete; test | Modify |
| `src-tauri/src/commands/agent_sessions.rs` | map `NoBackend` → "no keychain" on the two key-read sites | Modify |
| `.github/workflows/keystore.yml` | Windows CI cross-process round-trip | Create |
| `README.md` | Linux Secret Service runtime requirement + isolation note | Modify |

---

## Task 1: Enable the Windows + Linux keyring backends

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/services/keystore/mod.rs`

**Context:** Today `keyring` is a dep only on macOS, and `OsKeyStore` is the real impl on macOS / an `Err`-returning stub elsewhere. Enable the verified per-OS backends and widen the real impl to all three OSes. The impl body is unchanged in this task (still `KeyStoreError` unit struct; the enum comes in Task 2). The macOS build is the local gate; Linux/Windows compiles are proven by CI (Task 4 + the Docker e2e).

- [ ] **Step 1: Replace the macOS-only keyring block with three per-OS blocks**

In `src-tauri/Cargo.toml`, replace the block at lines ~31-36 (the `# The OS keychain backend is wired only on macOS…` comment through the macOS `keyring = …` line) with:

```toml
# OS keychain backends, per platform. `keyring` v3 has no default features.
# macOS: apple-native (Keychain). Windows: windows-native (Credential Manager).
# Linux: async-secret-service + crypto-rust — the PURE-RUST zbus + pure-Rust crypto
# path (encrypted DBus session, no system libdbus/OpenSSL build deps, keeps the
# Linux build dep-free). Do NOT switch Linux to `sync-secret-service` / `crypto-openssl`
# / `openssl` / `vendored`: they link system libdbus/OpenSSL and break the dep-free build.
[target.'cfg(target_os = "macos")'.dependencies]
keyring = { version = "3", features = ["apple-native"] }

[target.'cfg(target_os = "windows")'.dependencies]
keyring = { version = "3", features = ["windows-native"] }

[target.'cfg(target_os = "linux")'.dependencies]
keyring = { version = "3", features = ["async-secret-service", "crypto-rust"] }
```

- [ ] **Step 2: Widen the real `OsKeyStore` impl + narrow the stub**

In `src-tauri/src/services/keystore/mod.rs`, the real `OsKeyStore` (struct + `impl OsKeyStore` + `impl Default` + `impl KeyStore`) is currently gated `#[cfg(target_os = "macos")]` and the stub gated `#[cfg(not(target_os = "macos"))]`. Change **every** `#[cfg(target_os = "macos")]` on those real-impl items to:

```rust
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
```

and **every** `#[cfg(not(target_os = "macos"))]` on the stub items to:

```rust
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
```

(4 attributes on the real impl: `pub struct OsKeyStore;`, `impl OsKeyStore`, `impl Default for OsKeyStore`, `impl KeyStore for OsKeyStore`; and the matching 4 on the stub.)

- [ ] **Step 3: Update the module doc comment**

In `src-tauri/src/services/keystore/mod.rs`, replace the first doc-comment paragraph:

```rust
//! Secret storage abstraction. The production backend is the OS keychain
//! (`OsKeyStore`, macOS only for now); dev/e2e use a plaintext `FileKeyStore`
//! gated behind `debug_assertions` so a release binary can never select it.
```

with:

```rust
//! Secret storage abstraction. The production backend is the OS keychain
//! (`OsKeyStore`): macOS Keychain, Windows Credential Manager, or Linux Secret
//! Service (via keyring's per-OS backends). dev/e2e use a plaintext `FileKeyStore`
//! gated behind `debug_assertions` so a release binary can never select it.
//! Targets without a wired backend get an `Err(NoBackend)` stub.
```

Also update the stub's inline comment ("Non-macOS: no native keychain wired yet … never invoked there") to: `// Targets with no keyring backend (not macOS/Windows/Linux): fail closed with NoBackend.`

- [ ] **Step 4: Build + test on macOS**

Run: `cd src-tauri && cargo build && cargo test`
Expected: clean build; all existing tests PASS. (This proves the macOS arm + the widened cfg compile. The Linux compile is proven by the Docker e2e at branch finish; Windows by Task 4.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/services/keystore/mod.rs
git commit -m "feat(keystore): enable Windows + Linux keyring backends"
```

---

## Task 2: `KeyStoreError` enum + "no keychain available" UX

**Files:**
- Modify: `src-tauri/src/services/keystore/mod.rs`
- Modify: `src-tauri/src/commands/provider_accounts.rs`
- Modify: `src-tauri/src/commands/agent_sessions.rs`

**Context:** On a Linux box with no Secret Service provider (or a locked keyring), key ops fail. Today every failure maps to a generic string. Distinguish "no keychain available" — keyring reports it as `Error::NoStorageAccess` / `Error::PlatformFailure` — and surface a clear, still-secret-free message. `KeyStoreError` becomes a dataless two-variant enum. Callers use `.is_err()`/`Err(_)` today, so they compile unchanged; this task updates them to match the variant.

- [ ] **Step 1: Change `KeyStoreError` to a dataless enum + add the keyring-error mapper**

In `src-tauri/src/services/keystore/mod.rs`, replace `pub struct KeyStoreError;` (and its `#[derive(Debug)]`) with:

```rust
/// Dataless keystore error: signals failure only, carrying nothing the command
/// layer could leak. Two variants so a "no OS keychain on this system" condition
/// (no secret) can be surfaced distinctly from a generic failure.
#[derive(Debug)]
pub enum KeyStoreError {
    /// No usable OS keychain (e.g. Linux with no Secret Service provider, a locked
    /// login keyring, or no session bus).
    NoBackend,
    /// Any other failure (the underlying error is dropped at this boundary).
    Failure,
}
```

Add, next to the real `OsKeyStore` impl (same `cfg`):

```rust
/// Map a keyring error to the dataless `KeyStoreError`. "No usable keychain"
/// conditions (no provider / locked / no session bus) come through as
/// `NoStorageAccess` / `PlatformFailure`; everything else is a generic `Failure`.
/// (Verify the exact v3 variant signatures against docs.rs/keyring/3.6.3 — both
/// carry a boxed source: `NoStorageAccess(_)` / `PlatformFailure(_)`.)
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn map_keyring_err(e: keyring::Error) -> KeyStoreError {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            KeyStoreError::NoBackend
        }
        _ => KeyStoreError::Failure,
    }
}
```

- [ ] **Step 2: Route the impls through the mapper / `Failure`**

In `src-tauri/src/services/keystore/mod.rs`:

Real `OsKeyStore` impl — replace its three methods' bodies with:

```rust
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        entry.set_password(secret).map_err(map_keyring_err)
    }

    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
```

Stub `OsKeyStore` impl (`not(any(...))`) — return the new variant:

```rust
    fn set(&self, _key_ref: &str, _secret: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
    fn get(&self, _key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
    fn delete(&self, _key_ref: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
```

`FileKeyStore` impl — change its three `.map_err(|_| KeyStoreError)` to `.map_err(|_| KeyStoreError::Failure)` (the `set`, `get`, and `delete` IO error arms).

- [ ] **Step 3: Surface `NoBackend` in `provider_accounts.rs`**

In `src-tauri/src/commands/provider_accounts.rs`, extend the import (line ~8) to bring in the variants:

```rust
use crate::services::keystore::{self, KeyStore, KeyStoreError};
```

Replace the set block (currently `if store.set(&keychain_ref, api_key).is_err() { return Err("Failed to store key".into()); }`) with:

```rust
    match store.set(&keychain_ref, api_key) {
        Ok(()) => {}
        Err(KeyStoreError::NoBackend) => {
            return Err("No OS keychain is available on this system.".into())
        }
        Err(KeyStoreError::Failure) => return Err("Failed to store key".into()),
    }
```

Replace the delete block (currently `if store.delete(&account.keychain_ref).is_err() { return Err("Failed to delete account".into()); }`) with:

```rust
        match store.delete(&account.keychain_ref) {
            Ok(()) => {}
            Err(KeyStoreError::NoBackend) => {
                return Err("No OS keychain is available on this system.".into())
            }
            Err(KeyStoreError::Failure) => return Err("Failed to delete account".into()),
        }
```

In the test module: the `FailingStore` returns `Err(KeyStoreError)` in its three methods — change each to `Err(KeyStoreError::Failure)`. (`KeyStoreError` is already imported in the test `use` on line ~151.)

- [ ] **Step 4: Add a `NoBackend` test in `provider_accounts.rs`**

In the `provider_accounts.rs` test module, after the `FailingStore` definition, add:

```rust
    // A keystore whose `set` reports no available keychain (the Linux-no-provider /
    // locked-keyring case) — drives the distinct "No OS keychain" message.
    struct NoBackendStore;
    impl KeyStore for NoBackendStore {
        fn set(&self, _r: &str, _s: &str) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::NoBackend)
        }
        fn get(&self, _r: &str) -> Result<Option<String>, KeyStoreError> {
            Err(KeyStoreError::NoBackend)
        }
        fn delete(&self, _r: &str) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::NoBackend)
        }
    }
```

Then add a test mirroring the existing "Failed to store key" test (`create_with_failing_store_reports_error` near line ~236) but with `NoBackendStore` and asserting the message:

```rust
    #[test]
    fn create_with_no_backend_reports_no_keychain() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let err = create_provider_account_inner(
            &conn,
            &NoBackendStore,
            &ws,
            "anthropic",
            "My Acct",
            "sk-test-123",
        )
        .unwrap_err();
        assert_eq!(err, "No OS keychain is available on this system.");
    }
```

(Match the exact `create_provider_account_inner` argument order + the `migrated_conn`/`workspace` fixtures used by the sibling test — read that test and mirror it.)

- [ ] **Step 5: Surface `NoBackend` at the two key-read sites in `agent_sessions.rs`**

In `src-tauri/src/commands/agent_sessions.rs`, extend the import (line ~15) to:

```rust
use crate::services::keystore::{self, KeyStore, KeyStoreError};
```

In `list_account_models` (the `match keystore::resolve().get(&account.keychain_ref) { … }` near line ~112) and in `resolve_session_env` (the `match store.get(&account.keychain_ref) { … }` near line ~615), replace the single `Err(_) => return Err("Failed to load the account key".into()),` arm with:

```rust
        Err(KeyStoreError::NoBackend) => {
            return Err("No OS keychain is available on this system.".into())
        }
        Err(KeyStoreError::Failure) => return Err("Failed to load the account key".into()),
```

(Leave the `Ok(Some(..))` and `Ok(None) => "Stored key for this account is missing"` arms unchanged.)

- [ ] **Step 6: Build + test**

Run: `cd src-tauri && cargo test`
Expected: PASS, including the new `create_with_no_backend_reports_no_keychain` and the unchanged `create_with_failing_store_reports_error` (still "Failed to store key" via `Failure`).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/keystore/mod.rs src-tauri/src/commands/provider_accounts.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(keystore): dataless NoBackend error + 'no keychain available' message"
```

---

## Task 3: Cross-process `OsKeyStore` round-trip probes

**Files:**
- Modify: `src-tauri/src/services/keystore/mod.rs`

**Context:** `OsKeyStore` has no automated test today. Add two `#[ignore]`-gated probe tests that, run as **separate `cargo test` processes** (Task 4's CI + local), prove a *real, persistent* backend: a process-global `mock` (keyring's silent fallback if a feature were dropped) loses the entry between processes and fails. `#[ignore]` so a normal `cargo test` (no keychain) skips them.

- [ ] **Step 1: Add the probe tests**

In `src-tauri/src/services/keystore/mod.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    // Cross-process probes for the REAL OsKeyStore. Run as TWO separate `cargo test`
    // invocations (see .github/workflows/keystore.yml): the set runs in one process,
    // the get in another. A real OS keychain persists across processes; keyring's
    // in-memory `mock` fallback does not, so a dropped backend feature fails here.
    // #[ignore]d so a normal `cargo test` (machine without a keychain) skips them.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    const PROBE_REF: &str = "uaw-keystore-ci-probe";

    #[test]
    #[ignore = "hits the real OS keychain; run via the keystore CI job or locally"]
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn os_keystore_set_probe() {
        let store = OsKeyStore::new();
        store
            .set(PROBE_REF, "probe-secret-v1")
            .expect("set on the real OS keychain");
        // Intentionally does NOT delete — os_keystore_get_delete_probe (a separate
        // process) reads it to prove cross-process persistence.
    }

    #[test]
    #[ignore = "hits the real OS keychain; run after os_keystore_set_probe in a SEPARATE process"]
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn os_keystore_get_delete_probe() {
        let store = OsKeyStore::new();
        // Cross-process persistence: the value set by os_keystore_set_probe in another
        // process must be visible. A process-global `mock` backend returns None here.
        assert_eq!(
            store.get(PROBE_REF).expect("get on the real OS keychain"),
            Some("probe-secret-v1".to_string()),
            "key did not persist across processes — backend may be the in-memory mock"
        );
        // Overwrite (last-write-wins).
        store.set(PROBE_REF, "probe-secret-v2").unwrap();
        assert_eq!(store.get(PROBE_REF).unwrap(), Some("probe-secret-v2".to_string()));
        // Delete, then missing -> None, then delete-missing -> Ok (idempotent contract).
        store.delete(PROBE_REF).unwrap();
        assert_eq!(store.get(PROBE_REF).unwrap(), None);
        assert!(store.delete(PROBE_REF).is_ok());
    }
```

- [ ] **Step 2: Verify locally on macOS (two separate processes)**

Run (each is its own process, so they cross the process boundary):
```bash
cd src-tauri
cargo test os_keystore_set_probe -- --ignored --exact
cargo test os_keystore_get_delete_probe -- --ignored --exact
```
Expected: both PASS (the set persists to the macOS Keychain and the get in the second process sees it, overwrites, deletes). Note: macOS may prompt for keychain access the first time.

- [ ] **Step 3: Confirm the normal test run still skips them**

Run: `cd src-tauri && cargo test`
Expected: PASS, with `os_keystore_set_probe` / `os_keystore_get_delete_probe` reported as `ignored` (no keychain touched).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/keystore/mod.rs
git commit -m "test(keystore): cross-process #[ignore] OsKeyStore round-trip probes"
```

---

## Task 4: Windows CI round-trip job + docs

**Files:**
- Create: `.github/workflows/keystore.yml`
- Modify: `README.md`

**Context:** Prove the Windows backend compiles + actually persists (Credential Manager works daemon-free on `windows-latest`), by running the two probes in **separate processes**. `cargo test` on the Tauri crate needs the frontend dist (the `tauri-build` script), so build the frontend first (mirroring `e2e.yml`). The Linux compile is already proven by the Docker e2e build; the Linux Secret Service round-trip is left as a documented manual/opt-in check (a `dbus-run-session` provider is flaky in CI).

- [ ] **Step 1: Create the Windows keystore workflow**

Create `.github/workflows/keystore.yml`:

```yaml
name: keystore

on:
  pull_request:
    paths:
      - "src-tauri/src/services/keystore/**"
      - "src-tauri/Cargo.toml"
      - ".github/workflows/keystore.yml"
  push:
    branches: [main]

jobs:
  windows-roundtrip:
    # Windows Credential Manager works on the stock runner (no daemon), so this
    # proves a REAL persistent backend (not keyring's in-memory mock) + that Windows
    # compiles. The two probes run as SEPARATE processes to cross the process boundary.
    runs-on: windows-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v6
      - uses: pnpm/action-setup@v6
      - uses: actions/setup-node@v6
        with:
          node-version: 22
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      # cargo test compiles the Tauri crate, whose build script needs the frontend dist.
      - run: pnpm build
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
        with:
          workspaces: src-tauri -> target
      - name: Round-trip probe (set, then get in a separate process)
        working-directory: src-tauri
        run: |
          cargo test os_keystore_set_probe -- --ignored --exact
          cargo test os_keystore_get_delete_probe -- --ignored --exact
```

- [ ] **Step 2: Validate the workflow YAML**

Run: `actionlint .github/workflows/keystore.yml` (if `actionlint` is unavailable, carefully review: valid `on`/`jobs`/`steps`, the two-invocation run block, `working-directory`).
Expected: no errors.

- [ ] **Step 3: Document the Linux runtime requirement**

In `README.md`, add a short subsection (under the existing setup/requirements area) — find the requirements/prerequisites section and insert:

```markdown
### Provider key storage (per OS)

API keys are stored in the OS keychain — macOS Keychain, Windows Credential Manager,
or, on **Linux**, the **Secret Service** (provided by GNOME Keyring or KWallet). A
normal Linux desktop session has one; a headless/minimal/SSH session without a Secret
Service provider (or with a locked login keyring) cannot store keys — adding an
account will report "No OS keychain is available on this system." There is no plaintext
fallback (by design). Note that on Linux/Windows, stored secrets are readable by any
process in the user's unlocked session (no per-app ACL, unlike the macOS Keychain) —
consistent with this app's single-user, local-first trust model.
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/keystore.yml README.md
git commit -m "ci+docs(keystore): Windows cross-process round-trip + Linux Secret Service note"
```

---

## Self-Review

**Spec coverage**
- keyring v3 per-OS features (apple-native / windows-native / async-secret-service+crypto-rust) + the warning comment → Task 1 Step 1. ✓
- Widen `OsKeyStore` to `any(macos,windows,linux)`; stub `not(any(...))` → Task 1 Step 2. ✓
- `crypto-rust` mandatory (encrypted DBus session) → Task 1 Step 1 (in the feature list + comment). ✓
- Dataless `KeyStoreError::NoBackend` + "No OS keychain is available" at the add-account + key-read sites → Task 2. ✓
- Mock guard: explicit/commented Cargo.toml (Task 1) + cross-process probes (Task 3) run as separate processes in CI (Task 4). ✓
- Invariants preserved: `FileKeyStore` (debug-only) + `resolve()` untouched (no step changes them); fail-closed (stub/NoBackend, no plaintext fallback). ✓
- Verification: existing FileKeyStore/resolver tests untouched; new NoBackend test (Task 2); cross-process probes (Task 3); Windows CI (Task 4); Linux compile via the Docker e2e build; module doc updated (Task 1); Linux runtime + isolation docs (Task 4). ✓
- Honest scope (accounts/SDK key path, not all agents) → in the spec (no code). ✓

**Placeholder scan:** none — each code step has complete code; each run step has a command + expected result. (Task 2 Step 4 / Step 5 say "mirror the sibling test / read that site" — these reference concrete existing anchors, `create_provider_account_inner`'s signature and the `match …get(...)` sites, with the exact replacement code given.)

**Type consistency:** `KeyStoreError` is the enum `{ NoBackend, Failure }` everywhere after Task 2; `map_keyring_err(keyring::Error) -> KeyStoreError`; the impls return `KeyStoreError::{NoBackend,Failure}`; callers match those exact variants; the probe tests use `OsKeyStore::new()` + `PROBE_REF`. The `any(macos,windows,linux)` cfg is identical on the impl, `map_keyring_err`, `PROBE_REF`, and the probe tests.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-24-cross-platform-keystore.md`.
