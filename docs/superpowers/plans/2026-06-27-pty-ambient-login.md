# PTY Ambient-Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the PTY agents (`claude-code`, `codex`) honestly ambient-login — stop injecting a per-account key they ignore and stop showing a misleading "account bound" UI; per-account binding stays an SDK-agent feature.

**Architecture:** Null the `provider`/`api_key_env`/`clear_env` values on the two PTY adapters (the `AgentAdapter` struct and the SDK adapter are unchanged). That one registry change cascades: `resolve_session_env` (gated on `api_key_env`) stops injecting, and the launch-form account `<select>` (gated on `adapterProvider !== null`) disappears. Plus a 1-line `kind === 'sdk'` gate on the running-tab header so old PTY rows don't keep showing a stale account.

**Tech Stack:** Rust (the adapter registry + `account_env_tests`), Vue 3 (`AgentsView.vue`), WebdriverIO e2e.

---

## File Structure
- `src-tauri/src/services/agent/mod.rs` — null the two PTY adapters' account fields + a WHY comment; update `registry_has_the_three_clis`.
- `src-tauri/src/commands/agent_sessions.rs` — repoint 3 `account_env_tests` to the SDK adapter; replace the gemini-only reject test with a looped PTY no-injection guard. (`resolve_session_env` itself is unchanged — its `api_key_env: None` reject arm is the retained defense.)
- `src/components/AgentsView.vue` — `kind === 'sdk'` header gate + a launch-form hint.
- `e2e/specs/agent-account.e2e.ts` — delete the PTY-injection case; invert the filtering case to the negative.
- `src-tauri/src/services/keystore/mod.rs` (doc comment) + `README.md` — doc touch-ups.

**Task ordering:** Task 1 (the registry change) breaks the backend tests, so it bundles all backend test updates into one atomic commit (`cargo test` stays green). Tasks 2–3 are independent. The Docker e2e is the final gate (it exercises all three together).

---

### Task 1: Null the PTY adapters' account fields + fix all backend tests (one atomic commit)

**Files:**
- Modify: `src-tauri/src/services/agent/mod.rs` (adapters ~55-78, `registry_has_the_three_clis` ~150-178)
- Modify: `src-tauri/src/commands/agent_sessions.rs` (`account_env_tests` ~688-760)

- [ ] **Step 1: Update the registry test to the ambient shape (test-first — it will fail against the current adapters)**

Replace the body of `registry_has_the_three_clis` (`mod.rs:150-178`) with:

```rust
    #[test]
    fn registry_has_the_three_clis() {
        let ids: Vec<_> = adapters().iter().map(|a| a.id).collect();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"gemini"));
        assert!(find_adapter("claude-code").is_some());
        assert!(find_adapter("nope").is_none());

        // PTY agents are ambient-login: no provider/api_key_env/clear_env (uniform with
        // gemini). Interactive claude/codex ignore an injected key and use the user's login.
        let claude = find_adapter("claude-code").unwrap();
        assert_eq!(claude.provider, None);
        assert_eq!(claude.api_key_env, None);
        assert!(claude.clear_env.is_empty());

        let codex = find_adapter("codex").unwrap();
        assert_eq!(codex.provider, None);
        assert_eq!(codex.api_key_env, None);
        assert!(codex.clear_env.is_empty());

        let gemini = find_adapter("gemini").unwrap();
        assert_eq!(gemini.provider, None);
        assert_eq!(gemini.api_key_env, None);

        // The SDK adapter is the lone account-bearing adapter (the per-account path).
        let sdk = find_adapter("claude-agent-sdk").unwrap();
        assert_eq!(sdk.kind, "sdk");
        assert!(sdk.requires_account);
        assert_eq!(sdk.provider, Some("anthropic"));
        assert_eq!(sdk.api_key_env, Some("ANTHROPIC_API_KEY"));
        assert_eq!(claude.kind, "pty");
        assert!(!claude.requires_account);
    }
```

- [ ] **Step 2: Repoint the 3 injection-bearing `account_env_tests` to the SDK adapter + replace the reject test (test-first)**

In `agent_sessions.rs`:

