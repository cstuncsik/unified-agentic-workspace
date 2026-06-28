# Packaging Slice A — Bundle the SDK Sidecar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A locally-built app's SDK agent finds + runs its bundled Node sidecar (today the script resolves against `current_dir()`, which is the launch dir in a packaged app → not found).

**Architecture:** Bundle `sidecar/claude-agent-sdk` via `bundle.resources` (map form), install its deps via a `bundle` wrapper script (not `beforeBuildCommand`), and resolve the script with a dev/release split — `env → (dev: cwd | release: resource-dir-only, fail-closed)` — via a pure unit-testable seam.

**Tech Stack:** Tauri v2 (`bundle.resources`, `resource_dir()`), Rust (`services/agent/mod.rs` resolver), npm (the sidecar lockfile + `npm ci`).

---

## File Structure
- `src-tauri/src/services/agent/mod.rs` — the resolver seam (`resolve_sidecar_script` gains `resource_dir`/`dev`; the two `resolve_sdk_*` wrappers pass them) + its tests.
- `src-tauri/src/commands/agent_sessions.rs` — `start_sdk_session` + `list_account_models` (gains an injected `AppHandle`) pass the resource dir.
- `src-tauri/tauri.conf.json` — `bundle.resources`.
- `package.json` — `sidecar:install` + `bundle` scripts.
- `sidecar/claude-agent-sdk/.gitignore` + `package-lock.json` — un-ignore + commit the lockfile.

**Task ordering:** Task 1 (Rust) changes the resolver signature, so it bundles the wrappers + both callers + all tests into one atomic commit. Task 2 (config + lockfile) is independent. The manual local-build proof + the Docker e2e regression gate run after both.

---

### Task 1: Resolve from `resource_dir` (dev/release split) — one atomic commit

**Files:**
- Modify: `src-tauri/src/services/agent/mod.rs` (`resolve_sidecar_script` ~130, the two wrappers ~143/149, the `prefers_env` tests ~191/200, a new test in `mod tests` ~153)
- Modify: `src-tauri/src/commands/agent_sessions.rs` (`list_account_models` ~92, `start_sdk_session` call ~424)

