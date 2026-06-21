# M10b-2b slice 3 — per-session model picker (dynamic)

**Status:** design (spec)
**Branch:** `cstuncsik/milestone-10b-2b-slice3`
**Builds on:** slices 1+2 (`2026-06-19-agent-sdk-sidecar-design.md`, `2026-06-20-agent-sdk-edit-mode-design.md`)

## Goal

Let the user choose, per session, **which Claude model** the headless SDK agent runs
(cost vs capability: Opus / Sonnet / Haiku). The model list is fetched **dynamically**
from the Anthropic API so it's always current. The chosen model is persisted and shown
in the run view; absent → the SDK's own default. Completes the roadmap "per-session
model picker" frontend task.

## Scope

**In scope**
- A `sidecar/claude-agent-sdk/list-models.mjs` helper (Node built-in `fetch`, **no npm
  deps**) that `GET`s `/v1/models` with the env key and prints the model JSON.
- A `list_account_models(account_id)` command that runs the helper with the account's
  key (resolved backend-only) and returns `Vec<ModelInfo{ id, display_name }>` — the
  key never reaches the frontend.
- A model `<select>` in the SDK launch form (SDK-only): **"Default (SDK chooses)"** +
  the fetched models, lazy-fetched per account + cached, with loading/error states.
- The chosen model threaded to the agent sidecar (`query({ options: { model } })`) and
  persisted in the existing `agent_sessions.model_id` column.
- The model shown in `SdkRunView` so the picker is not write-only.

**Out of scope (later / deferred)**
- Cost/capability **tier labels** (Fast/Balanced/Capable) — would reintroduce a
  hardcoded model→tier map; revisit after this slice.
- **OpenAI/other providers** — the picker is SDK-only and the SDK adapter is
  Anthropic-only; OpenAI's models endpoint needs different auth. The provider gate +
  a documented extension point are in scope; the OpenAI helper is not.
- Model picker for PTY adapters (they pick models interactively in-terminal).
- Persisting the model cache across app restarts (in-memory for the session).
- Pagination beyond a single `limit=1000` page.

---

## Architecture

### 1. Sidecar — `list-models.mjs` (new) + `index.mjs` (model arg)

**`list-models.mjs`** — a standalone, dependency-free Node script:
- Reads the key from `process.env.ANTHROPIC_API_KEY` (never argv).
- `fetch("https://api.anthropic.com/v1/models?limit=1000", { headers: { "x-api-key": key, "anthropic-version": "2023-06-01" }, signal: AbortSignal.timeout(10_000) })`.
  - `anthropic-version` is **required** (omitting it → HTTP 400). `limit=1000` avoids the
    default-20 truncation (one page covers the model list).
- On a 2xx: print the response body (the `{ "data": [ { "id", "display_name", ... } ] }`
  JSON) to **stdout**, exit 0.
- On any non-2xx, fetch throw, or timeout: print **nothing** to stdout, write nothing
  sensitive anywhere, `process.exit(1)`. (Mirrors `index.mjs`'s fixed-error catch — the
  failing status/body/URL must never reach stdout, since Rust forwards opaque errors.)

**`index.mjs`** (agent runner) gains the model: reads `const model = process.argv[4] ?? ""`
and adds it to options via an explicit ternary (NOT `&&`):
```js
const options = { cwd, permissionMode: "dontAsk", allowedTools, settingSources: [],
  maxTurns: 30, env: {...}, ...(model ? { model } : {}),
  ...(mode === "edit" && { hooks: {...} }) };
```
Empty model → no `model` key → SDK default.

### 2. Backend

- **Shared resolver** in `services/agent/mod.rs` — factor the env-or-absolute logic so
  the two resolvers can't drift:
  ```rust
  fn resolve_sidecar_script(env_var: &str, rel: &str) -> String { /* env override (trim-nonempty) else current_dir.join(rel) absolute */ }
  pub fn resolve_sdk_sidecar() -> String { resolve_sidecar_script("UAW_AGENT_SDK_SIDECAR", "sidecar/claude-agent-sdk/index.mjs") }
  pub fn resolve_sdk_models_sidecar() -> String { resolve_sidecar_script("UAW_AGENT_SDK_MODELS", "sidecar/claude-agent-sdk/list-models.mjs") }
  ```
  Keeps the existing `UAW_AGENT_SDK_SIDECAR` (no rename); adds `UAW_AGENT_SDK_MODELS`.
