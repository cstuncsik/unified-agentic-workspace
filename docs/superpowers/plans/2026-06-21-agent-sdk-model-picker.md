# Claude Agent SDK Per-Session Model Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick, per SDK session, which Claude model runs it — the model list fetched dynamically from the Anthropic API (key stays backend-only), the choice persisted and shown in the run view.

**Architecture:** A dependency-free Node helper (`list-models.mjs`, `GET /v1/models`) is run by a `list_account_models` command via a new race-safe `spawn_oneshot` primitive; a pure `parse_models` maps the JSON to `ModelInfo`. The model threads form → store → `start_agent_session` → sidecar `argv[4]` → `query({ options: { model } })`, persisted in the existing `model_id` column. A dedicated `accountModels` Pinia store caches per account.

**Tech Stack:** Rust (Tauri 2, rusqlite, libc), Node (built-in fetch), Vue 3 + Pinia + TypeScript, WebdriverIO e2e.

**Spec:** `docs/superpowers/specs/2026-06-21-agent-sdk-model-picker-design.md`

Commands: Rust tests `cargo test --manifest-path src-tauri/Cargo.toml`; lint `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`; frontend `pnpm typecheck`; e2e types `pnpm e2e:typecheck`; full e2e `pnpm e2e:docker`; Node syntax `node --check <file>`.

---

## Task 1: `ModelInfo` + `parse_models` (pure)

**Files:** Modify `src-tauri/src/services/agent/sdk.rs`

- [ ] **Step 1: Write failing tests**

In the `#[cfg(test)] mod tests` block of `sdk.rs`, add:

```rust
    #[test]
    fn parse_models_valid() {
        let json = r#"{"data":[{"id":"claude-opus-4-5","display_name":"Claude Opus 4.5"},{"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5"}]}"#;
        let m = parse_models(json).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "claude-opus-4-5");
        assert_eq!(m[0].display_name, "Claude Opus 4.5");
    }
    #[test]
    fn parse_models_empty_data_is_ok_empty() {
        assert!(parse_models(r#"{"data":[]}"#).unwrap().is_empty());
    }
    #[test]
    fn parse_models_error_body_is_err() {
        assert!(parse_models(r#"{"error":{"type":"authentication_error"}}"#).is_err());
    }
    #[test]
    fn parse_models_truncated_is_err() {
        assert!(parse_models(r#"{"data":[{"id":"#).is_err());
    }
    #[test]
    fn parse_models_missing_display_name_falls_back_to_id() {
        let m = parse_models(r#"{"data":[{"id":"m1"}]}"#).unwrap();
        assert_eq!(m[0].display_name, "m1");
    }
    #[test]
    fn parse_models_skips_non_object_elements() {
        let m = parse_models(r#"{"data":[null,42,{"id":"x"}]}"#).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "x");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml parse_models`
Expected: FAIL to compile — `cannot find function parse_models` / `ModelInfo`.

- [ ] **Step 3: Implement**

At the top of `sdk.rs`, add `use serde::Serialize;` (next to the existing `use` lines). Then add after the `redact` function:

```rust
/// One model the user can pick for an SDK session, from the provider's models API.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

/// Parse the Anthropic `/v1/models` body into pickable models. `Ok(vec![])` for an
/// empty `data`; `Err` for a non-`{data}` body (an API error) or malformed JSON.
/// `display_name` falls back to `id`; non-object `data` elements are skipped; never
/// panics. The `Err` value is a fixed, dataless reason — the command maps any `Err`
/// to a fixed opaque string, so the raw body is never surfaced.
pub fn parse_models(stdout: &str) -> Result<Vec<ModelInfo>, String> {
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|_| "parse".to_string())?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "shape".to_string())?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|x| x.as_str())?;
            let display_name = m.get("display_name").and_then(|x| x.as_str()).unwrap_or(id);
            Some(ModelInfo {
                id: id.to_string(),
                display_name: display_name.to_string(),
            })
        })
        .collect())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml parse_models`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs
