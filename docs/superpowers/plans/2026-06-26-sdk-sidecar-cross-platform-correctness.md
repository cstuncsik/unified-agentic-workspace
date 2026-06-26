# SDK Sidecar Cross-Platform Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the real Claude Agent SDK sidecar run on macOS/Linux/Windows by fixing three latent bugs (invalid `permissionMode`, direct-exec/shebang spawn, non-executable model helper) that the e2e's fake sidecar masked.

**Architecture:** The Rust backend spawns the Node sidecar via `node <script> …` instead of direct-exec (fixes Windows + the exec-bit dependency); the sidecar swaps the invalid `permissionMode: "dontAsk"` for `bypassPermissions` plus an explicit tool denylist (preserving the locked-down surface); the e2e fakes become Node scripts so they still run under the new spawn. No packaging.

**Tech Stack:** Rust (`std::process::Command`), Node ESM/CJS sidecar (`@anthropic-ai/claude-agent-sdk@0.1.0`), bash e2e harness, WebdriverIO Docker e2e.

---

## File Structure

- `src-tauri/src/services/agent/sdk.rs` — `spawn` + `spawn_oneshot` invoke `node <script>`; new `node_bin(env)` helper; 6 spawn tests updated to swap the interpreter via the injected env (parallel-safe).
- `sidecar/claude-agent-sdk/index.mjs` — `permissionMode: "bypassPermissions"` + `disallowedTools`; corrected comment. Argv parsing unchanged.
- `docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md` — a correction banner (the `dontAsk` claim is false for SDK 0.1.0).
- `scripts/run-e2e.sh` — the two SDK fakes (`/tmp/uaw-fake-sdk`, `/tmp/uaw-fake-list-models`) rewritten from bash to Node. The PTY fake (`/tmp/uaw-fake-agent`) is untouched.

**Interdependency note:** Task 1 (Node spawn) and Task 3 (Node fakes) are a pair for the Docker e2e — neither alone leaves the e2e green. Per-task verification for Task 1 is `cargo test` (unit), for Task 3 is `node --check`. The Docker e2e is the **final** gate after all three tasks, not a per-task check.

---

### Task 1: Spawn the sidecar via `node`

**Files:**
- Modify: `src-tauri/src/services/agent/sdk.rs` (`spawn` ~178-215, `spawn_oneshot` ~227-244, tests ~357-410)

- [ ] **Step 1: Add the `node_bin` helper**

Insert above `pub fn spawn(` (around line 176, after the `SdkSpawned` struct):

```rust
/// The Node binary used to run the sidecar scripts. Resolved from the injected env
/// first (so tests swap it per-call without mutating process-global state — keeping
/// the env-mutating tests parallel-safe), then the backend's own env, defaulting to
/// `node` on PATH (the documented prerequisite; a missing `node` yields the existing
/// opaque spawn error).
fn node_bin(env: &[(String, String)]) -> String {
    env.iter()
        .find(|(k, _)| k == "UAW_AGENT_NODE")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("UAW_AGENT_NODE").ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "node".to_string())
}
```

- [ ] **Step 2: Run a build to confirm the helper compiles (unused warning is fine)**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: builds (a `node_bin` never-used warning is acceptable at this step).

- [ ] **Step 3: Route `spawn` through `node`**

In `spawn`, change the command construction. Replace:

```rust
    let mut cmd = Command::new(program);
    cmd.arg(goal)
        .arg(mode)
        .arg(model)
        .current_dir(cwd)
```

with:

```rust
    // Run the sidecar as `node <script> <goal> <mode> <model>`. Direct-exec of the
    // .mjs (relying on the shebang) is dead on Windows and needs the exec bit on Unix;
    // `node <script>` works on all three OSes. The script still sees goal/mode/model
    // at argv[2..4] (node is argv[0], the script argv[1]).
    let mut cmd = Command::new(node_bin(env));
    cmd.arg(program)
        .arg(goal)
        .arg(mode)
        .arg(model)
        .current_dir(cwd)
```

Also update the doc comment on `spawn` (line ~176): change "Spawn the sidecar as a piped child" to "Spawn the sidecar via `node` as a piped child".