- [ ] **Step 1: Update the two `prefers_env` tests + add the precedence test (test-first — they won't compile against the old signature)**

In `mod.rs` `mod tests`, change `resolve_sdk_sidecar()` → `resolve_sdk_sidecar(None)` and `resolve_sdk_models_sidecar()` → `resolve_sdk_models_sidecar(None)` in the two existing `*_prefers_env` tests, and add:

```rust
    #[test]
    fn resolve_sidecar_script_precedence() {
        use std::fs;
        let rel = "sidecar/claude-agent-sdk/index.mjs";
        let env_var = "UAW_TEST_SIDECAR_PREC"; // unique name -> no shared-var race
        std::env::remove_var(env_var);

        // A resource dir WITH the script present.
        let res = std::env::temp_dir().join(format!("uaw-res-{}", crate::util::new_id()));
        fs::create_dir_all(res.join("sidecar/claude-agent-sdk")).unwrap();
        fs::write(res.join(rel), b"").unwrap();
        let res_str = res.to_string_lossy().into_owned();

        // release + Some(dir) with the file -> the resource path (FULL path, contains the dir).
        let r = resolve_sidecar_script(env_var, rel, Some(&res), false);
        assert!(r.contains(&res_str), "release should use the resource dir: {r}");
        assert!(r.ends_with("index.mjs"));

        // release + None -> a non-cwd sentinel (fail closed, never the worktree cwd).
        let r = resolve_sidecar_script(env_var, rel, None, false);
        assert!(!r.contains(&res_str));
        assert!(r.starts_with("/nonexistent/"), "release+no-resource must fail closed: {r}");

        // dev -> cwd, ignores the resource dir.
        let r = resolve_sidecar_script(env_var, rel, Some(&res), true);
        assert!(!r.contains(&res_str), "dev must use cwd, not the resource dir: {r}");
        assert!(r.ends_with("index.mjs"));

        // env override wins over a present resource, in both modes.
        std::env::set_var(env_var, "/tmp/override.mjs");
        assert_eq!(resolve_sidecar_script(env_var, rel, Some(&res), false), "/tmp/override.mjs");
        assert_eq!(resolve_sidecar_script(env_var, rel, Some(&res), true), "/tmp/override.mjs");
        std::env::remove_var(env_var);

        let _ = fs::remove_dir_all(&res);
    }
```

- [ ] **Step 2: Run — expect a COMPILE failure (the resolver still has the old 2-arg signature)**

Run: `cd src-tauri && cargo test --lib resolve_sidecar 2>&1 | tail -15`
Expected: compile error — `resolve_sidecar_script` takes 2 args / `resolve_sdk_sidecar` takes 0.

- [ ] **Step 3: Add the `Path` import + rewrite the resolver and wrappers**

In `mod.rs`, ensure `use std::path::Path;` is present at the top (add it if absent). Replace `resolve_sidecar_script` (the current 2-arg version) with:

```rust
/// Resolve a sidecar script path. Precedence: an env override (trimmed, non-empty) wins;
/// then in DEV the repo sidecar via cwd (its node_modules is the working-tree install);
/// in RELEASE the bundled resource ONLY — never cwd (the agent-writable worktree, a
/// script-hijack/key-exfil vector). A missing release resource -> a non-existent path ->
/// spawn fails closed (the post-build assertion guarantees a correctly-bundled app has it).
fn resolve_sidecar_script(env_var: &str, rel: &str, resource_dir: Option<&Path>, dev: bool) -> String {
    if let Ok(v) = std::env::var(env_var) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if dev {
        return std::env::current_dir()
            .map(|d| d.join(rel).to_string_lossy().into_owned())
            .unwrap_or_else(|_| rel.to_string());
    }
    resource_dir
        .map(|d| d.join(rel).to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("/nonexistent/uaw-bundled-sidecar/{rel}"))
}
```

Replace the two wrappers with (they pass `cfg!(debug_assertions)` as `dev`):

```rust
/// The Node sidecar entry for the SDK agent (`UAW_AGENT_SDK_SIDECAR` overrides).
pub fn resolve_sdk_sidecar(resource_dir: Option<&Path>) -> String {
    resolve_sidecar_script(
        "UAW_AGENT_SDK_SIDECAR",
        "sidecar/claude-agent-sdk/index.mjs",
        resource_dir,
        cfg!(debug_assertions),
    )
}

/// The Node helper that lists a provider's models (`UAW_AGENT_SDK_MODELS` overrides).
pub fn resolve_sdk_models_sidecar(resource_dir: Option<&Path>) -> String {
    resolve_sidecar_script(
        "UAW_AGENT_SDK_MODELS",
        "sidecar/claude-agent-sdk/list-models.mjs",
        resource_dir,
        cfg!(debug_assertions),
    )
}
```

- [ ] **Step 4: Update the two callers in `agent_sessions.rs`**

`start_sdk_session` (~line 424): change `let sidecar = agent::resolve_sdk_sidecar();` to:
```rust
    let sidecar = agent::resolve_sdk_sidecar(app.path().resource_dir().ok().as_deref());
```
(`app: AppHandle` is already in scope; `Manager` is already imported.)

`list_account_models` (~line 92): add `app: AppHandle` as the **first** parameter:
```rust
pub fn list_account_models(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    account_id: String,
) -> Result<Vec<sdk::ModelInfo>, String> {
```
and change the call (~line 122) `&agent::resolve_sdk_models_sidecar(),` to:
```rust
        &agent::resolve_sdk_models_sidecar(app.path().resource_dir().ok().as_deref()),
```
(Leave the `cwd = current_dir()` at line 120 unchanged — it's the helper's working dir, orthogonal to script resolution.)

- [ ] **Step 5: Run the full crate suite + clippy**

Run: `cd src-tauri && cargo test 2>&1 | tail -12 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: all pass (the precedence test + the updated `prefers_env` tests green; the `list_account_models` frontend invoke is unchanged — Tauri injects `AppHandle` by type); clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/agent/mod.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(packaging): resolve the SDK sidecar from resource_dir (dev/release split, fail-closed)"
```

---

### Task 2: Bundle the sidecar + commit the lockfile — one commit

**Files:**
- Modify: `src-tauri/tauri.conf.json` (bundle block ~26)
- Modify: `package.json` (scripts ~14-16)
- Modify: `sidecar/claude-agent-sdk/.gitignore`
- Add: `sidecar/claude-agent-sdk/package-lock.json`

- [ ] **Step 1: Add `bundle.resources` (map form) to `tauri.conf.json`**

In the `"bundle"` object (which has `"targets": "all"`), add a `resources` key:
```json
  "bundle": {
    "active": true,
    "targets": "all",
    "resources": {
      "../sidecar/claude-agent-sdk": "sidecar/claude-agent-sdk"
    },
```
(Keep the existing keys; just insert `resources`. The source `../sidecar/...` is resolved from `src-tauri/` where this file lives → repo-root `sidecar/`; it lands at `<resource_dir>/sidecar/claude-agent-sdk/`.)

- [ ] **Step 2: Add the `sidecar:install` + `bundle` scripts to `package.json`**

In `"scripts"`, add:
```json
    "sidecar:install": "npm ci --ignore-scripts --omit=optional --prefix sidecar/claude-agent-sdk",
    "bundle": "pnpm sidecar:install && pnpm tauri build",
```
(`--prefix` makes it cwd-independent; `--ignore-scripts` closes the lifecycle-RCE surface; `--omit=optional` drops the native sharp. `bundle` is the bundling entry point; `beforeBuildCommand` stays `"pnpm build"` so the `--no-bundle` e2e build is untouched.)

- [ ] **Step 3: Un-ignore the lockfile + regenerate it cleanly**

Remove the `package-lock.json` line from `sidecar/claude-agent-sdk/.gitignore` (keep `node_modules/`). Then regenerate the lockfile so it records all platforms (incl. the omitted sharp, for full integrity):
```bash
cd sidecar/claude-agent-sdk && rm -f package-lock.json && npm install --package-lock-only && cd -
```
Expected: `package-lock.json` written (no `node_modules` install), with `integrity` sha512 on every entry.

- [ ] **Step 4: Verify `sidecar:install` works against the committed lockfile**

Run: `cd /Users/csaba/projects/unified-agentic-workspace && rm -rf sidecar/claude-agent-sdk/node_modules && pnpm sidecar:install 2>&1 | tail -5 && test -f sidecar/claude-agent-sdk/node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs && echo "OK: sidecar installed, sharp omitted" && test ! -d sidecar/claude-agent-sdk/node_modules/@img && echo "OK: no @img/sharp"`
Expected: `npm ci` succeeds from the lockfile; `sdk.mjs` present; no `@img/sharp-*` (omitted). `git status` shows `package-lock.json` staged, `node_modules/` still ignored.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json package.json sidecar/claude-agent-sdk/.gitignore sidecar/claude-agent-sdk/package-lock.json
git commit -m "feat(packaging): bundle the SDK sidecar (bundle.resources + committed lockfile + install script)"
```

---

## After both tasks

1. **Final whole-branch review** (opus): review `git diff main...HEAD` — the dev/release precedence split (release never uses cwd; the `cfg!(debug_assertions)` gate), the `bundle.resources` map form + the source/dest cwd reasoning, the `list_account_models` `AppHandle` (frontend-invisible), the committed lockfile (`--omit=optional` at install, all-platforms in the lock), and that the Docker e2e is unaffected (env override + `--no-bundle`).
2. **Docker e2e** (`pnpm e2e:docker`) — the regression gate. It sets `UAW_AGENT_SDK_SIDECAR`/`_MODELS` (precedence 1) + builds `--no-bundle`, so the resolver change is invisible to it; the agent-sdk specs (incl. the model picker at `agent-sdk.e2e.ts:165`, which proves the new `AppHandle` injection is frontend-invisible) must stay green (12 spec files).
3. **Manual local-build proof — the real proof of the bundled path** (run on a CLEAN tree so the dev `node_modules` can't mask a broken bundle):
   - `rm -rf sidecar/claude-agent-sdk/node_modules` (clean tree).
   - `pnpm bundle` (installs the sidecar deps, then `tauri build`).
   - **Post-build presence assertion:** `test -f src-tauri/target/release/bundle/macos/UAW.app/Contents/Resources/sidecar/claude-agent-sdk/node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs` → must exist (else the bundle is incomplete — fail loudly). *(Slice B's CI generalizes this per-OS.)*
   - `unset UAW_AGENT_SDK_SIDECAR UAW_AGENT_SDK_MODELS`, then launch the bundled app from a **neutral cwd**: `cd /tmp && open /Users/csaba/projects/unified-agentic-workspace/src-tauri/target/release/bundle/macos/UAW.app`.
   - In the app: (a) start an **SDK agent (plan mode)** against a bound Anthropic account + a worktree → the feed shows an assistant/result row (proves `index.mjs` was found in `resource_dir`, `node` spawned it, and `node_modules` resolved); (b) the **model picker populates** (proves `list-models.mjs` + the `list_account_models` `AppHandle`).
   - A quick `tauri dev` smoke (start an SDK session) confirms no dev regression (dev uses cwd, unchanged).
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR.

---

## Self-Review

**Spec coverage:**
- `bundle.resources` (map form) → Task 2 Step 1. ✓
- Install via a `bundle` wrapper (not `beforeBuildCommand`) + `--prefix` → Task 2 Step 2. ✓
- Resolve from `resource_dir`, dev/release split, reuse `rel`, `Option<&Path>` seam → Task 1 Steps 3-4. ✓
- `list_account_models` gains `AppHandle`; its cwd left alone → Task 1 Step 4. ✓
- Commit the lockfile (`--package-lock-only`, un-ignore) → Task 2 Steps 3-4. ✓
- Precedence unit test (full paths, `dev` param); existing `prefers_env` updated → Task 1 Steps 1,3. ✓
- Post-build presence assertion + the 4-point clean-tree manual proof → After-tasks 3. ✓
- e2e blind-but-unaffected; `agent-sdk.e2e.ts:165` named → After-tasks 2. ✓
- Out of scope (Slice B: universal-mac, `--bundles`, the pipeline, the ~40MB ripgrep prune) → no task touches them. ✓

**Placeholder scan:** none — every code step has full content; every command has an expected result.

**Type/contract consistency:** `resolve_sidecar_script(&str, &str, Option<&Path>, bool) -> String` is used identically in the wrappers (which pass `cfg!(debug_assertions)`) and the precedence test. `resolve_sdk_sidecar(Option<&Path>)` / `resolve_sdk_models_sidecar(Option<&Path>)` are called with `app.path().resource_dir().ok().as_deref()` at both call sites and `None` in the `prefers_env` tests. `list_account_models`'s new `app: AppHandle` first param matches the `start_agent_session` pattern; the frontend `invoke("list_account_models", { codingWorkspaceId, accountId })` is unchanged. The resource destination `sidecar/claude-agent-sdk` (tauri.conf) mirrors the source tree so `resource_dir.join(rel)` (with `rel = "sidecar/claude-agent-sdk/index.mjs"`) reproduces the bundled layout.