git commit -m "feat(m10b-2b): parse_models + ModelInfo"
```

---

## Task 2: `spawn_oneshot` (race-safe one-shot subprocess)

**Files:** Modify `src-tauri/src/services/agent/sdk.rs`

- [ ] **Step 1: Write failing tests**

In the test module of `sdk.rs`, add:

```rust
    #[test]
    fn spawn_oneshot_captures_stdout() {
        let out = spawn_oneshot("echo", &["hello"], &std::env::temp_dir(), &[], std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(out.trim(), "hello");
    }
    #[test]
    fn spawn_oneshot_nonzero_exit_is_err() {
        assert!(spawn_oneshot("false", &[], &std::env::temp_dir(), &[], std::time::Duration::from_secs(5)).is_err());
    }
    #[test]
    fn spawn_oneshot_times_out() {
        let r = spawn_oneshot("sleep", &["10"], &std::env::temp_dir(), &[], std::time::Duration::from_millis(50));
        assert!(r.is_err()); // watcher kills the child after 50ms
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml spawn_oneshot`
Expected: FAIL to compile — `cannot find function spawn_oneshot`.

- [ ] **Step 3: Implement**

At the top of `sdk.rs` add `use std::io::Read;`, `use std::sync::{Arc, Mutex};`, `use std::time::Duration;` (alongside the existing `use std::io::BufRead;`). Then add this function (after `spawn`):

```rust
/// Run a short-lived helper, capture all stdout, enforce a wall-clock timeout.
/// Unlike `spawn` (which streams + owns a process group), this is request/response:
/// stderr is discarded, no handle is returned. A watcher thread kills the child after
/// `timeout` — but only while holding the `done` lock and only if the reader hasn't
/// finished, so it can't kill a reused PID. If the kill fires, the result is `Err`
/// regardless of captured stdout. Non-zero exit / spawn failure → `Err`. Every `Err`
/// is the fixed opaque "Failed to list models".
pub fn spawn_oneshot(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<String, String> {
    const ERR: &str = "Failed to list models";
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|_| ERR.to_string())?;
    let mut stdout = child.stdout.take().ok_or_else(|| ERR.to_string())?;
    let pid = child.id();

    let done = Arc::new(Mutex::new(false));
    let killed = Arc::new(Mutex::new(false));
    let watcher = {
        let (done, killed) = (done.clone(), killed.clone());
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            let mut d = done.lock().unwrap();
            if !*d {
                *killed.lock().unwrap() = true;
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                *d = true;
            }
        })
    };

    let mut out = String::new();
    let read_res = stdout.read_to_string(&mut out);
    {
        *done.lock().unwrap() = true;
    }
    let status = child.wait();
    let _ = watcher.join();

    if *killed.lock().unwrap() {
        return Err(ERR.to_string());
    }
    match (read_res, status) {
        (Ok(_), Ok(s)) if s.success() => Ok(out),
        _ => Err(ERR.to_string()),
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml spawn_oneshot`
Expected: PASS (3 tests; the timeout test takes ~50ms).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs
git commit -m "feat(m10b-2b): spawn_oneshot race-safe subprocess with timeout"
```

---

## Task 3: shared resolver + `resolve_sdk_models_sidecar`

**Files:** Modify `src-tauri/src/services/agent/mod.rs`

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` of `mod.rs`, add after `resolve_sdk_sidecar_prefers_env`:

```rust
    #[test]
    fn resolve_sdk_models_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
        assert!(resolve_sdk_models_sidecar().ends_with("list-models.mjs"));
        std::env::set_var("UAW_AGENT_SDK_MODELS", "/tmp/fake-models");
        assert_eq!(resolve_sdk_models_sidecar(), "/tmp/fake-models");
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_sdk_models_sidecar`
Expected: FAIL to compile — `cannot find function resolve_sdk_models_sidecar`.

- [ ] **Step 3: Implement**

In `mod.rs`, replace the existing `resolve_sdk_sidecar` function (and its doc comment) with the factored version:

```rust
/// Resolve a sidecar script path: an env override (trimmed, non-empty) wins; else the
/// bundled relative path made ABSOLUTE against the backend cwd (the child is spawned
/// with cwd=worktree, so a relative program path would resolve there and fail).
fn resolve_sidecar_script(env_var: &str, rel: &str) -> String {
    if let Ok(v) = std::env::var(env_var) {
        if !v.trim().is_empty() {
            return v;
        }
    }
    std::env::current_dir()
        .map(|d| d.join(rel).to_string_lossy().into_owned())
        .unwrap_or_else(|_| rel.to_string())
}

/// The Node sidecar entry for the SDK agent (`UAW_AGENT_SDK_SIDECAR` overrides).
pub fn resolve_sdk_sidecar() -> String {
    resolve_sidecar_script("UAW_AGENT_SDK_SIDECAR", "sidecar/claude-agent-sdk/index.mjs")
}

/// The Node helper that lists a provider's models (`UAW_AGENT_SDK_MODELS` overrides).
pub fn resolve_sdk_models_sidecar() -> String {
    resolve_sidecar_script("UAW_AGENT_SDK_MODELS", "sidecar/claude-agent-sdk/list-models.mjs")
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml resolve_sdk`
Expected: PASS (`resolve_sdk_sidecar_prefers_env` still passes; `resolve_sdk_models_sidecar_prefers_env` passes).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/mod.rs
git commit -m "feat(m10b-2b): factor sidecar resolver + resolve_sdk_models_sidecar"
```

---

## Task 4: thread `model` to the agent sidecar (argv[4]) + persist

**Files:** Modify `src-tauri/src/services/agent/sdk.rs` (spawn + its 3 tests), `src-tauri/src/commands/agent_sessions.rs` (start_agent_session, start_sdk_session)

- [ ] **Step 1: Add `model` to `sdk::spawn`**

In `sdk.rs`, change `spawn`'s signature and the arg chain. Replace the signature line + the `cmd.arg(goal).arg(mode)` line:

```rust
/// Spawn the sidecar as a piped child in `cwd`; goal as argv[2], mode as argv[3],
/// model as argv[4] (empty = SDK default), env injected, stdin null, stderr discarded.
pub fn spawn(
    program: &str,
    goal: &str,
    mode: &str,
    model: &str,
    cwd: &Path,
    env: &[(String, String)],
) -> Result<SdkSpawned, String> {
    let mut cmd = Command::new(program);
    cmd.arg(goal)
        .arg(mode)
        .arg(model)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
```

(Leave the rest of `spawn` unchanged.)

- [ ] **Step 2: Update the three spawn tests**

In `sdk.rs` tests: `spawn_forwards_mode_as_second_arg` — the call + assertion:

```rust
        let mut sp = spawn("echo", "GOAL", "edit", "m1", &dir, &[]).expect("spawn echo");
        // ...
        assert_eq!(out.trim(), "GOAL edit m1");
```

`spawn_injects_env_overriding_inherited` — add an empty model arg:

```rust
        let mut sp = spawn(
            "printenv",
            "UAW_SDK_PROBE",
            "plan",
            "",
            &dir,
            &[("UAW_SDK_PROBE".into(), "INJECTED".into())],
        )
        .expect("spawn printenv");
```

`spawn_missing_program_is_opaque` — add an empty model arg:

```rust
        let err = match spawn("/no/such/sidecar-xyz", "goal", "plan", "", &std::env::temp_dir(), &[]) {
```

- [ ] **Step 3: Thread `model` through the commands**

In `src-tauri/src/commands/agent_sessions.rs`:

(a) `start_agent_session` — add `model: Option<String>` after `mode: Option<String>` in the signature.

(b) In the SDK-branch dispatch, pass the model to `start_sdk_session` after the mode arg:

```rust
            prompt.unwrap_or_default(),
            sdk::normalize_sdk_mode(mode.as_deref()),
            model.as_deref(),
            id,
```

(c) `start_sdk_session` — add `model: Option<&str>` after `mode: &str` in the signature.

(d) In `start_sdk_session`, update the `sdk::spawn` call to pass the model:

```rust
    } = sdk::spawn(&sidecar, &goal, mode, model.unwrap_or(""), Path::new(&worktree_path), &sdk_env)?;
```

(e) In `start_sdk_session`, the `agent_session::create(...)` call currently passes `None` for `model_id` (the 9th arg, after `account_row_id.as_deref()`). Change that `None` to `model`:

```rust
            account_row_id.as_deref(),
            model,
            "sdk",
            Some(mode),
```

- [ ] **Step 4: Build, test, lint**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (the updated spawn tests included).
Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): thread model to the SDK sidecar argv + persist model_id"
```

---

## Task 5: `list_account_models` command

**Files:** Modify `src-tauri/src/commands/agent_sessions.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the command**

In `agent_sessions.rs`, ensure `use std::time::Duration;` is present at the top (add if missing; `use std::path::{Path, PathBuf};` already is). Add the command (near `list_agent_sessions`):

```rust
/// Max wall-clock time the model-list helper may run before it is killed.
const MODELS_TIMEOUT: Duration = std::time::Duration::from_secs(10);

/// List the models the given account can use, by running the dependency-free Node
/// helper with the account's key injected (key never returns to the frontend).
/// Anthropic-only; every failure is a fixed opaque error.
#[tauri::command]
pub fn list_account_models(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    account_id: String,
) -> Result<Vec<sdk::ModelInfo>, String> {
    // Resolve the workspace + workspace-scoped account under one lock, then release
    // before any keychain IO or spawn.
    let account = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        load_session_account(&conn, Some(&account_id), &cw.workspace_id)?
            .ok_or_else(|| "Selected account is not available in this workspace".to_string())?
    };
    // Model listing is Anthropic-only — reject before spawning anything.
    if account.provider != "anthropic" {
        return Err("Model listing is only supported for Anthropic accounts".into());
    }
    // Resolve the key from the keychain (no lock held).
    let key = match keystore::resolve().get(&account.keychain_ref) {
        Ok(Some(k)) => k,
        Ok(None) => return Err("Stored key for this account is missing".into()),
        Err(_) => return Err("Failed to load the account key".into()),
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
    let stdout = sdk::spawn_oneshot(
        &agent::resolve_sdk_models_sidecar(),
        &[],
        &cwd,
        &[("ANTHROPIC_API_KEY".to_string(), key)],
        MODELS_TIMEOUT,
    )?;
    sdk::parse_models(&stdout).map_err(|_| "Failed to list models".to_string())
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, add to the `tauri::generate_handler![...]` list, after `commands::agent_sessions::stop_agent_session,`:

```rust
            commands::agent_sessions::list_account_models,
```

- [ ] **Step 3: Build + lint**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: compiles.
Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: no warnings. (No unit test: the command needs Tauri `State` + spawns a subprocess; it's covered by the e2e in Task 9.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs src-tauri/src/lib.rs
git commit -m "feat(m10b-2b): list_account_models command"
```

---

## Task 6: sidecar — `list-models.mjs` + `index.mjs` model arg

**Files:** Create `sidecar/claude-agent-sdk/list-models.mjs`; Modify `sidecar/claude-agent-sdk/index.mjs`

- [ ] **Step 1: Create the model-list helper**

Create `sidecar/claude-agent-sdk/list-models.mjs`:

```js
#!/usr/bin/env node
// Dependency-free model-list helper for the SDK model picker. Key via env (injected
// by the backend, never argv). On success prints the Anthropic /v1/models JSON to
// stdout; on ANY failure prints nothing to stdout or stderr and exits 1 — the backend
// forwards a fixed opaque error, so the API status/body/endpoint must never surface.
try {
  const res = await fetch("https://api.anthropic.com/v1/models?limit=1000", {
    headers: {
      "x-api-key": process.env.ANTHROPIC_API_KEY ?? "",
      "anthropic-version": "2023-06-01",
    },
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) process.exit(1);
  process.stdout.write(await res.text());
} catch {
  process.exit(1);
}
```

- [ ] **Step 2: Add the model arg to the agent runner**

In `sidecar/claude-agent-sdk/index.mjs`, add `const model = process.argv[4] ?? "";` next to the existing `goal`/`mode`/`cwd` declarations, and add the model spread to the `options` object (before the `hooks` spread):

```js
const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const model = process.argv[4] ?? "";
```

and in `const options = { ... }`:

```js
  env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  ...(model ? { model } : {}),
  ...(mode === "edit" && {
    hooks: { PreToolUse: [{ matcher: "Write|Edit", hooks: [boundToWorktree] }] },
  }),
```

- [ ] **Step 3: Syntax-check both**

Run: `node --check sidecar/claude-agent-sdk/list-models.mjs && node --check sidecar/claude-agent-sdk/index.mjs`
Expected: no output, exit 0.

- [ ] **Step 4: Commit**

```bash
git add sidecar/claude-agent-sdk/list-models.mjs sidecar/claude-agent-sdk/index.mjs
git commit -m "feat(m10b-2b): list-models.mjs helper + sidecar model arg"
```

---

## Task 7: frontend types + api + `accountModels` store (additive)

**Files:** Modify `src/types/agentSession.ts`, `src/api/agentSessions.ts`; Create `src/stores/accountModels.ts`

- [ ] **Step 1: Add the `ModelInfo` type**

In `src/types/agentSession.ts`, add:

```ts
/** A pickable model for an SDK session (from the provider's models API). */
export interface ModelInfo {
  id: string;
  display_name: string;
}
```

- [ ] **Step 2: Add the api call**

In `src/api/agentSessions.ts`, update the import and add the function:

```ts
import type { AgentAdapter, AgentSession, ModelInfo } from "../types/agentSession";
```

```ts
export function listAccountModels(codingWorkspaceId: string, accountId: string): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("list_account_models", { codingWorkspaceId, accountId });
}
```

- [ ] **Step 3: Create the store**

Create `src/stores/accountModels.ts`:

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import type { ModelInfo } from "../types/agentSession";
import * as api from "../api/agentSessions";

/** Per-account model lists, fetched on demand and cached for the app session.
 *  Deliberately NOT in providerAccounts (whose load() clears state per workspace). */
export const useAccountModelsStore = defineStore("accountModels", () => {
  const modelsByAccount = ref<Record<string, ModelInfo[]>>({});
  const loadingByAccount = ref<Record<string, boolean>>({});
  const errorByAccount = ref<Record<string, string | null>>({});
  // Internal guard, not reactive (Set mutations aren't reactive; read only here).
  const inFlight = new Set<string>();

  async function loadModels(codingWorkspaceId: string, accountId: string) {
    if (!accountId || modelsByAccount.value[accountId] || inFlight.has(accountId)) return;
    inFlight.add(accountId);
    loadingByAccount.value = { ...loadingByAccount.value, [accountId]: true };
    errorByAccount.value = { ...errorByAccount.value, [accountId]: null };
    try {
      const models = await api.listAccountModels(codingWorkspaceId, accountId);
      modelsByAccount.value = { ...modelsByAccount.value, [accountId]: models };
    } catch (e) {
      errorByAccount.value = { ...errorByAccount.value, [accountId]: String(e) };
    } finally {
      inFlight.delete(accountId);
      loadingByAccount.value = { ...loadingByAccount.value, [accountId]: false };
    }
  }

  return { modelsByAccount, loadingByAccount, errorByAccount, loadModels };
});
```

- [ ] **Step 4: Typecheck**

Run: `pnpm typecheck`
Expected: PASS (purely additive).

- [ ] **Step 5: Commit**

```bash
git add src/types/agentSession.ts src/api/agentSessions.ts src/stores/accountModels.ts
git commit -m "feat(m10b-2b): accountModels store + listAccountModels api + ModelInfo type"
```

---

## Task 8: model select in the form + run-view header

**Files:** Modify `src/api/agentSessions.ts`, `src/stores/agentSessions.ts`, `src/components/AgentsView.vue`, `src/components/SdkRunView.vue`

- [ ] **Step 1: Thread `model` through api + store**

In `src/api/agentSessions.ts`, update `startAgentSession` (add `model` after `mode`):

```ts
export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  accountId: string | null,
  prompt: string | null,
  mode: string | null,
  model: string | null,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", {
    codingWorkspaceId, adapterId, accountId, prompt, mode, model, cols, rows,
  });
}
```

In `src/stores/agentSessions.ts`, update `start` (add `model` after `mode`):

```ts
  async function start(
    codingWorkspaceId: string,
    adapterId: string,
    accountId: string | null,
    prompt: string | null,
    mode: string | null,
    model: string | null,
    cols: number,
    rows: number,
  ) {
    await ensureListeners();
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, accountId, prompt, mode, model, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }
```

- [ ] **Step 2: AgentsView — store, ref, computeds, watches**

In `src/components/AgentsView.vue` `<script setup>`:

Add the import + store (next to the other store imports):

```ts
import { useAccountModelsStore } from "../stores/accountModels";
```
```ts
const accountModels = useAccountModelsStore();
```

Add the ref (after `const newMode = ref("plan");`):

```ts
const newModelId = ref("");
```

Add computeds (near the other computeds):

```ts
const accountModelOptions = computed(() => accountModels.modelsByAccount[newAccountId.value] ?? []);
const modelsLoading = computed(() => accountModels.loadingByAccount[newAccountId.value] ?? false);
const modelsError = computed(() => accountModels.errorByAccount[newAccountId.value] ?? null);
```

In `watch(newAdapterId, ...)`, add `newModelId.value = "";`:

```ts
watch(newAdapterId, () => {
  newAccountId.value = "";
  newGoal.value = "";
  newMode.value = "plan";
  newModelId.value = "";
});
```

Add a NEW watch on `newAccountId` (after the adapter watch): reset the model synchronously, then fetch:

```ts
// When the account changes, reset the model and lazy-load that account's models.
watch(newAccountId, (val) => {
  newModelId.value = "";
  if (val) accountModels.loadModels(newWorktreeId.value, val);
});
```

In the `workspaces.currentId` watch, add `newModelId.value = "";` alongside `newMode.value = "plan";`:

```ts
    newGoal.value = "";
    newMode.value = "plan";
    newModelId.value = "";