(a) `matching_account_injects_key_and_clears_collisions` — change `find_adapter("claude-code")` to `find_adapter("claude-agent-sdk")` and assert **both** OAuth tokens are blanked (the SDK clears two). Replace lines ~703-719 (the `let claude = …` through the final assert) with:

```rust
        let sdk = find_adapter("claude-agent-sdk").unwrap();
        let env = resolve_session_env(&sdk, Some(&acct), &store).unwrap();

        // Key present EXACTLY once, only as the value of ANTHROPIC_API_KEY — never as a
        // key, never in any other entry (e.g. a clear_env slot).
        assert_eq!(
            env.iter()
                .filter(|(_, v)| v == SENTINEL)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["ANTHROPIC_API_KEY"],
        );
        assert!(env.iter().all(|(k, _)| k != SENTINEL));
        // The SDK adapter blanks BOTH ambient OAuth tokens.
        assert!(env.iter().any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v.is_empty()));
        assert!(env.iter().any(|(k, v)| k == "CLAUDE_CODE_OAUTH_TOKEN" && v.is_empty()));
```

(b) `provider_mismatch_is_rejected_without_leak` — change `find_adapter("claude-code")` (line ~730) to `find_adapter("claude-agent-sdk")` (the SDK is `provider: Some("anthropic")`, so the openai account still trips the mismatch arm). The assertion string is unchanged.

(c) `missing_key_fails_closed` — change `find_adapter("claude-code")` (line ~757) to `find_adapter("claude-agent-sdk")` (anthropic account, no key stored → still reaches the missing-key arm). Unchanged assertion.

(d) Replace the whole `adapter_without_key_env_rejects_account` test (lines ~736-748) with the looped no-injection regression guard:

```rust
    #[test]
    fn pty_adapters_reject_an_account_and_never_inject() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        let store = temp_store();
        store.set(&acct.keychain_ref, SENTINEL).unwrap();

        // The whole point of ambient-PTY: a key bound to a PTY agent must NEVER reach the
        // env. With api_key_env: None, an account is rejected (not silently injected).
        for id in ["claude-code", "codex", "gemini"] {
            let a = find_adapter(id).unwrap();
            let err = resolve_session_env(&a, Some(&acct), &store).unwrap_err();
            assert!(!err.contains(SENTINEL), "{id} leaked the key");
            assert_eq!(err, "This agent does not support API key accounts");
        }
    }
```

- [ ] **Step 3: Run the backend tests — verify they FAIL against the still-unchanged adapters**

Run: `cd src-tauri && cargo test --lib registry_has_the_three_clis account_env_tests 2>&1 | tail -20`
Expected: failures — `registry_has_the_three_clis` (claude/codex still `Some(...)`), `matching_account_injects_key_and_clears_collisions` / `provider_mismatch_is_rejected_without_leak` / `missing_key_fails_closed` would still pass (claude-code still injects) but `pty_adapters_reject_an_account_and_never_inject` fails (claude-code/codex still inject). This confirms the tests now pin the new behavior.

- [ ] **Step 4: Null the PTY adapters' account fields + add the WHY comment**

In `mod.rs`, replace the `claude-code` and `codex` adapter literals (lines ~55-78). For `claude-code`:

```rust
        // Ambient-login PTY agent. NO provider/api_key_env/clear_env: interactive `claude`
        // ignores an injected ANTHROPIC_API_KEY (cli.js's customApiKeyResponses approval
        // gate) and runs as the user's own `claude login`. Injecting a per-account key was
        // misleading and spilled a live key into a process that ignores it. Per-account
        // binding lives on the SDK adapter (`claude-agent-sdk`).
        AgentAdapter {
            id: "claude-code",
            name: "Claude Code",
            program: "claude",
            args: vec![],
            provider: None,
            api_key_env: None,
            clear_env: vec![],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
```

For `codex` (same rationale — interactive codex uses the stored `auth_mode: chatgpt`, ignoring `OPENAI_API_KEY`):

