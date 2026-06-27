# PTY agents are ambient-login — Design

**Goal:** Stop pretending the interactive PTY agents (`claude-code`, `codex`) support per-account binding. They don't — interactive `claude`/`codex` ignore the injected account key and run as the user's own CLI login — so today UAW injects a live key the agent ignores *and* shows a misleading "account bound" UI. Make them honestly ambient-login: per-account binding stays an SDK-agent feature.

**Status:** Approved design (post 3-lens-plus review; findings folded in). Ready for an implementation plan.

**Context:** The closing slice of the credential-isolation arc. PR #21 made the **SDK** agent fail-closed to its bound account (HOME isolation). This slice corrects the **PTY** side, where account binding was added by M10b-2a but is non-functional.

---

## Background — the proven gap (empirical, this session)
- **Interactive `claude` ignores the injected `ANTHROPIC_API_KEY`** and runs as the user's ambient `claude login` subscription (startup showed *"Welcome back Csaba!"* + the user's real org/usage, with a dummy key set). Mechanism: `cli.js`'s **`customApiKeyResponses` approval gate** — an unapproved custom env key is not used in interactive mode (only `claude -p`, headless, uses it).
- **Interactive `codex` ignores the injected `OPENAI_API_KEY`**: with `OPENAI_API_KEY=dummy` set, `codex login status` still reports *"Logged in using ChatGPT"* and `~/.codex/auth.json` is `auth_mode: chatgpt` / `OPENAI_API_KEY: null`. You must run `codex login --with-api-key` to use a key; the env var alone is ignored.
- So **per-account binding silently doesn't work for either PTY agent.** UAW injects the bound key (which is ignored) and the UI shows "account A bound" while the agent runs as the ambient identity — **identity confusion**, and a live key spilled into a process that ignores it.

## Decision
**Option B — accept ambient for PTY, keep per-account binding SDK-only.** Rejected **option A** (force the key via a seeded `apiKeyHelper` + isolated HOME for claude, `codex login --with-api-key` for codex): viable but per-CLI, fights each CLI's interactive auth, and is version-fragile. The SDK agent (PR #21) remains the fail-closed per-account path.

---

## The change

### `src-tauri/src/services/agent/mod.rs` — null the account fields for the PTY adapters
**Not a struct change — a value change.** The `AgentAdapter` struct keeps `provider`/`api_key_env`/`clear_env` (the SDK adapter needs them). Set them to the ambient shape for the two PTY adapters, making them uniform with the already-ambient `gemini`:
- `claude-code`: `provider: None`, `api_key_env: None`, `clear_env: vec![]` (was `Some("anthropic")` / `Some("ANTHROPIC_API_KEY")` / `vec!["ANTHROPIC_AUTH_TOKEN"]`).
- `codex`: `provider: None`, `api_key_env: None`, `clear_env: vec![]` (was `Some("openai")` / `Some("OPENAI_API_KEY")` / `vec![]`).
- `claude-agent-sdk`: **unchanged** (`provider: Some("anthropic")`, `api_key_env: Some("ANTHROPIC_API_KEY")`, `clear_env: vec![both OAuth tokens]`, `requires_account: true`).

Add a **WHY comment** on the PTY adapters citing the empirical mechanism, so a future reader doesn't "fix" the missing injection:
> Ambient-login: interactive `claude`/`codex` ignore an injected API key (claude's `customApiKeyResponses` approval gate; codex's stored `auth_mode: chatgpt`) and run as the user's own CLI login. Injecting a per-account key was misleading and spilled a live key into a process that ignores it. Per-account binding lives on the SDK adapter (`claude-agent-sdk`).