- **`sdk::spawn_oneshot(program, args: &[&str], cwd, env, timeout) -> Result<String, String>`**
  (new primitive, distinct from the streaming `spawn`): spawns a piped child (stdin
  null, stdout piped, **stderr null**), a watcher thread kills the child after `timeout`
  (no new crate — sleep-then-kill), reads all stdout to a `String`, and on non-zero exit
  / kill / spawn failure returns a **fixed opaque** `"Failed to list models"`. A named
  `MODELS_TIMEOUT = Duration::from_secs(10)`.
- **`sdk::parse_models(stdout: &str) -> Result<Vec<ModelInfo>, String>`** (pure,
  unit-tested): parse `{ data: [ { id, display_name? } ] }`; `display_name` falls back
  to `id` when absent; `Ok(vec![])` for `{ data: [] }`; `Err(_)` for non-`{data}` JSON
  (an error body) or malformed/truncated JSON. `ModelInfo { id: String, display_name:
  String }` derives `Debug, Clone, Serialize` (snake_case fields, matching the codebase).
- **`list_account_models(state, account_id) -> Result<Vec<ModelInfo>, String>`** command:
  resolve + **workspace-scope-validate** the account (reuse `load_session_account`) and
  read its key under one DB lock → **release** → reject non-Anthropic providers with a
  fixed error (no spawn) → `spawn_oneshot(resolve_sdk_models_sidecar(), &[], cwd,
  &[("ANTHROPIC_API_KEY", key)], MODELS_TIMEOUT)` → `parse_models` → map any `Err` to the
  fixed opaque `"Failed to list models"`. Register in `tauri::generate_handler!`.