```rust
        // Ambient-login PTY agent (see claude-code). Interactive `codex` ignores an
        // injected OPENAI_API_KEY and uses its stored login (auth_mode: chatgpt).
        AgentAdapter {
            id: "codex",
            name: "Codex",
            program: "codex",
            args: vec![],
            provider: None,
            api_key_env: None,
            clear_env: vec![],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
```

Leave `gemini` and `claude-agent-sdk` unchanged.

- [ ] **Step 5: Run the full crate suite + clippy — verify green**

Run: `cd src-tauri && cargo test 2>&1 | tail -12 && cargo clippy --all-targets 2>&1 | tail -5`
Expected: all tests pass (the 5 updated tests now match the ambient adapters; the SDK injection test passes against the SDK adapter); clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/agent/mod.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(agents): PTY adapters are ambient-login (no account injection; SDK-only binding)"
```

---

### Task 2: Frontend — gate the running-tab account on SDK + a launch hint

**Files:**
- Modify: `src/components/AgentsView.vue` (header ~309, the launch form ~235)

- [ ] **Step 1: Gate the running-tab account label on the SDK kind**

Replace (line ~309):

```vue
            <template v-if="accountLabel(t.session.account_id)">
              · {{ accountLabel(t.session.account_id) }}
            </template>
```

with:

```vue
            <template v-if="t.session.kind === 'sdk' && accountLabel(t.session.account_id)">
              · {{ accountLabel(t.session.account_id) }}
            </template>
```

(So an old PTY session with a stale `account_id` no longer shows a misleading account; the SDK still shows its real binding.)

- [ ] **Step 2: Add the launch-form ambient hint**

Immediately after the account `<select>` block (the `</select>` at line ~235, before the SDK `<select v-if="selectedIsSdk">` at ~236), insert:

```vue
        <p
          v-if="!adapterSupportsAccounts && !selectedIsSdk"
          class="muted new__hint"
          data-testid="pty-ambient-hint"
        >
          This agent uses your own CLI login. Accounts apply to the SDK agent only.
        </p>
```

- [ ] **Step 3: Typecheck + build — verify green**

Run: `pnpm build 2>&1 | tail -12`
Expected: `vue-tsc --noEmit` + `vite build` succeed (no type errors; `t.session.kind` is a valid field on `AgentSession`).

- [ ] **Step 4: Commit**

```bash
git add src/components/AgentsView.vue
git commit -m "fix(agents): only show a bound account on SDK sessions; PTY launch hint"
```

---

### Task 3: e2e to the negative + doc touch-ups

**Files:**
- Modify: `e2e/specs/agent-account.e2e.ts` (delete the injection case ~78-92; invert the filtering case ~94-109)
- Modify: `src-tauri/src/services/keystore/mod.rs` (a doc comment) + `README.md`

- [ ] **Step 1: Rewrite the e2e to assert the new (ambient) behavior**

In `e2e/specs/agent-account.e2e.ts`: update the file-header comment + the `describe` title, **delete** the `it("injects the bound account key into the terminal env …")` block (lines ~78-92), and **replace** the `it("filters accounts by adapter …")` block (lines ~94-109) with the two negative cases below. Keep the setup `it` (lines ~47-76) — it provides the worktree the agent launches in (the account it creates is now unused for binding but still exercises the Providers flow).

Change the `describe(...)` line (31) to:
```ts
describe("PTY agents are ambient-login (no per-account binding)", () => {
```

Replace lines ~94-109 with:
```ts
  it("PTY agents offer no provider-account select", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/acct");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    expect(await $('[aria-label="Provider account"]').isExisting()).toBe(false);
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Codex");
    expect(await $('[aria-label="Provider account"]').isExisting()).toBe(false);
  });

  it("a PTY agent launches with the ambient login (no key injected -> KEY:unset)", async () => {
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    await (await $("button*=New terminal")).click();
    await (await $('[data-testid="agent-terminal"]')).waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await visibleTermText()).includes("KEY:unset"), {
      timeout: 15_000,
      timeoutMsg: "expected KEY:unset — a PTY agent injects no account key",
    });
  });
