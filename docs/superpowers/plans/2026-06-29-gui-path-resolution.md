# GUI-launch PATH resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A bundled UAW app launched from Finder/Dock finds its agent binaries (today it inherits launchd's minimal PATH, so `claude`/`codex`/`gemini` and the SDK's `node` can't be found).

**Architecture:** At startup — before Tauri spawns any thread — run the user's login shell once to recover the real PATH and merge it into the process env (both spawn paths inherit it). Plus two one-line "not found" error hints. Pure, globals-free cores so the tests don't race the suite.

**Tech Stack:** Rust (`std::env::{split_paths,join_paths,set_var}`, `std::process::Command`, `mpsc::recv_timeout`), Tauri 2.

---

## File Structure
- **Create `src-tauri/src/services/login_path.rs`** — the whole PATH-resolution unit: pure `merge_paths`/`extract_path`/`is_executable`, the param-driven `run_login_shell`, the `$SHELL`-reading `pick_login_shell`, and the thin `augment_process_path()` (`#[cfg(unix)]` real / `#[cfg(not(unix))]` no-op). Owns its `#[cfg(test)]` tests.
- **Modify `src-tauri/src/services/mod.rs`** — add `pub mod login_path;`.
- **Modify `src-tauri/src/lib.rs`** — call `services::login_path::augment_process_path()` as the first statement of `run()`, before `tauri::Builder`.
- **Modify `src-tauri/src/services/agent/pty.rs:52`** — append the `UAW_AGENT_BIN` hint to the existing spawn `map_err`.
- **Modify `src-tauri/src/services/agent/sdk.rs`** — one-clause edit to `node_spawn_error`'s message (add the `UAW_AGENT_NODE` hint).
- **Modify `src-tauri/tauri.conf.json` + `src-tauri/Cargo.toml`** — version `0.1.0` → `0.1.1`.

**Ordering:** Task 1 (the module) is the fix. Task 2 (error hints) is independent. Task 3 (version bump) is release mechanics. Tasks 1–2 are TDD on pure cores; the startup wiring + the bundle behavior are proven by the manual smoke-test (after tasks).

---

### Task 1: The `login_path` module + wire it into `run()`

**Files:**
- Create: `src-tauri/src/services/login_path.rs`
- Modify: `src-tauri/src/services/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/services/agent/pty.rs` (module doc), `src-tauri/src/services/agent/sdk.rs` (module doc)

