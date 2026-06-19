# Milestone 10b-2a — Per-session Account Binding + Key Injection

## Goal

Connect M10b-1's stored provider keys to real agent runs: when a user opens an
interactive CLI terminal, they may bind a **provider account**; at launch the
backend resolves that account's API key from the OS keychain and injects it as the
provider's env var into **that session's PTY**. The session records `account_id`
(and a forward-compat `model_id`, unused this slice). The frontend never receives
a raw key. No Node sidecar — that's M10b-2b, which plugs in behind the same picker.

Done when a user can start a `claude`/`codex` session bound to an Anthropic/OpenAI
account, the chosen key is the one the CLI uses, and the bound account is visible —
without the key ever reaching the DB, transcript, events, errors, or the frontend.

This spec folds in a 5-discipline design review (security/architecture/frontend/
testing/product). Verified facts the review established: SQLite
`ADD COLUMN … REFERENCES … ON DELETE SET NULL` is valid + enforced (NULL default);
portable-pty `CommandBuilder::env()` **overrides** an inherited parent var.
**[review]** tags mark folded-in findings.

## Decisions

- **Account is optional.** `None` → no env injected → today's behavior (CLI uses
  its own OAuth / ambient env). This is the #1 regression guard **[review:
  account=None must stay legacy]** — "just start claude" and accountless launches
  must keep working unchanged.
- **Adapter descriptor** gains `provider: Option<&str>`, `api_key_env:
  Option<&str>`, and `clear_env: &[&str]` (higher-precedence ambient vars to
  neutralize). `claude-code` → `anthropic` / `ANTHROPIC_API_KEY` /
  `["ANTHROPIC_AUTH_TOKEN"]`; `codex` → `openai` / `OPENAI_API_KEY` / `[]`;
  `gemini` → `None` / `None` / `[]`.
- **Gemini gets no account binding in 2a** **[review: dead-end]**. M10b-1's
  provider whitelist is `anthropic`/`openai` only, so a `google` account can never
  exist; advertising an account picker / key env for gemini would be permanently
  empty + misleading. Gemini stays a working bare CLI (account picker hidden). We
  do **not** add `google` to M10b-1 (explicitly deferred).
- **Neutralize colliding higher-precedence vars** **[review: stale ambient
  wins]**. Injecting `ANTHROPIC_API_KEY` is not enough if an ambient
  `ANTHROPIC_AUTH_TOKEN` (which outranks it) is present — so on injection we also
  set each `clear_env` var to `""`.