```

Also delete the now-unused `accountOptionTexts` helper (lines ~19-24) and update the file-header comment (lines ~26-30) to describe the ambient behavior (the M10b-2a injection it described is gone). The injection + redaction coverage now lives solely in `agent-sdk.e2e.ts`.

- [ ] **Step 2: Update the keystore doc comment + README**

In `src-tauri/src/services/keystore/mod.rs`, find the comment referencing `resolve_session_env` injecting a session's account key (it credits M10b-2a) and narrow it to the SDK path, e.g. change "Consumed by `resolve_session_env` … to inject a session's account key into the agent PTY env" to "Consumed by `resolve_session_env` to inject the **SDK** agent's account key (the PTY agents are ambient-login — they use the user's own CLI login)."

In `README.md`, add one line near the agents/accounts description:
> Provider accounts apply to the **SDK agent** only. The PTY agents (Claude Code, Codex, Gemini) authenticate with your own CLI login (`claude` / `codex` / `gemini`).

- [ ] **Step 3: Sanity-check the e2e spec parses**

Run: `npx tsc --noEmit -p e2e/tsconfig.json 2>&1 | tail -8 || node --check e2e/specs/agent-account.e2e.ts 2>&1 | tail -5`
Expected: no type/syntax error in the rewritten spec. (If there's no `e2e/tsconfig.json`, the `node --check` fallback at least confirms it parses.)

- [ ] **Step 4: Commit**

```bash
git add e2e/specs/agent-account.e2e.ts src-tauri/src/services/keystore/mod.rs README.md
git commit -m "test(e2e)+docs: PTY agents are ambient (no account select); accounts are SDK-only"
```

---

## After all tasks

1. **Final whole-branch review** (opus): review `git diff main...HEAD` — the value-nulling (struct + SDK adapter unchanged), the cascade (no separate frontend logic beyond the header gate + hint), the retained `resolve_session_env` defense, all 5 backend test updates + the no-injection guard, the e2e inversion, and that no key is injected into a PTY child anymore.
2. **Docker e2e** (`pnpm e2e:docker`) — the integration gate. It must stay green: the rewritten `agent-account.e2e.ts` asserts PTY agents show no account select + launch `KEY:unset`; `agent-sdk.e2e.ts` still proves SDK `KEY:set`. (12 spec files.)
3. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR.

---

## Self-Review

**Spec coverage:**
- Null the PTY adapter account fields + WHY comment → Task 1 Step 4. ✓
- The cascade (no injection + account select hidden) → Task 1 (api_key_env: None) + the `adapterProvider`-gated select (auto). ✓
- `kind === 'sdk'` header gate for stale old rows → Task 2 Step 1. ✓
- Launch hint → Task 2 Step 2. ✓
- 3 repointed `account_env_tests` (keep value asserts; SDK clears 2) → Task 1 Step 2(a-c). ✓
- `registry_has_the_three_clis` inverted, SDK contrast kept → Task 1 Step 1. ✓
- The no-injection regression guard (claude-code/codex/gemini) → Task 1 Step 2(d). ✓
- e2e delete-and-invert; `agent-sdk.e2e.ts` is the injection owner → Task 3 Step 1. ✓
- `resolve_session_env` retained as defense (no code change) → noted in File Structure + Task 1 (no edit to it). ✓
- keystore comment + README → Task 3 Step 2. ✓

**Placeholder scan:** none — every step has full code or an exact command. The keystore-comment edit gives the before/after text (the comment wording may vary slightly; the instruction is to narrow it to the SDK path).

**Type/contract consistency:** `provider: None`/`api_key_env: None`/`clear_env: vec![]` are the same shape `gemini` already uses (compiles). The 3 repointed tests use `find_adapter("claude-agent-sdk")` consistently; the SDK's `api_key_env` is `ANTHROPIC_API_KEY` (so the "key once as ANTHROPIC_API_KEY" assert still holds) and it clears `ANTHROPIC_AUTH_TOKEN` + `CLAUDE_CODE_OAUTH_TOKEN` (so the two-token assert holds). `t.session.kind` is the existing discriminator already used at `AgentsView.vue:324-325`. The e2e reuses the existing `visibleTermText` helper + `KEY:unset` (the fake agent's marker; `wdio.conf.ts` scrubs ambient `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` so `KEY:unset` is deterministic).