```

- [ ] **Step 3: AgentsView — the select + openTerminal**

In the template, add the model select + error hint immediately after the mode `<select>` (and before the goal textarea):

```html
        <select
          v-if="selectedIsSdk"
          v-model="newModelId"
          class="re-select"
          data-size="sm"
          aria-label="Agent model"
          :disabled="modelsLoading"
        >
          <option value="">{{ modelsLoading ? "Loading models…" : "Default (SDK chooses)" }}</option>
          <option v-for="m in accountModelOptions" :key="m.id" :value="m.id">{{ m.display_name }}</option>
        </select>
        <p v-if="selectedIsSdk && modelsError" class="muted new__hint">
          models unavailable — check your API key
        </p>
```

In `openTerminal`, pass the model after the mode arg:

```ts
    await store.start(
      newWorktreeId.value,
      newAdapterId.value,
      newAccountId.value || null,
      selectedIsSdk.value ? newGoal.value.trim() || null : null,
      selectedIsSdk.value ? newMode.value : null,
      selectedIsSdk.value ? newModelId.value || null : null,
      80,
      24,
    );
```

- [ ] **Step 4: SdkRunView — model header**

In `src/components/SdkRunView.vue`, add a model line as the first child of `.sdk-wrap` (above `.sdk-feed`):

```html
  <div class="sdk-wrap">
    <p class="muted sdk-model" data-testid="sdk-model">Model: {{ session.model_id ?? "Default" }}</p>
    <div class="sdk-feed" data-testid="agent-sdk-feed">