- [ ] **Step 4: Route `spawn_oneshot` through `node`**

In `spawn_oneshot`, replace:

```rust
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
```

with:

```rust
    // Same `node <script> <args…>` invocation as `spawn` (see node_bin).
    let mut cmd = Command::new(node_bin(env));
    cmd.arg(program)
        .args(args)
        .current_dir(cwd)
```

- [ ] **Step 5: Update the 6 spawn tests to swap the interpreter via the injected env**

Replace the existing `spawn_injects_env_overriding_inherited`, `spawn_forwards_mode_as_second_arg`, `spawn_missing_program_is_opaque`, `spawn_oneshot_captures_stdout`, `spawn_oneshot_nonzero_exit_is_err`, `spawn_oneshot_times_out` (lines ~357-410) with these. Each sets `UAW_AGENT_NODE` to a real system binary **via the injected env** (no process-global mutation → parallel-safe); the `program` slot then becomes that binary's first argument, exactly as the real script path is `node`'s first argument.

```rust
    #[test]
    fn spawn_injects_env_overriding_inherited() {
        std::env::set_var("UAW_SDK_PROBE", "PARENT");
        let dir = std::env::temp_dir();
        // Swap "node" for `printenv` via the injected env. The script slot becomes
        // printenv's VAR arg (the var it echoes); goal/mode/model are empty (skipped).
        // The injected UAW_SDK_PROBE=INJECTED must override the inherited PARENT.
        let mut sp = spawn(
            "UAW_SDK_PROBE",
            "",
            "",
            "",
            &dir,
            &[
                ("UAW_AGENT_NODE".into(), "printenv".into()),
                ("UAW_SDK_PROBE".into(), "INJECTED".into()),
            ],
        )
        .expect("spawn printenv");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        std::env::remove_var("UAW_SDK_PROBE");
        assert_eq!(out.trim(), "INJECTED"); // injected beats the inherited "PARENT"
    }

    #[test]
    fn spawn_forwards_mode_as_second_arg() {
        let dir = std::env::temp_dir();
        // Swap "node" for `echo`; empty script slot, so echo prints just the forwarded
        // goal/mode/model — proving they arrive in order after the script arg.
        let mut sp = spawn(
            "",
            "GOAL",
            "edit",
            "m1",
            &dir,
            &[("UAW_AGENT_NODE".into(), "echo".into())],
        )
        .expect("spawn echo");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        assert_eq!(out.trim(), "GOAL edit m1");
    }

    #[test]
    fn spawn_missing_node_is_opaque() {
        // A missing `node` (not a missing script) is now the spawn-failure path.
        let err = match spawn(
            "goal",
            "plan",
            "",
            "",
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "/no/such/node-xyz".into())],
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected spawn to fail"),
        };
        assert_eq!(err, "Failed to start the agent sidecar");
    }

    #[test]
    fn spawn_oneshot_captures_stdout() {
        // `echo "" hello` → " hello" → trimmed "hello".
        let out = spawn_oneshot(
            "",
            &["hello"],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "echo".into())],
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn spawn_oneshot_nonzero_exit_is_err() {
        // `false` ignores its args and exits non-zero.
        assert!(spawn_oneshot(
            "",
            &[],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "false".into())],
            std::time::Duration::from_secs(5),
        )
        .is_err());
    }

    #[test]
    fn spawn_oneshot_times_out() {
        // `sleep 10` (script slot = "10") outlasts the 50 ms timeout → killed → Err.
        let r = spawn_oneshot(
            "10",
            &[],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "sleep".into())],
            std::time::Duration::from_millis(50),
        );
        assert!(r.is_err());
    }
```

- [ ] **Step 6: Run the sdk tests — verify they pass**

Run: `cd src-tauri && cargo test --lib services::agent::sdk 2>&1 | tail -20`
Expected: all `sdk::tests` pass (incl. the 6 spawn tests + the unchanged parse/pump/status tests). If `printenv`/`echo`/`false`/`sleep` resolve on the dev/CI PATH (they do on macOS + Ubuntu), the env/arg/oneshot assertions hold.

- [ ] **Step 7: Run the full crate test suite — verify no regression**

