# M10b-2b slice 3 — per-session model picker (dynamic)

**Status:** design (spec)
**Branch:** `cstuncsik/milestone-10b-2b-slice3`
**Builds on:** slices 1+2 (`2026-06-19-agent-sdk-sidecar-design.md`, `2026-06-20-agent-sdk-edit-mode-design.md`)

## Goal

Let the user choose, per session, **which Claude model** the headless SDK agent runs
(cost vs capability: Opus / Sonnet / Haiku). The model list is fetched **dynamically**
from the Anthropic API so it's always current. The chosen model is persisted and shown
in the run view; absent → the SDK's own default.

**Done when:** in an SDK launch, selecting an account loads that account's models from the
API; picking one and launching runs the agent on it and `SdkRunView` shows the chosen
model; the frontend never receives a raw key; the full e2e is green (hermetic, offline).

## Scope

**In scope**
- A `sidecar/claude-agent-sdk/list-models.mjs` helper (Node built-in `fetch`, **no npm
  deps**) that `GET`s `/v1/models` with the env key and prints the model JSON.
- A `list_account_models(coding_workspace_id, account_id)` command that runs the helper
  with the account's key (resolved backend-only, workspace-scoped) and returns
  `Vec<ModelInfo{ id, display_name }>` — the key never reaches the frontend.
- A model `<select>` in the SDK launch form (SDK-only): **"Default (SDK chooses)"** +
  the fetched models, lazy-fetched per account + cached, with loading/error states.
- The chosen model threaded to the agent sidecar (`query({ options: { model } })`) and
  persisted in the existing `agent_sessions.model_id` column.
- The model shown in `SdkRunView` so the picker is not write-only.

**Out of scope (later / deferred)**
- Cost/capability **tier labels** (Fast/Balanced/Capable) — would reintroduce a hardcoded
  model→tier map; revisit after this slice.
- **OpenAI/other providers** — the picker is SDK-only and the SDK adapter is
  Anthropic-only; OpenAI's models endpoint needs different auth. The provider gate + a
  documented extension point are in scope; the OpenAI helper is not.