```

Add the style (in the `<style scoped>` block):

```css
.sdk-model {
  margin: 0;
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}
```

- [ ] **Step 5: Typecheck + format**

Run: `pnpm typecheck`
Expected: PASS.
Run: `pnpm format:check` — if it flags the changed files, run `pnpm format` and re-check.

- [ ] **Step 6: Commit**

```bash
git add src/api/agentSessions.ts src/stores/agentSessions.ts src/components/AgentsView.vue src/components/SdkRunView.vue
git commit -m "feat(m10b-2b): model select in the SDK form + model header in the run view"
```

---

## Task 9: e2e — fakes + model-picker scenario

**Files:** Modify `wdio.conf.ts`, `scripts/run-e2e.sh`, `e2e/specs/agent-sdk.e2e.ts`

- [ ] **Step 1: Set the hermetic override**

In `wdio.conf.ts` `beforeSession`, add after the `UAW_AGENT_SDK_SIDECAR` line:

```ts
    process.env.UAW_AGENT_SDK_MODELS = "/tmp/uaw-fake-list-models";
```

- [ ] **Step 2: Build the fake helper + add the MODEL probe to the fake sidecar**

In `scripts/run-e2e.sh`, add a fake model-list helper (after the `uaw-fake-sdk` block):

```bash
# Fake model-list helper for the SDK model-picker e2e: emits canned /v1/models JSON
# (the shape parse_models accepts) and nothing else. No network, no auth needed.
cat >/tmp/uaw-fake-list-models <<'MODELS'
#!/usr/bin/env bash
printf '{"data":[{"id":"claude-opus-4-5","display_name":"Claude Opus 4.5"},{"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5"}]}\n'
MODELS
chmod +x /tmp/uaw-fake-list-models
```

In the existing `/tmp/uaw-fake-sdk` heredoc, add `model="${3:-}"` after the `mode=` line, and emit a JSON model-probe tool event before the result line:

```bash
goal="$1"
mode="${2:-plan}"
model="${3:-}"
km=KEY:unset; [ -n "${ANTHROPIC_API_KEY:-}" ] && km=KEY:set
if [ "$mode" = "edit" ]; then
  printf 'edited by fake sdk\n' > AGENT_EDIT.md