- [ ] **Step 1: Write the failing tests** — register the module in `src-tauri/src/services/mod.rs` (add `pub mod login_path;`) so it compiles, then create `src-tauri/src/services/login_path.rs` with ONLY this content (the `imp` fns + the `pub use` target don't exist yet → compile fails):

```rust
//! Repairs the process PATH at startup. A GUI-launched app (macOS Finder/Dock, some
//! Linux desktop launches) inherits a minimal PATH lacking the user's shell-configured
//! dirs (homebrew/nvm/asdf), so bare-name spawns — the PTY agents, the SDK's `node`,
//! `git` — can't find their binaries. `augment_process_path()` runs the user's LOGIN
//! shell once to recover the real PATH and merges it in. It is called from `run()`
//! BEFORE Tauri spawns any thread: `set_var` under a concurrent `getenv` is a data race.

/// Windows/non-unix GUI apps inherit the full registry/system PATH — nothing to repair.
#[cfg(not(unix))]
pub fn augment_process_path() {}

#[cfg(unix)]
pub use imp::augment_process_path;

#[cfg(unix)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    const NONCE_ENV: &str = "UAW_PATH_NONCE";
    // The nonce is supplied via the env var above (no interpolation into this string → no
    // shell injection); printf brackets the real PATH so rc-file banners can be skipped.
    const PROBE: &str = r#"printf '%s%s%s' "$UAW_PATH_NONCE" "$PATH" "$UAW_PATH_NONCE""#;
    const ALLOWED_SHELLS: &[&str] = &["zsh", "bash", "sh", "dash", "ksh"];

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn merge_prepends_login_first_occurrence_wins() {
            // /usr/bin is in both → kept once, at the login position; homebrew beats it.
            assert_eq!(
                merge_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/sbin"),
                "/opt/homebrew/bin:/usr/bin:/sbin"
            );
        }
        #[test]
        fn merge_dedups_within_a_side_and_drops_empty_segments() {
            assert_eq!(merge_paths("/a:/b:/a", "/c::/b"), "/a:/b:/c");
        }
        #[test]
        fn merge_preserves_full_order() {
            assert_eq!(merge_paths("/l1:/l2:/l3", "/c1:/c2"), "/l1:/l2:/l3:/c1:/c2");
        }
        #[test]
        fn merge_empty_login_returns_current_and_vice_versa() {
            assert_eq!(merge_paths("", "/usr/bin:/bin"), "/usr/bin:/bin");
            assert_eq!(merge_paths("/usr/bin:/bin", ""), "/usr/bin:/bin");
        }
        #[test]
        fn extract_takes_the_last_nonce_pair_past_banner_noise() {
            let n = "NONCEXYZ";
            let out = format!("Welcome {n} fake banner\n{n}/real/bin:/usr/bin{n}");
            assert_eq!(extract_path(&out, n).as_deref(), Some("/real/bin:/usr/bin"));
        }
        #[test]
        fn extract_none_when_unbracketed_or_empty() {
            assert_eq!(extract_path("no markers here", "NONCE"), None);
            assert_eq!(extract_path("oneNONCEmarker", "NONCE"), None);
            assert_eq!(extract_path("NONCENONCE", "NONCE"), None); // empty PATH between
        }

        fn write_fake_shell(body: &str) -> PathBuf {
            let p = std::env::temp_dir().join(format!("uaw-fake-shell-{}", crate::util::new_id()));
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn run_login_shell_returns_the_nonced_path() {
            // The fake shell ignores its -lc args and emits the nonce'd PATH from env.
            let sh = write_fake_shell(r#"printf '%s%s%s' "$UAW_PATH_NONCE" "/fake/bin:/usr/bin" "$UAW_PATH_NONCE""#);
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_secs(5));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got.as_deref(), Some("/fake/bin:/usr/bin"));
        }
        #[test]
        fn run_login_shell_none_on_nonzero_exit() {
            let sh = write_fake_shell("exit 1");
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_secs(5));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got, None);
        }
        #[test]
        fn run_login_shell_none_on_timeout() {
            let sh = write_fake_shell("sleep 10");
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_millis(100));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got, None);
        }
        #[test]
        fn is_executable_true_only_for_exec_regular_file() {
            let f = std::env::temp_dir().join(format!("uaw-exec-{}", crate::util::new_id()));
            std::fs::write(&f, "x").unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!is_executable(&f), "non-exec file");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_executable(&f), "exec file");
            let _ = std::fs::remove_file(&f);
            assert!(!is_executable(&std::env::temp_dir()), "a directory is not executable-program");
        }
    }
}
```

- [ ] **Step 2: Run — expect a COMPILE failure** (the `imp` fns are referenced by the tests but not defined)

Run: `cd src-tauri && cargo test --lib login_path 2>&1 | tail -15`
Expected: compile error — `cannot find function merge_paths`/`extract_path`/`run_login_shell`/`is_executable` in this scope.

- [ ] **Step 3: Implement the module body** — inside `mod imp { … }`, ABOVE the `#[cfg(test)]` block, add:

```rust
    /// Repair the process PATH from the user's login shell. Fail-safe: any problem leaves
    /// PATH untouched. MUST run on the main thread before any other thread can `getenv`
    /// (edition 2021: `set_var` is a safe call; the race mitigation is purely positional —
    /// do NOT wrap it in `unsafe`, which trips `unused_unsafe` under clippy `-D warnings`).
    pub fn augment_process_path() {
        let shell = pick_login_shell();
        let nonce = crate::util::new_id();
        let Some(login) =
            run_login_shell(&shell, &std::env::temp_dir(), &nonce, Duration::from_secs(5))
        else {
            return;
        };
        let current = std::env::var("PATH").unwrap_or_default();
        let merged = merge_paths(&login, &current);
        if !merged.is_empty() {
            std::env::set_var("PATH", merged);
        }
    }

    /// `$SHELL` if it is an absolute path to an existing, executable, allowlisted POSIX
    /// shell; otherwise `/bin/zsh` (macOS) / `/bin/sh`. Skips fish/nu/unset/garbage.
    fn pick_login_shell() -> PathBuf {
        if let Some(sh) = std::env::var_os("SHELL") {
            let p = PathBuf::from(&sh);
            let ok_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| ALLOWED_SHELLS.contains(&n));
            if p.is_absolute() && ok_name && is_executable(&p) {
                return p;
            }
        }
        PathBuf::from(if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" })
    }

    fn is_executable(p: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    /// Run `<shell> -lc <PROBE>` from `cwd` (neutral, so no repo-local rc is sourced) with
    /// the nonce in env; return the captured PATH or None on any failure/timeout. The shell
    /// is a param (no `$SHELL` read, no `set_var`) so this is unit-testable + parallel-safe.
    pub fn run_login_shell(
        shell: &Path,
        cwd: &Path,
        nonce: &str,
        timeout: Duration,
    ) -> Option<String> {
        let child = Command::new(shell)
            .arg("-lc")
            .arg(PROBE)
            .env(NONCE_ENV, nonce)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let (tx, rx) = mpsc::channel();
        // Collect on a thread; bound the wait. On timeout the child is a harmless one-shot
        // (orphaned, no resources we hold) and the app proceeds with the un-augmented PATH.
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(out)) if out.status.success() => {
                extract_path(&String::from_utf8_lossy(&out.stdout), nonce)
            }
            _ => None,
        }
    }

    /// The PATH bracketed by the LAST pair of `nonce` markers (so a banner printed before
    /// the real `printf` can't win). None if not bracketed by two markers or empty between.
    pub fn extract_path(stdout: &str, nonce: &str) -> Option<String> {
        let end = stdout.rfind(nonce)?;
        let start = stdout[..end].rfind(nonce)?;
        let path = &stdout[start + nonce.len()..end];
        (!path.is_empty()).then(|| path.to_string())
    }

    /// Merge `login` dirs ahead of `current`, first-occurrence wins, empty segments dropped.
    pub fn merge_paths(login: &str, current: &str) -> String {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut out: Vec<PathBuf> = Vec::new();
        for p in std::env::split_paths(login).chain(std::env::split_paths(current)) {
            if p.as_os_str().is_empty() {
                continue; // an empty PATH field means cwd — drop it
            }
            if seen.insert(p.clone()) {
                out.push(p);
            }
        }
        std::env::join_paths(out)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|_| current.to_string())
    }
```

- [ ] **Step 4: Wire the call + coupling comments** (the module was registered in Step 1)

In `src-tauri/src/lib.rs`, make it the FIRST statement of `run()` (before `tauri::Builder`):
```rust
pub fn run() {
    // Repair the process PATH from the login shell BEFORE Tauri starts any thread, so
    // bare-name agent/git spawns resolve in a GUI-launched bundle. See services::login_path.
    services::login_path::augment_process_path();
    tauri::Builder::default()
```
Add a one-line note to the top-of-file module docs of `src-tauri/src/services/agent/pty.rs` and `…/sdk.rs`:
```rust
//! Bare-name spawns here rely on the process PATH being augmented at startup
//! (see `services::login_path`) so a GUI-launched bundle can find the binary.
```

- [ ] **Step 5: Run the tests + clippy — expect PASS**

Run: `cd src-tauri && cargo test 2>&1 | tail -12 && cargo clippy --all-targets 2>&1 | tail -4`
Expected: all pass (9 new `login_path` tests green; the rest unaffected). Clippy clean — in particular NO `unused_unsafe` (the `set_var` is a bare call, no `unsafe` block on edition 2021) and no `dead_code` (on this unix host every `imp` fn is used by `augment_process_path` or a test).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/login_path.rs src-tauri/src/services/mod.rs src-tauri/src/lib.rs src-tauri/src/services/agent/pty.rs src-tauri/src/services/agent/sdk.rs
git commit -m "feat(path): augment the process PATH from the login shell at startup"
```

---

### Task 2: Not-found error hints (PTY + SDK)

**Files:**
- Modify: `src-tauri/src/services/agent/pty.rs:52` (+ a new test in its `mod tests`, ~line 87)
- Modify: `src-tauri/src/services/agent/sdk.rs` (`node_spawn_error` ~line 200, the test ~line 442)

- [ ] **Step 1: Write the failing tests**

In `pty.rs`'s `#[cfg(test)] mod tests`, add:
```rust
    #[test]
    fn spawn_missing_program_hints_at_uaw_agent_bin() {
        let err = spawn("uaw-definitely-not-a-real-binary-xyz", &[], &std::env::temp_dir(), &[], 80, 24)
            .expect_err("spawning a nonexistent program must fail");
        assert!(err.contains("UAW_AGENT_BIN"), "got: {err}");
    }
```
In `sdk.rs`, extend the existing `spawn_missing_node_reports_node_not_found` test (the one asserting `contains("Node.js was not found on PATH")`) with one more assertion:
```rust
        assert!(err.contains("UAW_AGENT_NODE"), "got: {err}");
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cd src-tauri && cargo test --lib 'spawn_missing' 2>&1 | tail -12`
Expected: both fail — the current PTY error lacks "UAW_AGENT_BIN"; the current Node message lacks "UAW_AGENT_NODE".

- [ ] **Step 3: Add the hints**

In `pty.rs`, change the spawn `map_err` (line ~52):
```rust
        .map_err(|e| {
            format!(
                "failed to start agent '{program}': {e}\n\
                 If '{program}' is installed, set UAW_AGENT_BIN to its full path."
            )
        })?;
```
In `sdk.rs`, replace the `node_spawn_error` message literal (the `ErrorKind::NotFound` arm):
```rust
        "Node.js was not found on PATH. The SDK agent requires Node.js 18+ — install it or set UAW_AGENT_NODE to its path (PTY agents are unaffected).".to_string()
```

- [ ] **Step 4: Run the suite + clippy — expect PASS**

Run: `cd src-tauri && cargo test 2>&1 | tail -10 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: all pass (the two `spawn_missing_*` tests green; `spawn_missing_node_reports_node_not_found` still satisfies its original `Node.js was not found on PATH` assertion). Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/pty.rs src-tauri/src/services/agent/sdk.rs
git commit -m "feat(agent): point the not-found errors at UAW_AGENT_BIN / UAW_AGENT_NODE"
```

---

### Task 3: Bump the version to 0.1.1

**Files:**
- Modify: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` (+ `src-tauri/Cargo.lock` via build)

- [ ] **Step 1: Bump both manifests**

In `src-tauri/tauri.conf.json`: `"version": "0.1.0"` → `"version": "0.1.1"`.
In `src-tauri/Cargo.toml`: `version = "0.1.0"` → `version = "0.1.1"` (the `[package]` version near the top, NOT a dependency).

- [ ] **Step 2: Regenerate the lockfile + verify they agree**

Run: `cd src-tauri && cargo build 2>&1 | tail -3 && echo "conf=$(jq -r .version tauri.conf.json) cargo=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"`
Expected: build succeeds (updates the `uaw` entry in `Cargo.lock`); prints `conf=0.1.1 cargo=0.1.1` (so the release version-gate will pass for a `v0.1.1` tag).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(release): bump version to 0.1.1"
```

---

## After all tasks

1. **Final whole-branch review** (opus) over `git diff main...HEAD` — the startup ordering (before Tauri threads; no `unsafe` on ed2021), the fail-safe paths, `merge_paths`/`extract_path` correctness, the neutral-CWD + `$SHELL`-allowlist security posture, the error hints, and that nothing touched the override seams or `resolve_program`.
2. **`cargo test` + `cargo clippy --all-targets`** (green) and **`pnpm e2e:docker`** — regression gate (the e2e sets abs-path overrides that bypass PATH; merge-not-replace preserves its inherited PATH, so it must stay green, 12 spec files).
3. **Manual bundle smoke-test — the real proof** (CI can't simulate launchd): `pnpm bundle` → launch the built `.app` **from Finder** (not a terminal) → start a **PTY agent (claude)** AND an **SDK agent** against a bound account → both spawn + produce output. Confirm a deliberately-missing binary shows the `UAW_AGENT_BIN` hint. (Optional regression check: confirm a pre-fix build reproduced "No viable candidates".)
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR. After merge, re-cut the release as **`v0.1.1`** (the v0.1.0 draft can be deleted) and re-run the smoke-test on the real installers before publishing.

---

## Self-Review

**Spec coverage:**
- `augment_process_path()` first in `run()`, before Tauri, no `unsafe` (ed2021) → Task 1 Steps 3-4. ✓
- `-lc` login shell, neutral CWD, stdin null, nonce-via-env, `printf`, last-pair extract → Task 1 Step 3 (`run_login_shell`/`extract_path`/`PROBE`). ✓
- `$SHELL` allowlist + abs+exec validation + `/bin/zsh`|`/bin/sh` fallback → `pick_login_shell`/`is_executable`. ✓
- Simple `recv_timeout` (not the condvar mirror), fail-safe everywhere → `run_login_shell`. ✓
- `merge_paths` via `split_paths`/`join_paths`, first-wins, drop empties, prepend login → Task 1 Step 3 + tests. ✓
- Cut `find_on_path`; PTY hint on the existing `map_err`; SDK message edit → Task 2. ✓
- `#[cfg(unix)]`/`#[cfg(not(unix))]` no-op; module under `services/`; coupling comments → Task 1 Steps 1,4. ✓
- Pure globals-free tests (no `augment_process_path`/`pick_login_shell` unit test — they read `$SHELL`/set PATH); manual smoke as the bundle proof → Task 1 tests + After-tasks 3. ✓
- v0.1.1 ships → Task 3. ✓
- Out of scope (Settings UI, seam changes, Windows, shared-timeout extraction) → no task touches them. ✓

**Placeholder scan:** none — full module, full tests, exact edits, exact commands.

**Type/contract consistency:** `merge_paths(&str,&str)->String`, `extract_path(&str,&str)->Option<String>`, `run_login_shell(&Path,&Path,&str,Duration)->Option<String>`, `is_executable(&Path)->bool`, `pick_login_shell()->PathBuf`, `augment_process_path()` — identical between the Step-1 tests and the Step-3 impl. The nonce env var is `UAW_PATH_NONCE` in both `NONCE_ENV` and `PROBE`. `crate::util::new_id()` (pub, `util.rs:4`) supplies the nonce + the test temp names. The lib.rs call `services::login_path::augment_process_path()` matches the `pub use imp::augment_process_path` / `#[cfg(not(unix))] pub fn` surface.
