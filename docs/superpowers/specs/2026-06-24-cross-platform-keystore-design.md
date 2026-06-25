# Cross-platform keystore backends — Design

**Goal:** Enable the OS-keychain keystore on Windows (Credential Manager) and Linux (Secret Service) so provider API keys can be stored cross-platform — removing the only macOS-only gate in the backend, and unblocking **UAW accounts + the SDK agent's key path** on Linux/Windows.

**Status:** Approved design (post 5-discipline review, empirically verified). Ready for an implementation plan.

**Context:** Discovered while scoping "packaging / real distribution": Linux/Windows release builds have no key storage → the SDK agent (which requires a bound account) and UAW accounts are dead there. Fixing the keystore first makes a cross-platform release actually usable; packaging is the next slice.

---

## Background

`services/keystore/mod.rs` is a clean abstraction:
- `KeyStore` trait (`set`/`get`/`delete`); a **dataless** `KeyStoreError` (carries nothing the command layer could leak).
- `OsKeyStore`: on macOS, `keyring::Entry` (feature `apple-native`). On **non-macOS, a stub returning `Err`** (present only so the crate compiles; "never invoked there because dev/e2e select `FileKeyStore`").
- `FileKeyStore`: plaintext file store, `#[cfg(debug_assertions)]`-only (dev/e2e via `UAW_KEYSTORE_DIR`) — a release binary cannot select it.
- `resolve()`: debug + `UAW_KEYSTORE_DIR` → `FileKeyStore`; else `OsKeyStore`.

A grep confirmed the keystore is the **only** `cfg(target_os = "macos")` gate in the backend. Callers (`resolve_session_env` in `agent_sessions.rs`, `provider_accounts.rs`, `workspaces.rs`) go through `resolve()` + the trait — unchanged by this work.

## Decisions (verified during review)

1. **Stay on keyring v3** (resolved 3.6.3) — its pure-Rust path covers all three OSes; no v4 bump (avoids re-validating the working macOS API).
2. **Per-OS features** (the exact strings, empirically verified by compiling in the e2e Docker base image):
   - macOS: `["apple-native"]` (existing).
   - Windows: `["windows-native"]` (Credential Manager; pulls only pure-Rust `windows-sys`/`byteorder`/`zeroize` — no extra toolchain).
   - Linux: **`["async-secret-service", "crypto-rust"]`** — the **pure-Rust zbus** Secret Service path. **NOT `sync-secret-service`** (it links system `libdbus` via `libdbus-sys`, needs `libdbus-1-dev` — absent from the e2e apt list → would break the Linux build; proven). `crypto-rust` is **mandatory**: without a crypto feature the Secret Service session silently downgrades to `EncryptionType::Plain` (API key transits the DBus socket in cleartext — no compile error).
