# SDK Credential Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the headless SDK agent fail closed to its bound account — isolate `HOME` to a fresh per-session dir so the grandchild Claude Code CLI can't silently fall back to the user's ambient `claude login` when the injected key is bad.

**Architecture:** Two small private helpers in `commands/agent_sessions.rs` — `with_isolated_home` (pure: points the credential-home env vars at a dir) and `create_isolated_home` (fail-closed dir creation, `0700`) — wired into `start_sdk_session` to replace its existing `CLAUDE_CONFIG_DIR`-only push. The SDK sidecar (`index.mjs`) is unchanged: its existing `...process.env` spread carries the isolated `HOME` to the grandchild CLI, and `settingSources: []` already neutralizes the settings/`apiKeyHelper`/repo-local-config paths.

**Tech Stack:** Rust (`std::fs`, `std::os::unix::fs::PermissionsExt`), the existing `account_env_tests` unit module.

---

## File Structure
- `src-tauri/src/commands/agent_sessions.rs` — add `with_isolated_home` + `create_isolated_home` (module-level, above `start_sdk_session`); replace the inline `CLAUDE_CONFIG_DIR` push (lines ~402-410) with a call to both. Tests go in the existing `mod account_env_tests` (`use super::*`, has `new_id`).

No other files change. `sidecar/claude-agent-sdk/index.mjs` is intentionally untouched (see the spec). No migration, no frontend, no new dependency.

---

### Task 1: `with_isolated_home` — point credential-home env vars at a dir (pure, unit-tested)

**Files:**
- Modify: `src-tauri/src/commands/agent_sessions.rs` (new fn above `start_sdk_session` ~line 379; test in `mod account_env_tests` ~line 634)

- [ ] **Step 1: Write the failing test**

Add inside `mod account_env_tests { … }` (after the existing tests):

```rust
    #[test]
    fn isolated_home_points_credential_vars_at_the_dir_and_keeps_the_key() {
        let base = vec![
            ("ANTHROPIC_API_KEY".to_string(), "sk-secret".to_string()),
            ("ANTHROPIC_AUTH_TOKEN".to_string(), String::new()),
        ];
        let out = with_isolated_home(base, "/tmp/uaw-xyz.home");
        // Every credential-home var now points at the isolated dir.
        for k in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "APPDATA", "LOCALAPPDATA"] {
            assert!(
                out.iter().any(|(kk, v)| kk == k && v == "/tmp/uaw-xyz.home"),
                "missing isolated var {k}"
            );
        }
        // The injected key survives unchanged, exactly once, only as ANTHROPIC_API_KEY.
        assert_eq!(
            out.iter()
                .filter(|(_, v)| v == "sk-secret")
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["ANTHROPIC_API_KEY"],
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib isolated_home_points 2>&1 | tail -15`
Expected: FAIL to compile — `cannot find function 'with_isolated_home'`.

- [ ] **Step 3: Write the helper**

Insert immediately above `#[allow(clippy::too_many_arguments)]\nfn start_sdk_session(` (~line 379):