### The cascade — one registry change fixes both surfaces
Both gates key off these fields, so no separate logic change is needed for them:
- **Backend injection** is gated on `api_key_env` (`resolve_session_env`): with `api_key_env: None`, a PTY adapter never injects.
- **Frontend account select** is gated on `adapterSupportsAccounts = adapterProvider(...) !== null` (`AgentsView.vue:65`), sourced from the backend adapter list (no hardcoded provider on the frontend — confirmed): with `provider: None`, the account `<select>` disappears from the launch form for the PTY adapters. `watch(newAdapterId)` already resets `newAccountId` on adapter switch, so no stale account is sent.
- **New PTY sessions** therefore launch with `account_id: None`.

### `resolve_session_env` semantics — retained fail-closed defense (clarification, no code change)
With `api_key_env: None` **and** a non-`None` account, `resolve_session_env` **errors** `"This agent does not support API key accounts"` — it does **not** silently return ambient. This is intentional and **retained**: the command is a trust boundary; if a stale frontend or a replayed/crafted IPC call passes an `account_id` for a PTY adapter, it fails closed rather than mis-injecting. Normal launches pass `account: None` (the UI hides the select), so this path is defense-only. The provider-mismatch arm (`adapter.provider != account.provider`) likewise stays as retained defense. (Pinned by a test, below.)

### `src/components/AgentsView.vue` — gate the running-tab account on SDK (1 line)
The running-tab header (`:309`) renders `accountLabel(t.session.account_id)` **kind-blind**. An **old** PTY session (launched before this change, when injection existed) has a non-null `account_id`, so its header would still show a misleading account — the very confusion this fixes, persisting in history. Gate it on the session kind:
```vue
<template v-if="t.session.kind === 'sdk' && accountLabel(t.session.account_id)">
  · {{ accountLabel(t.session.account_id) }}
</template>
```
This also hardens any future PTY row. No DB migration needed.

Add a one-line launch-form hint (the existing `new__hint` pattern) shown when a non-SDK, non-account adapter is selected, so the absent account picker isn't confusing:
```vue
<p v-if="!adapterSupportsAccounts && !selectedIsSdk" class="muted new__hint" data-testid="pty-ambient-hint">
  This agent uses your own CLI login. Accounts apply to the SDK agent only.
</p>
```

### Docs
- `src-tauri/src/services/keystore/mod.rs`: update the stale comment ("Consumed by `resolve_session_env` (M10b-2a) to inject a session's account key") → now SDK-only.
- `README.md`: one line — accounts are an SDK-agent feature; the PTY agents (claude/codex/gemini) use your own CLI login.

---

## Tests (full enumeration — the review found the plan must name all of these)

