# SDK credential isolation — Design

**Goal:** Make the headless Claude Agent SDK agent authenticate **only** as its bound account. Today, when the injected account key is bad/expired/revoked, the SDK's grandchild Claude Code CLI **silently falls back to the user's ambient `claude login`** (macOS Keychain / `~/.claude/.credentials.json`) and runs as that identity. This closes that gap, fail-closed: a bad key errors, never silently switches identity.

**Status:** Approved design (re-scoped from a broader credential-isolation design after three plan-reviews — see "Scope decision"). Ready for an implementation plan.

**Context:** The SDK adapter is `requires_account: true` and headless. `start_sdk_session` already sets a per-session `CLAUDE_CONFIG_DIR` ("Isolate the SDK's own on-disk config… away from ~/.claude") — but that is **not enough**: `CLAUDE_CONFIG_DIR` alone does **not** sever the macOS login Keychain. Only an isolated `HOME` does.

---

## Background — the proven gap (empirical)

- **SDK `query()` falls back.** A dummy `ANTHROPIC_API_KEY` + the user's real ambient login + **default HOME** → the run **succeeded** via the Keychain login (wrong identity). With an **isolated HOME** → "Invalid API key" (fail-closed). `CLAUDE_CONFIG_DIR` set to a temp dir did **not** change this — only `HOME` did.
- **Why HOME:** macOS resolves the login Keychain at `$HOME/Library/Keychains`, and `~/.claude/.credentials.json` is HOME-relative — a fresh HOME hides both. (Confirmed for codex too: its login lives under `CODEX_HOME`/`$HOME` — out of scope here, see below.)
- **The SDK already neutralizes the *settings* sources** that could otherwise inject a credential: `index.mjs:84` passes **`settingSources: []`**, so the grandchild CLI loads no user/project `settings.json` — i.e. a **user** `apiKeyHelper` and any **repo-local `.claude/` config** in the worktree are already ignored. (A *managed/system* `apiKeyHelper` is the remaining documented edge — see Security.)
- **The injection + token-blanking already in place:** `resolve_session_env` injects the account key as `ANTHROPIC_API_KEY` (overriding any inherited one); the SDK adapter blanks `ANTHROPIC_AUTH_TOKEN` + `CLAUDE_CODE_OAUTH_TOKEN`, and `index.mjs:88` re-blanks both for the grandchild. The **only** missing severance is HOME.

## Scope decision (why SDK-only)

A broader design (isolate the PTY agents too, behind a per-account flag + per-session override + an env allowlist) was reviewed three times and judged over-scoped: the **PTY-`claude` fallback is unconfirmed** (`claude -p` with an explicit key was already fail-closed), so PTY isolation would be defense-in-depth for a gap not shown to exist — dragging in a config flag, a migration, an override UI, a truth table, and a spawn-layer env-allowlist change. This spec ships **only the proven fix** (the SDK gap). The rest is deferred (see Out of scope) until an interactive-PTY fallback is actually reproduced.

---

## The change

### `src-tauri/src/commands/agent_sessions.rs` — `start_sdk_session`
Replace the existing `CLAUDE_CONFIG_DIR`-only push (currently `sdk_env.push(("CLAUDE_CONFIG_DIR", transcript_path.with_extension("cfg")))`) with a fresh, isolated **HOME** that all the credential-home env vars point at:

1. Derive the per-session dir: `let isolated_home = transcript_path.with_extension("home");` → `<base>/<uuid>.home` (sibling of the transcript; the uuid has no dots, so the extension swap is safe).
2. **Create it fail-closed** via a small helper: `std::fs::create_dir(&isolated_home)` (**not** `create_dir_all` — it must fail if the path already exists or is a symlink, since a stale/planted `credentials.json` in a reused dir would re-leak), then set mode `0700` on Unix. Any error → return the existing opaque error and do **not** spawn. The uuid makes the path unguessable; `create_dir` + `0700` makes a pre-existing/planted dir fail-closed.
3. Point every credential-home var at it via a small **pure** helper (so it is unit-testable without spawning):
   ```rust
   /// Route the agent's credential-home env at an app-private dir so the grandchild
   /// Claude Code CLI cannot reach the user's ambient login (Keychain / ~/.claude /
   /// %APPDATA%). CLAUDE_CONFIG_DIR alone does NOT sever the macOS Keychain — HOME does.
   fn with_isolated_home(mut env: Vec<(String, String)>, home_dir: &str) -> Vec<(String, String)> {
       for k in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "APPDATA", "LOCALAPPDATA"] {
           env.push((k.to_string(), home_dir.to_string()));
       }
       env
   }
   ```
   Cross-platform: each OS reads only the vars it uses (`HOME` on macOS/Linux; `USERPROFILE`/`APPDATA`/`LOCALAPPDATA` on Windows); the rest are harmless. The spawn inherits-then-overrides, so these override the inherited values.
4. Call it: `let sdk_env = with_isolated_home(env.clone(), &isolated_home.to_string_lossy());` — **unconditionally** (the SDK is always isolated; there is no flag to read, so a refactor cannot drop it). The `injected_key` mask (computed earlier from `env`) is unaffected — `with_isolated_home` only appends dir vars, never touches the key entry.