```rust
/// Route the agent's credential-home env at an app-private dir so the grandchild Claude
/// Code CLI cannot reach the user's ambient login (macOS Keychain at $HOME/Library/
/// Keychains, ~/.claude/.credentials.json, %APPDATA% on Windows). CLAUDE_CONFIG_DIR alone
/// does NOT sever the Keychain — HOME does. Each OS reads only the vars it uses; the rest
/// are inert. Appended last so they override any inherited value at spawn time.
fn with_isolated_home(mut env: Vec<(String, String)>, home_dir: &str) -> Vec<(String, String)> {
    for k in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "APPDATA", "LOCALAPPDATA"] {
        env.push((k.to_string(), home_dir.to_string()));
    }
    env
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib isolated_home_points 2>&1 | tail -8`
Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(sdk): with_isolated_home helper (route credential-home env at a dir)"
```

---

### Task 2: `create_isolated_home` (fail-closed dir) + wire both into `start_sdk_session`

**Files:**
- Modify: `src-tauri/src/commands/agent_sessions.rs` (new fn above `start_sdk_session`; replace the `CLAUDE_CONFIG_DIR` push ~lines 402-410; test in `mod account_env_tests`)

- [ ] **Step 1: Write the failing test**

Add inside `mod account_env_tests { … }`:

```rust
    #[test]
    fn create_isolated_home_is_fresh_0700_and_fails_on_reuse() {
        let mut p = std::env::temp_dir();
        p.push(format!("uaw-home-{}.home", new_id()));

        // Fresh path → created as a directory.
        create_isolated_home(&p).expect("fresh dir created");
        assert!(p.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "isolated home must be 0700");
        }

        // Pre-existing path → fail-closed (a reused dir could hold a stale credential).
        assert!(create_isolated_home(&p).is_err());

        let _ = std::fs::remove_dir_all(&p);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib create_isolated_home 2>&1 | tail -15`
Expected: FAIL to compile — `cannot find function 'create_isolated_home'`.

- [ ] **Step 3: Write the helper**

Insert immediately above `with_isolated_home` (from Task 1):

```rust
/// Create the per-session isolated HOME, fail-closed: error if the path already exists or
/// is a symlink — a stale or planted credentials.json in a reused dir could re-leak the
/// ambient login. The session uuid makes the path unguessable; 0700 keeps another local
/// user from reading the CLI-written credential. Errors map to the existing opaque string.
fn create_isolated_home(path: &Path) -> Result<(), String> {
    // create_dir (NOT create_dir_all) fails if the path exists or is a symlink.
    std::fs::create_dir(path).map_err(|_| "Failed to start the agent sidecar".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Failed to start the agent sidecar".to_string())?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib create_isolated_home 2>&1 | tail -8`
Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 5: Wire both helpers into `start_sdk_session`**

In `start_sdk_session`, replace the existing isolation block (the `// Isolate the SDK's own on-disk config…` comment + `let mut sdk_env = env.clone();` + the `sdk_env.push(("CLAUDE_CONFIG_DIR", …with_extension("cfg")…))` push, currently ~lines 402-410):

```rust
    // Isolate the SDK's own on-disk config/session files away from ~/.claude.
    let mut sdk_env = env.clone();
    sdk_env.push((
        "CLAUDE_CONFIG_DIR".to_string(),
        transcript_path
            .with_extension("cfg")
            .to_string_lossy()
            .to_string(),
    ));
```

with:

```rust
    // Isolate the SDK agent's credential resolution: a fresh, app-private HOME so the
    // grandchild Claude Code CLI cannot fall back to the user's ambient `claude login`
    // (macOS Keychain / ~/.claude) when the injected key is bad. CLAUDE_CONFIG_DIR alone
    // does NOT sever the Keychain; HOME does. The sidecar's `...process.env` spread carries
    // these to the grandchild; `settingSources: []` already blocks the settings paths.
    let isolated_home = transcript_path.with_extension("home");
    create_isolated_home(&isolated_home)?;
    let sdk_env = with_isolated_home(env.clone(), &isolated_home.to_string_lossy());
```

(The `injected_key` computed just above from `env` is unaffected — `with_isolated_home` only appends dir vars. `sdk_env` is no longer `mut`. The `sdk::spawn(&sidecar, &goal, mode, …, &sdk_env)?` call just below is unchanged.)

- [ ] **Step 6: Run the full crate test suite + clippy**

Run: `cd src-tauri && cargo test 2>&1 | tail -12 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: all tests pass (the new 2 + the existing suite — `start_sdk_session` callers compile unchanged); clippy clean (the repo self-imposes `-D warnings`; fix any warning the change introduces, e.g. an unused `mut`).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(sdk): isolate HOME per session so a bad key fails closed (no ambient fallback)"
```

---

## After both tasks

1. **Final whole-branch review** (opus): review `git diff main...HEAD` — confirm the two helpers, the `start_sdk_session` wiring (the `.cfg` push fully replaced; `injected_key` mask intact; `sdk_env` feeds `sdk::spawn`), that `index.mjs` is untouched (the spread carries the isolated HOME), the fail-closed dir creation, and that the spec's security boundary holds (HOME severs the Keychain; the deferred edges are documented, not silently dropped).
2. **Docker e2e** (`pnpm e2e:docker`) — the regression gate. The SDK e2e drives a **fake** sidecar via `UAW_AGENT_SDK_SIDECAR` (the fake ignores HOME) and a file-backed keystore via `UAW_KEYSTORE_DIR`, so the isolation change doesn't alter it; the agent-sdk specs must stay green (12/12). Proves no regression.
3. **Manual real-key check (product owner — the real fail-closed proof):**
   - **Negative:** bind an account whose key is **revoked/garbage**, launch an SDK agent → it **errors** ("Invalid API key"), and does **not** stream a successful turn as your ambient `claude login`.
   - **Positive:** a **valid** bound key → the SDK agent runs and authenticates as that account.
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR.

---

## Self-Review

**Spec coverage:**
- "Isolate HOME (+ home-equiv vars) to a fresh per-session dir in `start_sdk_session`" → Task 1 (`with_isolated_home`, the 5 vars) + Task 2 Step 5 (wiring). ✓
- "`create_dir` fail-closed + `0700`, not `create_dir_all`" → Task 2 (`create_isolated_home` + its test). ✓
- "Unconditional (no flag)" → Task 2 Step 5 calls the helpers directly, no conditional. ✓
- "`index.mjs` unchanged" → File Structure + the wiring comment; no task touches it. ✓
- "Key mask unaffected" → Task 2 Step 5 note (computed from `env` before the append). ✓
- Verification (unit + manual negative/positive + e2e regression) → the two task tests + the After-both-tasks section. ✓
- Deferred items (PTY, the flag, the broader env scrub) → no task touches them. ✓

**Placeholder scan:** none — every step has full code/commands. No "handle edge cases"/TBD.

**Type/contract consistency:** `with_isolated_home(Vec<(String,String)>, &str) -> Vec<(String,String)>` and `create_isolated_home(&Path) -> Result<(), String>` are used in Task 2 Step 5 exactly as defined in Tasks 1/2 Step 3. The 5 var names are identical across the helper, its test, and the spec. The error string matches the existing opaque `"Failed to start the agent sidecar"`. `transcript_path.with_extension("home")` mirrors the existing `.with_extension("cfg")` idiom (uuid has no dots).