### Rust — `src-tauri/src/commands/agent_sessions.rs` (`mod account_env_tests`)
Three tests currently use `find_adapter("claude-code")` as the account-bearing fixture and **break** (claude-code now has `api_key_env: None` → they hit the "does not support accounts" guard first). Repoint each to the SDK adapter `claude-agent-sdk`:
- `matching_account_injects_key_and_clears_collisions` (:696) → `claude-agent-sdk`. **Keep the value assertions**, and since the SDK clears **two** tokens, assert **both** `ANTHROPIC_AUTH_TOKEN` and `CLAUDE_CODE_OAUTH_TOKEN` are present-and-empty (don't degrade to `.is_ok()`).
- `provider_mismatch_is_rejected_without_leak` (:723) → `claude-agent-sdk` + an *openai* account, so it still exercises the provider-mismatch arm (else it would hit the `api_key_env` guard first).
- `missing_key_fails_closed` (:751) → `claude-agent-sdk` + an anthropic account with no key in the store.
- Unchanged (still valid): `no_account_yields_empty_env` (:689, adapter-independent), `requires_account_gate` (:821, claude-code stays `requires_account: false`), `adapter_without_key_env_rejects_account` (:737, gemini) — extended below.

**Add the regression guard the slice exists for** (extend `adapter_without_key_env_rejects_account`, or a new test) — PTY adapter + an account = no injection:
```rust
// The whole point of ambient-PTY: a key bound to claude-code/codex must NEVER reach the
// env. With api_key_env: None, an account is rejected (not silently injected).
for id in ["claude-code", "codex", "gemini"] {
    let a = find_adapter(id).unwrap();
    let err = resolve_session_env(&a, Some(&acct), &store).unwrap_err();
    assert!(!err.contains(SENTINEL));                              // never leak the key
    assert_eq!(err, "This agent does not support API key accounts");
}
```
Without this, re-adding `api_key_env` to a PTY adapter silently reverts the change with every test green.

### Rust — `src-tauri/src/services/agent/mod.rs` (`registry_has_the_three_clis`, :150)
**Invert** the claude/codex field assertions to the ambient shape and **keep** the SDK contrast (the positive anchor):
```rust
let claude = find_adapter("claude-code").unwrap();
assert_eq!(claude.provider, None);
assert_eq!(claude.api_key_env, None);
assert!(claude.clear_env.is_empty());
let codex = find_adapter("codex").unwrap();
assert_eq!(codex.provider, None);
assert_eq!(codex.api_key_env, None);
// SDK stays the account adapter (unchanged assertions):
let sdk = find_adapter("claude-agent-sdk").unwrap();
assert_eq!(sdk.api_key_env, Some("ANTHROPIC_API_KEY"));
```

### e2e — `e2e/specs/agent-account.e2e.ts` (rewrite to the negative, not a redirect)
This spec is built entirely around PTY (`claude-code`) injection; it cannot "move to the SDK" — the SDK path uses a different fake (`/tmp/uaw-fake-sdk` via `UAW_AGENT_SDK_SIDECAR`) + an NDJSON feed (not a terminal echoing `KEY:set`), and `agent-sdk.e2e.ts:95` **already** proves the SDK injection + redaction. So:
- **Delete** the "injects the bound account key into the terminal" case (binding an account to Claude Code → `KEY:set`) — redundant with `agent-sdk.e2e.ts`.
- **Invert** the "filters accounts by adapter" case to the new behavior: selecting Claude Code (or Codex) shows **no** `[aria-label="Provider account"]` select (`expect(await $('[aria-label="Provider account"]').isExisting()).toBe(false)`), and a launched PTY session reaches the agent with `KEY:unset`.
- `agent-sdk.e2e.ts` is the **named owner** of the SDK injection coverage (no change there).

---

## Security boundary
Net **improvement**: the bound key no longer reaches a process that ignores it (less secret exposure); the misleading "account A bound" UI for PTY is removed (closes identity confusion), including for old rows via the `kind === 'sdk'` header gate. No security property is lost by dropping the `claude-code` `ANTHROPIC_AUTH_TOKEN` blank — it only ever existed to stop a stale ambient token from out-precedencing the *injected* key, and there is no injected key now; the PTY inherits the user's own env (their environment to manage), as it does in any normal terminal. The fail-closed defense (`api_key_env: None` + provider-mismatch arms in `resolve_session_env`) is retained and now guards the PTY adapters too. The key never reached the PTY transcript/events/frontend regardless (it was only an env value).

## Out of scope
Option A (auth-forcing: `apiKeyHelper` seeding / `codex login --with-api-key`) · a DB migration to null old PTY `account_id`s (the 1-line UI gate handles the only user-visible effect; the column stays meaningful for the SDK) · any change to the SDK per-account path.

## Review findings incorporated
"Null the values, not remove the struct fields" (the SDK keeps them) · enumerate all 3 breaking `account_env_tests` (not 1) + keep their value asserts (SDK clears 2 tokens) · invert `registry_has_the_three_clis` keeping the SDK contrast · the e2e is a delete-and-invert to the negative, not a redirect (SDK injection already owned by `agent-sdk.e2e.ts`) · the missing **PTY+account=no-injection** regression guard · the 1-line `kind === 'sdk'` header gate for stale old PTY rows (so "no frontend change" is corrected) · `resolve_session_env` errors (not silently ambient) on PTY+account — retained as intentional defense · the WHY comment cites the mechanism · keystore doc-comment + README updates · don't collapse the orthogonal adapter fields.