### `sidecar/claude-agent-sdk/index.mjs` — **unchanged**
The existing `env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" }` (line 88) **propagates the isolated HOME** (the sidecar's `process.env.HOME` is the Rust-injected isolated value) to the grandchild CLI, which is exactly what severs the Keychain. `settingSources: []` (line 84) already blocks the settings-based credential paths. No change needed for this slice. (Constructing a *minimal* sidecar env — to also drop inherited `ANTHROPIC_BASE_URL`/Bedrock/proxy vars — belongs to the deferred broader-scrub slice, not the proven fix.)

---

## Security boundary

- **What this guarantees:** a bound SDK session authenticates **only** as the injected account key; a bad key **fails closed** (the CLI errors), never silently runs as the ambient login. macOS Keychain + `~/.claude/.credentials.json` + a user `apiKeyHelper` (via `settingSources: []`) are all severed.
- **Per-session dir is a security property:** fresh per session (keyed on the session uuid, **never** account-id), `create_dir` fail-closed (no reuse/symlink), mode `0700`. The CLI may write its own credential (the bound account's, derived) into this throwaway dir on success — bounded by `0700` + the dir being single-session.
- **Accepted edges (documented, not closed by this slice):**
  - A **managed/system `apiKeyHelper`** (`/Library/Application Support/ClaudeCode/managed-settings.json`, etc.) is local-admin/MDM-controlled, not HOME-relative, and `settingSources: []` does not suppress managed settings. It sits at precedence-4 (below the injected key), so it only matters on a bad key on a machine an admin has configured. Out of scope (undefeatable from UAW).
  - **Broader inherited-env redirects** (`ANTHROPIC_BASE_URL`/`ANTHROPIC_CUSTOM_HEADERS`/`CLAUDE_CODE_USE_BEDROCK`/`AWS_*`/`HTTP(S)_PROXY`) inherited from the backend's own process env and carried through the `index.mjs` spread are **not** scrubbed here. They require the UAW backend itself to be launched with such vars (unusual for a local-first desktop app) and closing them needs the spawn-layer env-allowlist the reviews flagged — deferred to the broader slice.
- **No new secret-at-rest from UAW:** the injected key still only ever exists as the env value of `ANTHROPIC_API_KEY`; isolation writes no key to disk (the CLI's own write into the 0700 throwaway dir is the bound account's, bounded).
- **GC:** the `<uuid>.home` dirs accumulate under the transcripts base (like the `.cfg` dirs do today) and now hold a CLI-written credential — a **credential-hygiene** follow-up (retention/cleanup), explicitly deferred and named, acceptable for a local single-user app.

## Verification

- **Rust unit:** `with_isolated_home` sets `HOME`/`USERPROFILE`/`CLAUDE_CONFIG_DIR`/`APPDATA`/`LOCALAPPDATA` to the given dir, and the injected key (`ANTHROPIC_API_KEY`) is still present exactly once with its value after the call (extends the existing `matching_account_injects_key` invariant). A test for the dir helper: a fresh path succeeds + is `0700`; a pre-existing path / symlink → `Err` (fail-closed).
- **Manual (real key) — the real fail-closed proof:**
  - **Negative:** bind an account with a **revoked/garbage** key, run an SDK agent → it **errors** ("Invalid API key"), and does **not** stream a successful turn under the ambient login.
  - **Positive:** a **valid** bound key → the SDK agent runs and authenticates as that account, under the isolated HOME (already observed empirically to reach the API auth boundary under a fresh HOME — confirm an end-to-end run).
- **Docker e2e:** unaffected — it drives a **fake** sidecar via `UAW_AGENT_SDK_SIDECAR` (the fake ignores HOME), and the keystore is file-backed via `UAW_KEYSTORE_DIR`. The env change doesn't alter the fake's behavior; the existing agent-sdk specs stay green (regression gate). *(Optional nicety: have the fake echo its `HOME` and assert it is the session `<uuid>.home`, not the runner's home — a hermetic plumbing check. Not required for this slice.)*

## Out of scope (deferred — only if/when confirmed)
- **PTY credential isolation** (claude-code, codex) — the interactive-PTY fallback is **unconfirmed** (`claude -p` was already fail-closed). Revisit only after reproducing a real interactive fallback; codex would use `CODEX_HOME` (proven severance lever).
- The per-account `isolated` flag + migration, the per-session override UI, the effective-isolation truth table — all only needed once PTY isolation is conditional.
- The broader **inherited-env scrub** for redirect/cloud/proxy vars (the env-allowlist + the spawn-layer `env_clear` change + the `index.mjs` minimal env + the model-picker proxy scrub).
- GC of the isolated session dirs.

## Review findings incorporated
Decomposed to the proven SDK gap (three reviews flagged the broader design as defense-in-depth for an unconfirmed PTY gap) · fresh per-session dir keyed on the uuid, `create_dir` fail-closed + `0700` (not `create_dir_all`) · the SDK is always-isolated unconditionally (no flag to drop; the fail-closed invariant holds without a "when isolated" qualifier because SDK isolation is unconditional) · `index.mjs` left unchanged because the existing spread carries the isolated HOME and `settingSources: []` already handles the user/project settings + repo-local-config + user-apiKeyHelper edges · managed-apiKeyHelper + broader-env-redirect documented as accepted/deferred edges · the key mask is unaffected (a no-op).