- Model picker for PTY adapters (they pick models interactively in-terminal).
- Persisting the model cache across app restarts (in-memory for the session).
- Pagination beyond a single `limit=1000` page.
- Model in the agent tab label (it's shown inside the run view).

---

## Architecture

### 1. Sidecar — `list-models.mjs` (new) + `index.mjs` (model arg)

**`list-models.mjs`** — a standalone, dependency-free Node script:
- Reads the key from `process.env.ANTHROPIC_API_KEY` (never argv).
- `fetch("https://api.anthropic.com/v1/models?limit=1000", { headers: { "x-api-key": key, "anthropic-version": "2023-06-01" }, signal: AbortSignal.timeout(10_000) })`.
  - `anthropic-version` is **required** (omitting it → HTTP 400). `limit=1000` avoids the
    default-20 truncation (one page covers the model list).
- On a 2xx: print the response body (`{ "data": [ { "id", "display_name", ... } ] }` JSON)
  to **stdout**, exit 0.
- On any non-2xx, fetch throw, or timeout: print **nothing** to stdout **and nothing to
  stderr** (the failing status/body/URL must never surface — Rust forwards an opaque
  error), `process.exit(1)`. Mirrors `index.mjs`'s fixed-error catch.

**`index.mjs`** (agent runner) gains the model: `const model = process.argv[4] ?? ""` and
an explicit ternary (NOT `&&`):
```js
const options = { cwd, permissionMode: "dontAsk", allowedTools, settingSources: [],
  maxTurns: 30, env: {...}, ...(model ? { model } : {}),
  ...(mode === "edit" && { hooks: {...} }) };
```
Empty model → no `model` key → SDK default.

### 2. Backend

- **Shared resolver** in `services/agent/mod.rs` — factor the env-or-absolute logic so the
  two resolvers can't drift; the `!v.trim().is_empty()` guard is preserved:
  ```rust
  fn resolve_sidecar_script(env_var: &str, rel: &str) -> String {
      if let Ok(v) = std::env::var(env_var) { if !v.trim().is_empty() { return v; } }
      std::env::current_dir().map(|d| d.join(rel).to_string_lossy().into_owned()).unwrap_or_else(|_| rel.to_string())
  }
  pub fn resolve_sdk_sidecar() -> String { resolve_sidecar_script("UAW_AGENT_SDK_SIDECAR", "sidecar/claude-agent-sdk/index.mjs") }
  pub fn resolve_sdk_models_sidecar() -> String { resolve_sidecar_script("UAW_AGENT_SDK_MODELS", "sidecar/claude-agent-sdk/list-models.mjs") }
  ```
  Keeps the existing `UAW_AGENT_SDK_SIDECAR` (no rename); adds `UAW_AGENT_SDK_MODELS`.
  `resolve_sdk_sidecar_prefers_env` still passes; add a mirror test for the models resolver.
- **`sdk::spawn_oneshot(program, args: &[&str], cwd: &Path, env: &[(String,String)], timeout: Duration) -> Result<String, String>`**
  (new, distinct from the streaming `spawn`): spawns a piped child (stdin null, stdout
  piped, **stderr `Stdio::null()`**). A watcher thread sleeps `timeout` then kills the
  child **only if** a shared `done: Arc<Mutex<bool>>` flag is still false (the reader sets
  it after `read_to_string` returns — this prevents killing a reused PID). If the watcher
  fired the kill, the function returns `Err` **regardless of captured stdout** (the
  response is treated as incomplete). Non-zero exit / spawn failure → `Err`. All `Err`
  values are the fixed opaque `"Failed to list models"`. `MODELS_TIMEOUT = Duration::from_secs(10)`.
- **`sdk::parse_models(stdout: &str) -> Result<Vec<ModelInfo>, String>`** (pure,
  unit-tested): parse `{ data: [ { id, display_name? } ] }`; `display_name` falls back to
  `id` when absent; **skip non-object `data` elements** (never panic); `Ok(vec![])` for
  `{ data: [] }`; `Err(_)` for a non-`{data}` body (an API error) or malformed/truncated
  JSON. The `Err` value is **dataless** (a fixed reason; the raw body is never embedded)
  and is discarded by the command, which substitutes `"Failed to list models"`.
  `ModelInfo { id: String, display_name: String }` derives `Debug, Clone, Serialize`
  (snake_case fields, matching the codebase; lives in `sdk.rs`, `pub`).
- **`list_account_models(state, coding_workspace_id: String, account_id: String) -> Result<Vec<ModelInfo>, String>`**
  command:
  - Under **one DB lock**: `coding_workspace::get(coding_workspace_id)` → its `workspace_id`;
    `load_session_account(&conn, Some(&account_id), &cw.workspace_id)` (workspace-scoped,
    fixed opaque error if cross-workspace/missing). **Release the lock.**
  - Read the account's key from the keychain (`keystore::resolve().get(&account.keychain_ref)`)
    — **no lock held** (keychain IO must not hold the SQLite lock), like `resolve_session_env`.
  - Reject non-Anthropic providers with a fixed error **before any spawn**.
  - `spawn_oneshot(&resolve_sdk_models_sidecar(), &[], &cwd, &[("ANTHROPIC_API_KEY".into(), key)], MODELS_TIMEOUT)`
    where `cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))` (the
    helper does network only; cwd is irrelevant) → `parse_models` → `map_err(|_| "Failed to list models")`.
  - Register in `tauri::generate_handler!` (`src-tauri/src/lib.rs`).
- **Model threading:** `start_agent_session`/`start_sdk_session` gain `model: Option<String>`;
  the SDK branch persists it via the existing `model_id` column
  (`agent_session::create(..., model.as_deref(), "sdk")` — no migration) and passes it to
  `sdk::spawn(&sidecar, &goal, mode, model.as_deref().unwrap_or(""), &cwd, &env)` → `argv[4]`.
  `sdk::spawn` gains `model: &str` after `mode`. Model is a non-secret free-form id from our
  own fetched list, passed as a literal argv via `Command::arg` (no shell) — no injection.
  PTY path passes `None`/`""` (unaffected). **All three existing spawn tests** must add the
  arg: `spawn_forwards_mode_as_second_arg` (assert `"GOAL edit m1"`),
  `spawn_injects_env_overriding_inherited` (pass `""`), `spawn_missing_program_is_opaque`.

### 3. Frontend

- **`stores/accountModels.ts`** (new, dedicated — NOT `providerAccounts`, whose `load()`
  clears state per workspace and would evict the cache on every workspace switch):
  reactive `modelsByAccount: Record<id, ModelInfo[]>`, `loadingByAccount: Record<id, boolean>`,
  `errorByAccount: Record<id, string | null>`, and `loadModels(codingWorkspaceId, accountId)`
  — **all returned from the store setup** (mirrors `providerAccounts.ts`). The in-flight
  guard is a **plain `const inFlight = new Set<string>()`** (NOT a `ref` — Set mutations
  aren't reactive; it's internal guard state only). `loadModels` early-returns on `""`, a
  cache hit, or an in-flight account; writes are keyed by `accountId` so a stale response
  can't cross-contaminate. Cache lives for the app session.
- **`AgentsView`**: a model `<select aria-label="Agent model">` (`v-if="selectedIsSdk"`),
  options = **"Default (SDK chooses)"** (value `""`) + the account's fetched models (option
  value = `id`, text = `display_name`). States: loading → `:disabled` "Loading models…";
  error → Default-only + a `<p class="muted new__hint">models unavailable — check your API
  key</p>`; loaded → Default + models. A `newModelId` ref (default `""`).
  - Reset `newModelId = ""` in **`watch(newAdapterId)`** (directly, alongside `newAccountId`
    — don't rely on the async adapter→account cascade), in **`watch(newAccountId)`**
    (synchronously, then `if (val) accountModels.loadModels(newWorktreeId.value, val)`), and
    in the **`workspaces.currentId`** watch (alongside `newMode`).
  - `canStart` is **unchanged** — it must NOT reference `newModelId` ("Default" is always a
    valid launch).
  - `openTerminal` passes `selectedIsSdk ? newModelId.value || null : null` to `store.start`
    (`|| null` maps `""` → null).
- **`store.start`/`api.startAgentSession`** gain `model: string | null` (positional after
  `mode`, before `cols`, for the TS call chain); the Tauri invoke payload adds `model`
  (named — Tauri maps camelCase `model` → Rust `model`, so Rust param order is irrelevant);
  PTY passes `null`.
- **`SdkRunView`**: a `<p class="muted">Model: {{ session.model_id ?? "Default" }}</p>`
  above the feed (font-size matching the existing muted status text). The component already
  receives `session`; it's only rendered for `kind === "sdk"`, so no extra guard.

---

## Security analysis

Carries over slices 1/2 invariants (key in OS keychain by `provider:account_id`; frontend
never sees a raw key; fixed opaque backend errors rendered verbatim by the UI). New path:

1. **Key handling:** keychain → `spawn_oneshot` subprocess **env only** → `list-models.mjs`
   reads `process.env` (not argv/file/stdin), uses it in the request header, never writes
   it. The account **row** is loaded under the DB lock; the **key** is read from the keychain
   **after** the lock is released (no keychain IO under the SQLite lock). The `/v1/models`
   response cannot echo the key. The command returns only `{id, display_name}` — no key
   field. `parse_models`'s `Err` is dataless and is discarded → fixed string at the boundary,
   so a malformed body can't surface.
2. **Error opacity:** on any failure the helper prints nothing to stdout or stderr and exits
   1; `spawn_oneshot` drops stderr (`Stdio::null()`) and maps non-zero/kill/spawn-fail to the
   fixed `"Failed to list models"`. The API's status/body/endpoint never reach the frontend.
3. **Account scoping + provider gate:** `load_session_account` enforces workspace scope
   (existing, tested) using the `workspace_id` derived from the `coding_workspace_id` arg; a
   non-Anthropic account is rejected with a fixed error before any spawn.
4. **Model value:** non-secret; literal argv (no shell); empty → omitted; a crafted id just
   yields a benign SDK 400. Endpoint hardcoded `https://api.anthropic.com` (TLS verified by
   default Node fetch; not disabled) — no SSRF / no configurable base URL.
   `UAW_AGENT_SDK_MODELS` is a dev/e2e script-path override with the same threat model as the
   existing `UAW_AGENT_SDK_SIDECAR` (documented; not set in production).

---

## Known limitations (documented)

- The model cache is **in-memory** (per app session); a restart re-fetches on first
  account-select (a brief "Loading models…").
- A persisted `model_id` may name a later-**deprecated** model; a re-run then fails at the
  SDK with that id in the error — acceptable, surfaced via the run's error event.
- No **cost/capability signal** yet (bare display names); tier labels deferred.
- **Anthropic-only**; OpenAI/other providers need their own helper (extension point: the
  provider gate).
- The model is shown **inside the run view**, not in the agent tab label.

---

## Testing strategy

**Rust unit**
- `parse_models`: valid 2-model → 2 `ModelInfo`; `{data:[]}` → empty `Ok`; an error body
  `{"error":{...}}` → `Err`; truncated JSON → `Err`; a `data` element missing `display_name`
  → `display_name == id`; a non-object `data` element (`{"data":[null,{"id":"x"}]}`) → that
  element skipped, no panic.
- `resolve_sdk_models_sidecar` mirrors `resolve_sdk_sidecar_prefers_env` (env override vs
  absolute fallback).
- `spawn_oneshot`: `echo hello` → `Ok("hello\n")`; `false` → `Err`; **timeout**:
  `spawn_oneshot("sleep", &["10"], .., Duration::from_millis(50))` → `Err` (deterministic
  ~50ms, proves the kill path; not "covered by review").
- Update **all three** `spawn`-calling tests for the new `model: &str` arg (see Backend).

**e2e (hermetic — no real network)**
- `wdio.conf.ts beforeSession` adds `process.env.UAW_AGENT_SDK_MODELS = "/tmp/uaw-fake-list-models"`
  (next to `UAW_AGENT_SDK_SIDECAR`).
- `scripts/run-e2e.sh` builds `/tmp/uaw-fake-list-models` (bash, `chmod +x`) emitting
  **exactly one stdout line**: `{"data":[{"id":"claude-opus-4-5","display_name":"Claude Opus 4.5"},{"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5"}]}`
  (the shape `parse_models` accepts; nothing else on stdout).
- The fake agent sidecar (`/tmp/uaw-fake-sdk`) reads `model="${3:-}"` and emits a JSON tool
  event `{"type":"tool","name":"model-probe","summary":"MODEL:<model>"}` (a JSON event so it
  reaches the feed; a bare line would be `Skip`ped). Existing plan/edit specs pass `$3=""`
  → `MODEL:` (they don't assert on it; the extra tool event keeps tool-count `> 0`).
- New `it()` in the existing `describe("claude agent sdk ...")`: select the account →
  `waitUntil` the model `<select>` populates (scope to the select element, `$$("option")`;
  no combined `[attr]+*=` selector) → assert `optCount > 1` → `selectByVisibleText("Claude Sonnet 4.5")`
  → launch (single-line goal) → assert (via `feedText()`/`browser.execute`) the feed contains
  `MODEL:claude-sonnet-4-5` (end-to-end threading). Also assert a launch with "Default (SDK
  chooses)" still produces a result event without error.

---

## Files touched

- `sidecar/claude-agent-sdk/list-models.mjs` (new); `sidecar/claude-agent-sdk/index.mjs`
  (model argv[4]).
- `src-tauri/src/services/agent/mod.rs` (shared resolver + `resolve_sdk_models_sidecar` + test);
  `src-tauri/src/services/agent/sdk.rs` (`spawn` model arg, `spawn_oneshot`, `parse_models`,
  `ModelInfo`, tests).
- `src-tauri/src/commands/agent_sessions.rs` (`list_account_models`, model threading);
  `src-tauri/src/lib.rs` (register the command).
- `src/types/agentSession.ts` (add `ModelInfo`; `AgentSession.model_id` already exists from
  M10b-2a), `src/api/agentSessions.ts` (`listAccountModels` + `model` param),
  `src/stores/accountModels.ts` (new), `src/stores/agentSessions.ts` (`start` model),
  `src/components/AgentsView.vue` (model select + resets + threading),
  `src/components/SdkRunView.vue` (model header).
- `scripts/run-e2e.sh`, `wdio.conf.ts`, `e2e/specs/agent-sdk.e2e.ts`.