3. **Widen** the real `keyring::Entry`-based `OsKeyStore` to `cfg(any(macos, windows, linux))`; the `Err`-returning stub narrows to `cfg(not(any(...)))` (other targets we don't ship).
4. **Guard against keyring's silent `mock` fallback**: if a backend feature is ever dropped for a target, keyring links an in-memory `mock` store (keys appear to save, then vanish — no compile error). A consumer-side `compile_error!` keyed on the dep's feature is **not** cleanly possible (Cargo doesn't expose a dependency's features as a consumer `cfg`, nor auto-activate a consumer feature per-target). So the guard is two-layer: (a) the **explicit per-target Cargo.toml features + a warning comment** (a future editor must actively break it), and (b) the CI round-trip below is **cross-process** (a spawned helper `set`s; the test process `get`s) — a process-global `mock` returns `None` across processes and fails the test, whereas a same-process round-trip would pass even on the mock.
5. **Distinguish "no keychain available"** from a generic failure (the #1 first-run failure on the new platforms), staying within the dataless-error invariant.
6. **Fail-closed, no plaintext fallback** on a Linux box with no Secret Service provider (preserves the no-plaintext-in-release invariant).

---

## Changes

### `src-tauri/Cargo.toml`
Replace the single macOS `keyring` block with three per-OS blocks (features per Decision 2), plus a comment: *"Linux MUST use `async-secret-service` + `crypto-rust` (pure-Rust zbus + pure-Rust crypto). Do NOT use `sync-secret-service`/`crypto-openssl`/`openssl`/`vendored` — they link system libdbus/OpenSSL and break the dep-free Linux build."*

### `src-tauri/src/services/keystore/mod.rs`
- **Widen** the `OsKeyStore` impl `cfg` from `macos` to `any(target_os = "macos", target_os = "windows", target_os = "linux")`; the stub `cfg` to `not(any(...))`. The impl **body is unchanged** — `keyring::Entry::new(SERVICE, key_ref)` + `set_password`/`get_password`/`delete_credential`, with `Err(keyring::Error::NoEntry) => Ok(None)`/`Ok(())`. (Verified: the v3 `Entry` sync API + the `NoEntry`-for-missing contract hold identically on Credential Manager + Secret Service.)
- **`KeyStoreError` → a dataless two-variant enum**: `NoBackend` and `Failure` (neither carries any secret/inner error). `OsKeyStore` maps keyring `Error::NoStorageAccess | Error::PlatformFailure` → `NoBackend` (no provider / locked keyring / no DBus session); every other error → `Failure`. `FileKeyStore` maps its IO errors → `Failure`. `NoEntry` still maps to `Ok(None)`/`Ok(())` (unchanged contract).
- **Update the module doc comment** ("macOS only for now" → the cross-platform reality; the stub now covers only non-mac/win/linux targets).
- `FileKeyStore` (`debug_assertions`), `resolve()`, `SERVICE`, and the trait are **unchanged**.

### Command boundary (`provider_accounts.rs` create, `agent_sessions.rs` `resolve_session_env` / `list_account_models`)
Match the `KeyStoreError` variant: `NoBackend` → a fixed, secret-free string **"No OS keychain is available on this system."**; `Failure` → the existing fixed strings (`"Failed to store key"` / `"Failed to load the account key"`). The frontend renders these verbatim, so both stay secret-free.

---

## Verification

- **Unchanged:** the `FileKeyStore` + `resolve()` unit tests; the `provider_accounts`/`agent_sessions` tests (they inject `FileKeyStore`); the WebdriverIO Docker e2e (selects `FileKeyStore` via `UAW_KEYSTORE_DIR`).
- **Linux compile** is proven by the **existing e2e build** (it compiles the whole app on Ubuntu; the Linux keyring backend now links in — if it broke the build, e2e would fail). The verified `async-secret-service` + `crypto-rust` set adds **no apt deps**.
- **New `#[ignore]`-gated `OsKeyStore` round-trip** test: `set → get → overwrite → delete → get-missing-returns-None` (pins the trait contract — the first real test of `OsKeyStore` on any OS).
- **New `windows-latest` CI job** runs the `#[ignore]` round-trip **across a process boundary** (a spawned helper — the test binary re-invoked with an env flag — does the `set`; the test process does the `get`): Credential Manager works daemon-free, so this reliably proves a **real, persistent non-macOS backend** (a process-global `mock` regression returns `None` across processes → the test fails) and that Windows compiles.
- **Linux Secret Service round-trip**: an opt-in job under `dbus-run-session` + `gnome-keyring` (flakier → non-blocking initially) and/or a documented manual check. **macOS round-trip**: run locally.
- *(Nice-to-have)* `cargo-audit`/`cargo-deny` on the new Linux dep subtree (`secret-service` v4, `zbus` v4, the pure-Rust crypto crates) — currently advisory-clean; worth a watch.

---

## Security notes
- **No plaintext in release** preserved: `FileKeyStore` + the `UAW_KEYSTORE_DIR` branch stay `#[cfg(debug_assertions)]`-only; `resolve()` unchanged; widening only replaces the non-macOS `Err` stub. No new plaintext path.
- **`crypto-rust`** gives the encrypted Secret Service session (no plaintext on the DBus socket) and avoids linking system OpenSSL (no OpenSSL CVE surface) — the right choice for a network-free local app.
- **Per-app isolation:** Linux Secret Service (and Windows DPAPI) have weaker per-app isolation than the macOS Keychain — any process in the user's unlocked session can read the stored secrets. This matches UAW's existing same-user trust model (the key is already injected into a child process's env at spawn). Documented, not a regression.
- **`NoBackend`** carries no secret (a variant discriminant) — the dataless-error contract holds.

## Scope / honesty
This unblocks **UAW accounts (store/inject a key) + the SDK agent's key path** on Linux/Windows. It does **not** make every agent work end-to-end there: the **PTY agents** (`claude`/`codex`/`gemini`) still need their CLIs on the user's PATH (they don't use the keystore), and the **SDK agent** additionally needs Node on PATH + the bundled sidecar (the packaging slice). The spec claims exactly the keystore.

## Out of scope
The packaging / release CI (next slice — now unblocked) · bundling a Node runtime · a startup "keychain unavailable" banner (the add-account `NoBackend` message covers the worst case) · a keyring v3→v4 upgrade · any encrypted-file fallback for headless Linux (rejected — it would breach the no-plaintext invariant).

## Review findings incorporated
Corrected the Linux feature to `async-secret-service` (sync would link libdbus + break the e2e build — empirically proven) · `crypto-rust` mandatory (else plaintext DBus transport) · two-layer guard against the silent `mock` fallback (explicit Cargo features + a cross-process CI round-trip; a consumer `compile_error!` on a dep feature isn't cleanly possible) · the dataless `NoBackend` error + "no keychain available" message (the #1 first-run failure) · a Windows-CI round-trip + opt-in Linux dbus-run-session check (compile ≠ works; `OsKeyStore` was untested) · per-app-isolation + Secret Service runtime docs · honest scope (accounts/SDK key path, not all agents) · own the v3 choice (drop the stale v4 claim) · fail-closed, no plaintext fallback.
