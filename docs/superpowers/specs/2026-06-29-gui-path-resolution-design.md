# GUI-launch PATH resolution — Design

**Goal:** A bundled UAW app launched from Finder/Dock can find its agent binaries. Today a `.app` inherits **launchd's minimal PATH** (`/usr/bin:/bin:/usr/sbin:/sbin`), not the user's login-shell PATH, so every agent fails — PTY `claude`/`codex`/`gemini` (spawned by bare name via `portable-pty` → "No viable candidates found in PATH") and the SDK agent (spawns `node` via `Command` → ENOENT). `tauri dev` works only because it inherits the terminal's PATH.

**Status:** Approved design (post 5-lens + codex review; findings folded in). Ready for an implementation plan. **Scope (user-confirmed):** augment the process PATH from the login shell at startup **+ better not-found errors**. NOT a Settings PATH-override UI.

**Context:** Found in the v0.1.0 bundle smoke-test (the publish gate). Ships as v0.1.1 — v0.1.0 stays an unpublished draft until this lands. Both agent spawn paths **inherit the process env**, so fixing the process PATH once at startup repairs both with no spawn-site change.

---

## Design

### 1. `augment_process_path()` — run it FIRST in `run()`, before Tauri (`src-tauri/src/lib.rs`)
Call it as the **first statement of `pub fn run()`, before `tauri::Builder::default()`** — NOT inside `.setup()`. By the time `setup()` runs, wry/tao/WebKit threads already exist and may `getenv` concurrently; `std::env::set_var` mutating the global env table under a concurrent `getenv` is a data race. Running it on the main thread before the builder is constructed is the safe window — do it there with a one-line safety comment ("main thread, before any Tauri/wry thread exists; nothing reads PATH before this point — DB/keystore use their own env vars"). **Edition note:** the crate is **edition 2021**, where `std::env::set_var` is a *safe* fn — do **NOT** wrap it in `unsafe { … }` (that trips `unused_unsafe` under the repo's self-imposed `clippy -D warnings`); the mitigation here is purely *positional* (call site before threads), and a future edition-2024 bump will require an `unsafe { … }` wrapper at that point. Agents only spawn via the `start_agent_session` command (after the webview loads), so they always see the augmented PATH.

### 2. Resolve the login PATH — a guarded, bounded, fail-safe shell probe
- **Platform:** `#[cfg(unix)]` does the work; `#[cfg(not(unix))]` is a no-op (Windows GUI apps inherit the registry system+user PATH).
- **Shell selection (don't blindly trust `$SHELL`):** use `$SHELL` only if it is an **absolute path to an existing, executable file** whose basename is a known POSIX shell (`zsh`/`bash`/`sh`/`dash`/`ksh`). Otherwise fall back to `/bin/zsh` (macOS) / `/bin/sh` (other unix). This skips `fish`/`nu` (non-POSIX `$PATH`/`echo` semantics break the probe) and an unset/garbage `$SHELL` — falling back rather than leaving the bundle broken.
- **Invocation — login, non-interactive:** `<shell> -lc 'printf %s%s%s "$UAW_PATH_NONCE" "$PATH" "$UAW_PATH_NONCE"'`. **Login (`-l`), not interactive (`-i`)** — PATH is set in login files (`.zprofile`/`.bash_profile`/`.profile`, where brew/nvm/asdf write); interactive rc files (`.zshrc`/`.bashrc`) print banners, assume a TTY, and can hang (a hang → timeout → augmentation silently does nothing → bundle stays broken). `printf` (not `echo -n`) for portability.
- **Neutral CWD (security):** run the probe with `current_dir(std::env::temp_dir())`. A login shell run with CWD inside a freshly-opened repo could source a repo-local direnv `.envrc`/dotfile that prepends `./node_modules/.bin` — which, since we prepend the login PATH, would let a malicious repo's planted `node`/`claude` run with the injected account key. A neutral CWD removes repo-relative entries from the resolved PATH, which is also what makes "prepend login dirs" safe.
- **Nonce sentinel (injection-proof, noise-proof):** generate a per-run nonce with `util::new_id()`, pass it as the `UAW_PATH_NONCE` **env var** on the probe command (NOT interpolated into the command string — the command string is a fixed literal, so no shell injection). `printf` brackets `$PATH` with the nonce; extraction takes the content between the **last** nonce pair (so a login-profile banner printed before it can't win).
- **Bounded wait (simple, not the `spawn_oneshot` condvar mirror):** spawn with stdin `/dev/null`, stdout piped, stderr discarded; collect output on a thread and `mpsc::recv_timeout(~5s)`. On timeout, abandon the probe (the child is a harmless one-shot; the app proceeds). This is a startup-launch path, so the bound matters — but a plain reader-thread + `recv_timeout` is enough; do not replicate `sdk::spawn_oneshot`'s watcher/condvar/killer.
- **Fail-safe (never break launch):** any of — no viable shell, spawn error, non-zero exit, timeout, missing/!2 nonce markers, empty extracted PATH — leaves the process PATH **untouched**. Never panics.

### 3. Merge, never replace (`merge_paths`, pure)
`merge_paths(login: &str, current: &str) -> String` using `std::env::split_paths` / `join_paths` (platform-correct, not manual `:` splitting):
- Concatenate **login dirs then current dirs**, keep **first occurrence** of each (order-preserving dedup), drop empty segments (an empty PATH field means cwd — a smell we don't want).
- Prepend-login is correct (the user's login PATH must beat launchd's minimal one) and safe given the neutral-CWD probe. Merge-not-replace preserves the dev/e2e PATH and is irrelevant to the abs-path override seams (`UAW_AGENT_BIN`/`_NODE`/`_SDK_SIDECAR`), which bypass PATH entirely.
- Both-sides collision is the load-bearing case: `merge_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/sbin")` → `/opt/homebrew/bin:/usr/bin:/sbin` (homebrew's `node` wins over `/usr/bin`'s; `/usr/bin` appears once, at the login position).

### 4. PTY not-found error — a one-line hint (NOT a pre-check)
**Cut the originally-planned `find_on_path` + pre-check** — `portable-pty`'s `CommandBuilder::spawn` already searches PATH and already emits a program-named, PATH-listing error; a hand-rolled search would be a second, divergent PATH scan on the hot path. Instead, append the override hint to the **existing** `map_err` at `src-tauri/src/services/agent/pty.rs:52`:
```rust
.map_err(|e| format!(
    "failed to start agent '{program}': {e}\n\
     If '{program}' is installed, set UAW_AGENT_BIN to its full path."
))?;
```
This runs after the startup PATH augmentation, so the common case already works; the hint only dresses the genuine not-found tail. `resolve_program` (`mod.rs`) is **untouched**.

### 5. SDK not-found error — edit the existing message (one clause)
In `sdk::node_spawn_error`, **replace** the existing literal (don't concatenate — it already says "not found on PATH"):
```rust
"Node.js was not found on PATH. The SDK agent requires Node.js 18+ — install it or set UAW_AGENT_NODE to its path (PTY agents are unaffected).".to_string()
```
The existing `spawn_missing_node_reports_node_not_found` test (asserts `contains("Node.js was not found on PATH")`) stays green.

### Module layout
A focused `src-tauri/src/services/login_path.rs`: the pure `merge_paths` + `extract_path(stdout, nonce) -> Option<String>` + `run_login_shell(shell: &Path, cwd: &Path, timeout) -> Option<String>` (takes the shell path as a param, returns the PATH — no global mutation), and the thin `augment_process_path()` (selects the shell, calls the above, `set_var`s). It's startup infra (also benefits `git.rs`'s `Command::new("git")`), so it sits under `services/`, not `services/agent/`. A one-line comment at the `run()` call site + in `pty.rs`/`sdk.rs` module docs notes that bare-name spawns rely on the startup augmentation.

## Security
- **Trust boundary:** running the user's own login shell = the same code that runs when they open a terminal — within the same-user desktop model. The two hardenings that keep it from widening attack surface: the **neutral CWD** (no repo-local rc/`.envrc` sourced → no attacker-controlled PATH entry → "prepend login" is safe) and the **fixed command string + nonce-via-env** (no untrusted interpolation → no shell injection; `$SHELL` validated to an allowlisted absolute executable, not arbitrarily exec'd).
- **No new secret, no network.** The probe reads PATH only. The injected account key path is untouched (still masked via `sdk::redact`). The override seams stay absolute-path-only and bypass PATH.
- **No new dependency** — pure std (`split_paths`/`join_paths`, `mpsc`, `Command`). Honors the no-`which` constraint; no `find_on_path` to add.

## Testing
Pure, globals-free units (so they don't race the parallel `cargo test` suite — critical, since `PATH`/`$SHELL` are fixed shared names the repo's unique-var idiom can't protect):
- **`merge_paths`** — independent **literal** expectations (never compute the oracle by re-running the impl): both-sides collision (the homebrew-beats-/usr/bin case), within-side dup, empty-segment dropped, full-order preservation across 3+ dirs, empty-login → current unchanged, empty-current → login.
- **`extract_path`** — last-pair extraction; missing nonce → `None`; single nonce → `None`; empty between → `None`; profile-noise-before-the-pair → still extracts the real PATH.
- **`run_login_shell`** — point it at a fake `chmod 0755` temp script (under `temp_dir().join("uaw-shell-{new_id()}")`) that `printf`s a nonce'd PATH → assert the returned PATH; a script that exits non-zero / omits the nonce → `None`; a `sleep`-script exceeding a short timeout → `None` (mirrors `sdk::spawn_oneshot_times_out`). All via the param (no `set_var`/`$SHELL` read) → parallel-safe.
- **PTY + SDK error strings** — assert the new substrings (`UAW_AGENT_BIN` / `UAW_AGENT_NODE`).
- The thin `augment_process_path()` is **not** unit-tested (it mutates global PATH + reads `$SHELL` — would race; and launchd's minimal PATH can't be simulated). `#[cfg(not(unix))]` is a compile-time no-op (note it, nothing to test).
- **Real proof — manual bundle smoke-test** (the gate that caught this; CI only gates e2e, so this is the sole bundle-PATH exercise): launch the built `.app` from Finder (neutral PATH) → start a PTY agent (claude) AND an SDK agent → both spawn + produce output. Verify a not-installed binary yields the clear hint. (Optionally confirm that *without* the fix the bundle reproduces "No viable candidates" — so the smoke test is a real regression gate.)

## Out of scope
A Settings PATH/binary-override UI · changing the `UAW_AGENT_BIN`/`_NODE`/`_SDK_SIDECAR` seams · Windows PATH handling (already correct) · auto-installing agent CLIs · extracting a shared timeout primitive from `spawn_oneshot`.

## Review findings incorporated
**BLOCKERs:** `set_var` moved to the top of `run()` before Tauri threads (data-race), mitigated positionally + a safety comment (edition 2021 → set_var is safe-by-signature; NO `unsafe` block or it trips `unused_unsafe`/clippy) · `-i` dropped to `-lc` (interactive hang → silent non-fix). **MAJORs:** `find_on_path`/pre-check **cut** → a one-line hint on `pty.rs:52`'s existing error (portable-pty already searches+errors) · **neutral CWD** for the probe (repo-local `.envrc` → key-bearing spawn) · `$SHELL` validated to an allowlisted absolute executable with a `/bin/zsh`|`/bin/sh` fallback (fish/nu/unset) · the test split into pure globals-free units (the `augment_process_path` test would race the suite) · `merge_paths` semantics pinned (first-occurrence-wins, both-sides-collision, `split_paths`/`join_paths`, drop empties) + literal expectations (no tautology). **Resolved contradiction:** keep a bound on the shell probe but a **simple `recv_timeout`**, not the `spawn_oneshot` condvar/watcher mirror. **MINORs:** SDK message is an in-place **edit** (not append) · nonce via **env** + `printf` + **last-pair** extraction · `#[cfg(unix)]`/`#[cfg(not(unix))]` (matching `SdkHandle::kill`) · startup→spawn coupling commented · module under `services/`.