Run: `cd src-tauri && cargo test 2>&1 | tail -15`
Expected: the whole suite passes (the spawn signature is unchanged, so `agent_sessions.rs` callers are unaffected).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs
git commit -m "fix(sdk): spawn the sidecar via \`node <script>\` (Windows + exec-bit safe)"
```

---

### Task 2: Fix the sidecar permission mode + lock-down

**Files:**
- Modify: `sidecar/claude-agent-sdk/index.mjs` (comment ~22-24, options ~66-79)
- Modify: `docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md` (correction banner after line 5)

- [ ] **Step 1: Correct the `allowedTools` comment**

Replace the comment block at `index.mjs` lines 22-24:

```js
// Explicit tool surface: dontAsk + an allowlist denies everything not listed (no
// shell, no egress, no subagents) — the SDK's documented locked-down pattern. Edit
// mode adds Write/Edit; plan mode is read-only.
```

with:

```js
// Explicit tool surface: an allowlist (allowedTools) plus a denylist of shell/egress/
// subagent tools (disallowedTools), run under bypassPermissions — no interactive
// prompts, nothing dangerous runs. Edit mode adds Write/Edit (bounded to the worktree
// by the PreToolUse hook below); plan mode is read-only.
```

- [ ] **Step 2: Replace `dontAsk` with `bypassPermissions` + add the denylist**

In the `options` object (lines ~66-79), replace:

```js
const options = {
  cwd,
  permissionMode: "dontAsk",
  allowedTools,
  settingSources: [],
```

with:

```js
const options = {
  cwd,
  // bypassPermissions: skip the interactive permission prompt (meaningless headless).
  // The locked-down surface is the allowlist + this denylist (shell/egress/subagents)
  // + the edit-mode worktree hook — NOT the mode name. "dontAsk" is INVALID in SDK
  // 0.1.0 (modes: default | acceptEdits | bypassPermissions | plan) and was rejected
  // at arg-parse, so the real agent never ran.
  permissionMode: "bypassPermissions",
  allowedTools,
  disallowedTools: ["Bash", "BashOutput", "KillShell", "WebFetch", "WebSearch", "Task"],
  settingSources: [],
```

Leave the rest of `options` (`maxTurns`, `env`, the `model` spread, the edit-mode `hooks` spread) unchanged.

- [ ] **Step 3: Verify the SDK installs and the mode is now accepted (keyless)**

This proves bug 1 is fixed: with a dummy key, the run must reach an **API auth error**, not an arg-parse rejection of the permission mode.

Run:
```bash
cd sidecar/claude-agent-sdk && npm install --ignore-scripts --omit=optional >/dev/null 2>&1 && \
ANTHROPIC_API_KEY=sk-ant-dummy-00000000 node index.mjs "say hi" plan 2>&1 | head -20; cd -
```
Expected: NDJSON and/or an error mentioning authentication / invalid x-api-key (a 401-class failure) — and crucially **no** line containing `permission-mode` / `dontAsk` / `is invalid`. (`node_modules/` stays gitignored — do not commit it.) If you instead see a permission-mode arg-parse error, `bypassPermissions` is wrong for this SDK build → STOP and report (BLOCKED): re-check the SDK's valid modes and pick the headless-correct one.

- [ ] **Step 4: Verify the denylist field is honored (keyless smoke)**

Confirm `disallowedTools` is an accepted option (it must not throw a schema/validation error at startup):

Run:
```bash
cd sidecar/claude-agent-sdk && ANTHROPIC_API_KEY=sk-ant-dummy-00000000 node index.mjs "list files" edit 2>&1 | head -20; cd -
```
Expected: same auth-class failure, **no** option-validation error mentioning `disallowedTools`. If `disallowedTools` is rejected as unknown, STOP and report (BLOCKED): replace it with a PreToolUse deny hook for those tool names instead. (The real deny-by-default + end-to-end behavior is confirmed by the product owner's manual real-key check — see the final section — which this plan cannot run without a real key.)

- [ ] **Step 5: Add the correction banner to the edit-mode design doc**

In `docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md`, insert immediately after line 5 (the `**Builds on:**` line, before `## Goal`):

```markdown

> **Correction (2026-06-26):** The `permissionMode: "dontAsk"` described throughout
> this doc is **invalid in the pinned SDK `0.1.0`** (its modes are `default |
> acceptEdits | bypassPermissions | plan`) — every real run was rejected at arg-parse,
> so the SDK agent never actually ran (the e2e uses a fake sidecar). The sidecar now
> uses `permissionMode: "bypassPermissions"` + the `allowedTools` allowlist + a
> `disallowedTools` denylist (shell/egress/subagents) + the edit-mode worktree hook.
> See `2026-06-26-sdk-sidecar-cross-platform-correctness-design.md`. The rest of this
> doc is kept as the original design record.
```

- [ ] **Step 6: Commit**

```bash
git add sidecar/claude-agent-sdk/index.mjs docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md
git commit -m "fix(sdk): use valid bypassPermissions mode + tool denylist (dontAsk invalid in 0.1.0)"
```

---

### Task 3: Node-ify the e2e fake sidecar scripts

**Files:**
- Modify: `scripts/run-e2e.sh` (the `/tmp/uaw-fake-sdk` heredoc ~71-91, the `/tmp/uaw-fake-list-models` heredoc ~93-99)

- [ ] **Step 1: Replace the fake SDK sidecar heredoc**

The backend now runs the sidecar as `node <script> …`, so the fake must be a Node script (not bash). Replace the block from the comment above `cat >/tmp/uaw-fake-sdk` through its `chmod +x /tmp/uaw-fake-sdk` (lines ~65-91) with:

```bash
# A fake Claude Agent SDK sidecar for the SDK e2e, run via `node` (the backend now
# invokes the sidecar as `node <script> <goal> <mode> <model>`). goal=argv[2],
# mode=argv[3] (default "plan"), model=argv[4]. Emits canned NDJSON incl. a deliberate
# $ANTHROPIC_API_KEY echo (to prove the backend redacts it), a non-JSON garbage line
# (to prove no-crash), a KEY:set/unset marker, then a result. In edit mode, writes an
# untracked file into the worktree (cwd) to simulate a real agent edit. No shebang /
# exec bit needed — node runs it as a file argument.
cat >/tmp/uaw-fake-sdk <<'SDK'
const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const model = process.argv[4] ?? "";
const key = process.env.ANTHROPIC_API_KEY ?? "";
const km = key ? "KEY:set" : "KEY:unset";
// cwd is the worktree (the backend sets current_dir); relative path never escapes it.
if (mode === "edit") require("fs").writeFileSync("AGENT_EDIT.md", "edited by fake sdk\n");
const w = (o) => process.stdout.write(JSON.stringify(o) + "\n");
w({ type: "assistant", text: "Planning: " + goal.replace(/"/g, "") });
w({ type: "tool", name: "Read", summary: "README.md" });
w({ type: "tool", name: "echo", summary: key || "none" });
process.stdout.write("this line is not json\n");
w({ type: "tool", name: "probe", summary: km });
w({ type: "tool", name: "model-probe", summary: "MODEL:" + model });
w({ type: "result", status: "success", summary: "Done" });
SDK
```

(Drop the `chmod +x /tmp/uaw-fake-sdk` line — node reads the file, the exec bit is irrelevant.)

- [ ] **Step 2: Replace the fake model-list helper heredoc**

Replace the block from the comment above `cat >/tmp/uaw-fake-list-models` through its `chmod +x /tmp/uaw-fake-list-models` (lines ~93-99) with:

```bash
# Fake model-list helper, run via `node`: emits canned /v1/models JSON (the shape
# parse_models accepts) and nothing else. No network, no auth, no shebang/exec bit.
cat >/tmp/uaw-fake-list-models <<'MODELS'
process.stdout.write(JSON.stringify({ data: [
  { id: "claude-opus-4-5", display_name: "Claude Opus 4.5" },
  { id: "claude-sonnet-4-5", display_name: "Claude Sonnet 4.5" },
] }) + "\n");
MODELS
```

(Drop the `chmod +x /tmp/uaw-fake-list-models` line.)

Leave `/tmp/uaw-fake-agent` and its `chmod +x` (the PTY-agent fake, ~lines 40-63) **unchanged** — it is spawned by the PTY path, not the SDK path.

- [ ] **Step 3: Syntax-check the two fakes are valid Node**

Run:
```bash
bash -n scripts/run-e2e.sh && \
sed -n '/cat >\/tmp\/uaw-fake-sdk <<.SDK./,/^SDK$/{//!p}' scripts/run-e2e.sh | node --check && \
sed -n '/cat >\/tmp\/uaw-fake-list-models <<.MODELS./,/^MODELS$/{//!p}' scripts/run-e2e.sh | node --check && \
echo "OK: shell + both node fakes parse"
```
Expected: `OK: shell + both node fakes parse` (the `bash -n` confirms the script, `node --check` confirms each heredoc body is valid JS).

- [ ] **Step 4: Commit**

```bash
git add scripts/run-e2e.sh
git commit -m "test(e2e): node-ify the fake SDK sidecar + model helper for the node spawn"
```

---

## After all tasks

1. **Final whole-branch review** (opus): review `git diff main...HEAD` — the `node <script>` spawn + `node_bin` seam, the `bypassPermissions`/denylist change, the doc banner, and the node-ified fakes. Confirm: the spawn signature is unchanged (callers unaffected), the tests are parallel-safe (interpreter via injected env, no new process-global `UAW_AGENT_NODE` mutation), the locked-down surface is preserved (allowlist + denylist + worktree hook), and the e2e fakes match the real argv/env contract (`argv[2..4]`, `ANTHROPIC_API_KEY`, `AGENT_EDIT.md` in edit mode).
2. **Docker e2e — the integration gate** (`pnpm e2e:docker`): the only check that exercises Task 1 + Task 3 together (the backend spawns `node /tmp/uaw-fake-sdk`). The agent-sdk specs must stay green (plan/edit completion, key redaction, the model picker). This is the regression proof.
3. **Manual real-key check (product owner)**: run the real sidecar against a real account from a worktree — confirm a real plan and edit stream, that `AGENT_EDIT.md`-style edits land inside the worktree, and that a non-allowlisted tool (e.g. a shell command) is refused. This is the only verification needing a real key; it confirms the end-to-end + deny-by-default behavior the keyless checks cannot.
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR.

---

## Self-Review

**Spec coverage:**
- Bug 1 (`dontAsk` invalid) → Task 2 (Steps 1-4). ✓
- Bug 2 (direct-exec/shebang spawn, Windows-dead) → Task 1 (Steps 3-4). ✓
- Bug 3 (non-exec `list-models.mjs`) → subsumed by Task 1's `node <script>` (exec bit irrelevant). ✓
- Decision 1 (`node <script>`) → Task 1. ✓ Decision 2 (`bypassPermissions`) → Task 2 Step 2. ✓ Decision 3 (deny-by-default verified) → Task 2 Steps 3-4 (keyless) + the manual real-key check. ✓ Decision 4 (keep SDK pin) → no version bump anywhere. ✓ Decision 5 (`UAW_AGENT_NODE` seam) → Task 1 Step 1 `node_bin`. ✓
- e2e fakes → Node → Task 3. ✓ Doc correction → Task 2 Step 5. ✓ Verification (Rust unit, keyless proof, Docker e2e, manual) → tasks + the After-all-tasks section. ✓

**Placeholder scan:** none — every code step has full content; the two BLOCKED branches (Task 2 Steps 3-4) are explicit fallbacks, not deferrals.

**Type/contract consistency:** `node_bin(env: &[(String, String)])` is called in both `spawn` and `spawn_oneshot`, both of which already take `env`. The script-argv contract (`argv[2]=goal, [3]=mode, [4]=model`) is consistent across the real `index.mjs`, the spawn order (`program, goal, mode, model`), and the Node fake. `disallowedTools` is the field added in Task 2 and verified in Step 4. The fake's emitted shapes (`assistant`/`tool`/`result`, `AGENT_EDIT.md`, `KEY:set/unset`, `MODEL:`) match the originals the e2e asserts on.