- **Fail closed** **[review]**: account selected but adapter has no `api_key_env`
  → `Err`; account's provider ≠ adapter's provider → `Err`; account not in the
  session's workspace → `Err`; validated account whose keychain key is missing
  (`get` → `Ok(None)`, e.g. deleted mid-launch) → `Err` (never fall back to the
  ambient key under that account's name).
- **Key is env-only.** Never an argv element, never in the stored `command`,
  transcript, events, or any error string. Errors for the key paths are **fixed
  opaque strings** (extending M10b-1's invariant to this command) **[review]**.
- **Trust indicator** **[review]**: the session row + serialized `AgentSession`
  carry `account_id`; the tab header shows the bound account's `display_name` (no
  key); `account_id` is added to the `session.started` event payload — so the user
  can see which credential is in play.
- **`model_id`**: nullable column added now (schema seam, mirrors `auth_mode`);
  **intentionally unconsumed in 2a** — no model UI, no `model_id` write path.

## Data model — migration `0011_agent_session_account.sql`

```sql
-- Bind an agent session to the provider account whose key it runs under. SET NULL
-- so deleting an account preserves session history (the binding just clears).
-- model_id is a forward-compat seam (per-session model picker is a later slice);
-- it is intentionally unconsumed in this milestone.
ALTER TABLE agent_sessions ADD COLUMN account_id TEXT
    REFERENCES provider_accounts(id) ON DELETE SET NULL;
ALTER TABLE agent_sessions ADD COLUMN model_id TEXT;
CREATE INDEX idx_agent_sessions_account ON agent_sessions(account_id);
```

Register as migration 11 in `db/mod.rs`; bump `workspace.rs::migrations_are_idempotent`
to `version == 11` **[review: test lives in workspace.rs]**. `AgentSession` struct +
`COLUMNS`/`from_row`/`create()`/`params!` gain `account_id: Option<String>`,
`model_id: Option<String>`. `create()` keeps positional args + the existing
`#[allow(clippy::too_many_arguments)]` (mirrors siblings — no params struct)
**[review]**; the two new `?N` placeholders append after `transcript_path`,
preserving the `created_at`/`updated_at` reuse. Update the `make()` test helper +
the `start_agent_session` call site. `agent_session.rs`'s test `migrated_conn`
already enables `PRAGMA foreign_keys = ON`, so the cascade/SET-NULL tests are real.

## Adapter descriptor — `services/agent/mod.rs`

```rust
pub struct AgentAdapter {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: Vec<&'static str>,
    pub provider: Option<&'static str>,      // matches provider_accounts.provider
    pub api_key_env: Option<&'static str>,   // env var the CLI reads its key from
    pub clear_env: Vec<&'static str>,        // higher-precedence ambient vars to blank on inject
    pub capabilities: AgentCapabilities,
}
```
`provider` serializes to the frontend so the picker filters accounts. `api_key_env`
/ `clear_env` are backend-only mechanics (harmless if serialized). The registry
test asserts the three adapters carry the right values (gemini: `provider == None`).

## Key resolution — two testable helpers (`commands/agent_sessions.rs`)

Split per the review: **workspace-scope** validation needs the connection (command,
under lock); **provider/key** logic is pure (helper, keychain IO, no lock).

```rust
// Load + workspace-scope-validate the chosen account (under the connection lock).
pub fn load_session_account(
    conn: &Connection,
    account_id: Option<&str>,
    workspace_id: &str,
) -> Result<Option<ProviderAccount>, String>
//  None                              -> Ok(None)
//  Some(id) not found / wrong ws     -> Err("Selected account is not available in this workspace")
//  Some(id) in ws                    -> Ok(Some(account))

// Build the PTY env for the session (keychain IO — call OUTSIDE the lock).
pub fn resolve_session_env(
    adapter: &AgentAdapter,
    account: Option<&ProviderAccount>,
    store: &dyn KeyStore,
) -> Result<Vec<(String, String)>, String>
//  account None                      -> Ok(vec![])                      (legacy behavior)
//  account Some, api_key_env None     -> Err("This agent does not support API key accounts")
//  provider mismatch                  -> Err("Selected account does not match this agent's provider")
//  store.get Err                      -> Err("Failed to load the account key")
//  store.get Ok(None)                 -> Err("Stored key for this account is missing")
//  store.get Ok(Some(key))            -> Ok([(api_key_env, key)] ++ clear_env.map(|c| (c, "")))
```
Every `Err` is a fixed string; none ever interpolates the key, the keychain_ref,
or the env value. (`KeyStoreError` is already dataless, so `store.get` errors carry
nothing.)

## Launch flow — `start_agent_session`

New optional param `account_id: Option<String>`. Lock discipline **[review: fold
into the existing lock, no 4th lock; resolve store before the lock; key after
release]**:

1. `let store = keystore::resolve();` and `find_adapter(...)` — before any lock.
2. **Lock #1 (the existing worktree-resolve lock):** load `coding_workspace`
   (→ `workspace_id`, `worktree_path`) **and** `let account =
   load_session_account(&conn, account_id.as_deref(), &cw.workspace_id)?`. Release.
3. **No lock:** `let env = resolve_session_env(&adapter, account.as_ref(),
   store.as_ref())?;` (keychain IO).
4. **No lock:** `pty::spawn(&program, &args, worktree_path, &env, cols, rows)?`.
5. **Lock #2 (existing insert):** `agent_session::create(..., account_id =
   account.as_ref().map(|a| a.id.as_str()), model_id = None)`.
6. **Lock #3 (existing event):** `session.started` payload gains `"account_id"`.

The key lives only in the `env` vec → child PTY env; it is never in `command`
(which stays program-only), argv, the transcript (a PTY does not echo env), events,
or any error. Existing non-key errors (`pty open/spawn`, lock) are unchanged
(secret-free already); only the new key paths use the fixed opaque strings above.

## PTY — `services/agent/pty.rs`

`spawn` gains `env: &[(String, String)]` (between `args` and `cols`), applied with
`cmd.env(k, v)` for each pair (alongside `TERM`). Verified: portable-pty seeds the
child env from the parent and `cmd.env` **overrides** an inherited same-name var,
so an injected key wins and a `clear_env` `""` blanks a higher-precedence ambient
var. The single caller + the two `pty.rs` tests pass the new arg (`&[]` where
irrelevant).

## Frontend

- `types/agentSession.ts`: `AgentAdapter` += `provider: string | null`;
  `AgentSession` += `account_id: string | null`, `model_id: string | null`.
- `api/agentSessions.ts`: `startAgentSession(codingWorkspaceId, adapterId,
  accountId, cols, rows)` → `invoke(..., { codingWorkspaceId, adapterId, accountId,
  cols, rows })`. The store's `start` threads `accountId`.
- `components/AgentsView.vue`:
  - Uses `useProviderAccountsStore()`. `newAccountId = ref("")`.
  - `adapterProvider(id)` = the adapter's `provider`; `adapterSupportsAccounts` =
    `provider != null`. `accountOptions = computed(() =>
    providerAccounts.list.filter(a => a.provider === adapterProvider(newAdapterId)))`
    (reactive over store + adapter) **[review]**.
  - A `<select v-model="newAccountId" class="re-select" data-size="sm"
    aria-label="Provider account">` rendered **only when** `adapterSupportsAccounts`,
    after the adapter select: a non-disabled `<option value="">Default (no key)</option>`
    first, then `accountOptions` (display_name). Empty list → only Default.
    `newAccountId` is **not** in `canStart` (account optional) **[review]**.
  - Reset `newAccountId = ""` on **both** an adapter change (`watch(newAdapterId)`,
    non-immediate) **and** a workspace switch (the existing `workspaces.currentId`
    watch that already resets the worktree) **[review]**.
  - `openTerminal` passes `newAccountId.value || null` (null, not undefined —
    matches `createSession`; `""` → `null`) **[review]**.
  - Tab header shows the bound account: `providerAccounts.list.find(a => a.id ===
    t.session.account_id)?.display_name`, rendered `v-if` present (degrades silently
    if the account was deleted) **[review]**.

## Security

- Key resolved at the call site, outside the lock, injected only into the child PTY
  env (the standard mechanism these CLIs use; strictly better than argv). Never in
  the DB, `command`, transcript, events, errors, or any frontend payload. All
  key-path errors are fixed opaque strings. Provider/workspace/key-presence
  mismatches fail closed. Frontend sends only `account_id`.
- **Known limitations (documented, no code fix):** (1) Claude Code, in interactive
  mode, prompts once to approve an injected `ANTHROPIC_API_KEY` and remembers the
  choice in its own config — UAW cannot fully suppress this; the trust indicator
  shows intent, not Claude's internal auth state. (2) "Default (no key)" means
  *inherit the CLI's own auth / ambient env* — this is what lets a Claude-Max /
  ChatGPT-subscription user keep launching; it must not regress to an error.

## Testing

### Rust
- **Adapter registry**: claude-code (`anthropic`/`ANTHROPIC_API_KEY`/clear
  `ANTHROPIC_AUTH_TOKEN`), codex (`openai`/`OPENAI_API_KEY`), gemini
  (`provider == None`).
- **`resolve_session_env` matrix** (FileKeyStore, sentinel key
  `SENTINEL_KEY_abc123`):
  - `None` → `[]`.
  - anthropic account + claude-code → env contains `("ANTHROPIC_API_KEY", key)`
    **and** `("ANTHROPIC_AUTH_TOKEN", "")`; success assertion distinguishes
    key-as-VALUE-of-api_key_env (expected) from key anywhere else **[review:
    value-vs-error]**.
  - provider mismatch (openai account + claude-code) → `Err`.
  - `api_key_env == None` (gemini) + account → `Err`.
  - validated account, empty store (`Ok(None)`) → `Err` (fail closed).
  - every `Err` branch: message never contains the sentinel.
- **`load_session_account`** (conn, PRAGMA FK on): `None`→`None`; in-workspace →
  `Some`; wrong-workspace / nonexistent → `Err` (no sentinel). Mirrors the
  cross-workspace-isolation concern **[review]**.
- **`agent_session`**: `create` round-trips `account_id`/`model_id`; deleting the
  bound provider account sets the session's `account_id` to `NULL` (FK SET NULL,
  pragma already on) while the session row survives **[review]**.
- **`pty::spawn` override**: set a parent var to a poison value, inject a different
  value, assert the child prints the **injected** one (`printf %s "$VAR"`, quoted;
  guard with `remove_var`) **[review: prove override, not just delivery]**.
- Idempotency → 11.

### E2e — extend the agent flow (`e2e/specs/*`)
- **Fake agent** (`scripts/run-e2e.sh`): emit a boolean marker from env presence,
  never the value, keeping `AGENT-READY` + `exec cat` so existing terminal specs
  pass:
  ```bash
  if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${OPENAI_API_KEY:-}" ]; then
    printf 'KEY:set\n'; else printf 'KEY:unset\n'; fi
  printf 'AGENT-READY\n'
  exec cat
  ```
- Flow: create a code project + repo + worktree + an Anthropic provider account →
  open a terminal with **claude-code + that account** → wait for `KEY:set` in the
  visible terminal (via the existing `visibleTermText` helper); assert the
  transcript/terminal **never** contains the fixture key value. Open another with
  **Default (no account)** → `KEY:unset`. Assert the **filter**: the anthropic
  account is an option for claude-code and **absent** for codex (negative
  assertion). Use `selectByVisibleText` for selects; scope selectors (combined-
  selector gotcha); `waitUntil` for the marker.

## Out of scope (later)
- Node-sidecar Claude Agent SDK adapter (**M10b-2b**) — plugs in as another adapter
  behind this same picker; key resolution (`load_session_account` /
  `resolve_session_env`) is reused, only the delivery differs.
- Model selection / injection (`model_id` stays null), OAuth, Google provider,
  changing the account on a live session, a per-workspace **default** account
  (per-session binding is the source of truth; a default is a later convenience).