fi
printf '{"type":"assistant","text":"Planning: %s"}\n' "${goal//\"/}"
printf '{"type":"tool","name":"Read","summary":"README.md"}\n'
printf '{"type":"tool","name":"echo","summary":"%s"}\n' "${ANTHROPIC_API_KEY:-none}"
printf 'this line is not json\n'
printf '{"type":"tool","name":"probe","summary":"%s"}\n' "$km"
printf '{"type":"tool","name":"model-probe","summary":"MODEL:%s"}\n' "$model"
printf '{"type":"result","status":"success","summary":"Done"}\n'
```

- [ ] **Step 3: Add the model-picker e2e scenario**

In `e2e/specs/agent-sdk.e2e.ts`, add an all-feeds reader near the existing `feedText` helper (the chosen-model marker is unique, so concatenating all feeds is unambiguous across the mounted tabs):

```ts
const allFeedsText = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="agent-sdk-feed"]')]
      .map((f) => f.textContent ?? "")
      .join("\n"),
  );
```

Add a new `it()` inside the existing `describe("claude agent sdk ...")`, after the edit-mode test:

```ts
  it("lists the account's models and runs the chosen one", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");

    // The model select lazy-loads the account's models (async Node spawn).
    const modelSelect = await $('[aria-label="Agent model"]');
    await browser.waitUntil(async () => (await modelSelect.$$("option")).length > 1, {
      timeout: 15_000,
      timeoutMsg: "model select never populated",
    });
    expect(await modelSelect.getText()).toContain("Default (SDK chooses)");
    await modelSelect.selectByVisibleText("Claude Sonnet 4.5");

    await (await $('[aria-label="Agent goal"]')).setValue("summarize with sonnet");
    await (await $("button*=New terminal")).click();

    // The chosen model id reached the sidecar (argv[4]) → the fake echoed it to the feed.
    await browser.waitUntil(async () => (await allFeedsText()).includes("MODEL:claude-sonnet-4-5"), {
      timeout: 15_000,
      timeoutMsg: "expected the chosen model to reach the sidecar",
    });
  });
```

- [ ] **Step 4: e2e typecheck + shell check**

Run: `pnpm e2e:typecheck`
Expected: PASS.
Run: `bash -n scripts/run-e2e.sh`
Expected: no syntax errors.

- [ ] **Step 5: Full e2e**

Run: `pnpm e2e:docker`
Expected: PASS — all specs green, including the new model-picker scenario (model select populates with the two canned models; the chosen `claude-sonnet-4-5` reaches the sidecar and shows in the feed). The existing plan/edit specs still pass (they launch with no model → `MODEL:` empty, asserted nowhere).

- [ ] **Step 6: Commit**

```bash
git add wdio.conf.ts scripts/run-e2e.sh e2e/specs/agent-sdk.e2e.ts
git commit -m "test(m10b-2b): model-picker e2e + fake list-models helper"
```

---

## Final verification

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — all green.
- [ ] Run `pnpm typecheck`, `pnpm e2e:typecheck`, and `pnpm e2e:docker` — all green.