- **Model threading:** `start_agent_session`/`start_sdk_session` gain `model:
  Option<String>`; the SDK branch persists it via the existing `model_id` column
  (`agent_session::create(..., model.as_deref(), ...)` — no migration) and passes it to
  `sdk::spawn(&sidecar, &goal, mode, model.as_deref().unwrap_or(""), cwd, &env)` →
  `argv[4]`. Model is a non-secret, free-form id from our own fetched list; passed as a
  literal argv via `Command::arg` (no shell) — no injection surface. PTY path passes
  `None`/`""` (unaffected; it doesn't spawn the sidecar).

### 3. Frontend

- **`stores/accountModels.ts`** (new, dedicated — NOT `providerAccounts`, whose
  `load()` clears state per workspace): `modelsByAccount: Record<id, ModelInfo[]>`,
  `loadingByAccount`, `errorByAccount`, and `loadModels(accountId)` guarded against
  duplicate/in-flight fetches (a per-account in-flight set) and cache hits. Cache lives
  for the app session (models are account-stable); never tied to workspace lifecycle.
- **`AgentsView`**: a model `<select aria-label="Agent model">` (`v-if="selectedIsSdk"`),
  options = **"Default (SDK chooses)"** (value `""`) + the account's fetched models
  (option value = `id`, text = `display_name`). States: loading → disabled "Loading
  models…"; error → Default-only + the inline hint "models unavailable — check your API
  key"; loaded → Default + models. A `newModelId` ref (default `""`).
  - `watch(newAccountId)`: reset `newModelId = ""` **synchronously**, then `if (val)
    accountModels.loadModels(val)` (guard `""`; the SDK adapter requires an account, so
    models only matter once one is chosen).
  - Also reset `newModelId = ""` in the existing `workspaces.currentId` watch (alongside
    `newMode`).
  - `canStart` is **unchanged** — it must NOT reference `newModelId` ("Default" is always
    a valid launch).
  - `openTerminal` passes `selectedIsSdk ? newModelId || null : null` to `store.start`.
- **`store.start`/`api.startAgentSession`** gain `model: string | null` (positional after
  `mode`, before `cols`); the invoke payload adds `model`; PTY passes `null`.
- **`SdkRunView`**: a small header line "Model: {{ session.model_id ?? 'Default' }}"
  above the feed (the component already receives `session`), so the picker is read-back,
  not write-only.

---

## Security analysis

Carries over slices 1/2 invariants (key in OS keychain by `provider:account_id`; frontend
never sees a raw key; fixed opaque backend errors rendered verbatim by the UI). New path:

1. **Key handling:** keychain → `spawn_oneshot` subprocess **env only** → `list-models.mjs`
   reads `process.env` (not argv/file/stdin), uses it in the request header, never writes
   it. The `/v1/models` response cannot echo the key (verified). The command returns only
   `{id, display_name}` — no key field. No `redact` needed (nothing relayed can contain
   the key), but `parse_models` error variants are **mapped to a fixed string** at the
   command boundary so a malformed body can't reach the frontend.
2. **Error opacity:** on any failure the helper prints nothing to stdout and exits 1;
   stderr is `Stdio::null()`; Rust returns the fixed `"Failed to list models"`. The API's
   status/error body / endpoint never surface.
3. **Account scoping + provider gate:** `load_session_account` enforces workspace scope
   (existing, tested); a non-Anthropic account is rejected with a fixed error before any
   spawn (no OpenAI key pointed at the Anthropic endpoint).
4. **Model value:** non-secret; passed as a literal argv (no shell); a crafted id just
   yields a benign SDK 400. No SSRF — the endpoint is hardcoded `https://api.anthropic.com`
   (TLS verified by default Node fetch; not disabled). `UAW_AGENT_SDK_MODELS` is a
   dev/e2e script-path override with the same threat model as the existing
   `UAW_AGENT_SDK_SIDECAR` (documented; not set in production).

---

## Known limitations (documented)

- The model cache is **in-memory** (per app session); a restart re-fetches on first
  account-select (a brief "Loading models…").
- A persisted `model_id` may name a later-**deprecated** model; a re-run then fails at the
  SDK with that id in the error — acceptable, surfaced via the run's error event.
- No **cost/capability signal** yet (bare display names); tier labels deferred.
- **Anthropic-only**; OpenAI/other providers need their own helper (extension point noted).

---

## Testing strategy

**Rust unit**
- `parse_models`: valid 2-model body → 2 `ModelInfo`; `{data:[]}` → empty `Ok`; an error
  body `{"error":{...}}` → `Err`; truncated JSON → `Err`; a `data` element missing
  `display_name` → `display_name == id` (no panic).
- `resolve_sdk_models_sidecar` mirrors `resolve_sdk_sidecar` (env override vs absolute).
- `spawn_oneshot`: captures stdout of a canned program; non-zero exit → `Err`; (timeout
  path can be a fast `sleep`-based check or covered by review).
- Update the existing `spawn_forwards_mode_as_second_arg` test for the new `model` arg
  (`spawn("echo","GOAL","edit","m1",..)` → `"GOAL edit m1"`).

**e2e (hermetic — no real network)**
- `wdio.conf.ts beforeSession` sets `UAW_AGENT_SDK_MODELS = "/tmp/uaw-fake-list-models"`.
- `scripts/run-e2e.sh` builds `/tmp/uaw-fake-list-models` (bash) emitting exactly one line
  of `{"data":[{"id":"claude-opus-4-5","display_name":"Claude Opus 4.5"},{"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5"}]}` (shape `parse_models` accepts; no extra lines).
- The fake agent sidecar (`/tmp/uaw-fake-sdk`) reads `model="${3:-}"` and emits a JSON tool
  event `{"type":"tool","name":"model-probe","summary":"MODEL:<model>"}` (a JSON event so
  it reaches the feed; a non-JSON line would be Skipped). Existing plan/edit specs pass
  `$3=""` → `MODEL:` (they don't assert on it).
- New scenario: select the account → `waitUntil` the model `<select>` populates (scope to
  the select element, `$$("option")`; do not mix `[attr]` + `*=`) → assert `optCount > 1`
  → `selectByVisibleText("Claude Sonnet 4.5")` → launch → assert the feed contains
  `MODEL:claude-sonnet-4-5` (proves end-to-end threading). Also assert a launch with
  "Default (SDK chooses)" still works (Default is valid).

---

## Files touched

- `sidecar/claude-agent-sdk/list-models.mjs` (new); `sidecar/claude-agent-sdk/index.mjs`
  (model argv[4]).
- `src-tauri/src/services/agent/mod.rs` (shared resolver + `resolve_sdk_models_sidecar`);
  `src-tauri/src/services/agent/sdk.rs` (`spawn` model arg, `spawn_oneshot`,
  `parse_models`, `ModelInfo`, tests).
- `src-tauri/src/commands/agent_sessions.rs` (`list_account_models`, model threading);
  `src-tauri/src/lib.rs` (register the command).
- `src/types/agentSession.ts` (`ModelInfo`), `src/api/agentSessions.ts`
  (`listAccountModels` + `model` param), `src/stores/accountModels.ts` (new),
  `src/stores/agentSessions.ts` (`start` model), `src/components/AgentsView.vue` (model
  select + resets + threading), `src/components/SdkRunView.vue` (model header).
- `scripts/run-e2e.sh`, `wdio.conf.ts`, `e2e/specs/agent-sdk.e2e.ts`.
