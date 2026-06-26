# SDK sidecar cross-platform correctness — Design

**Goal:** Make the *real* Claude Agent SDK sidecar actually run — on macOS, Linux, **and Windows** — by fixing three latent bugs that the e2e's fake sidecar has masked. The slice is provable locally and contains **no packaging**; it makes the SDK agent correct so the packaging slice can bundle a known-good sidecar.

**Status:** Approved design (post 5-discipline review of the packaging design, which surfaced these bugs empirically). Ready for an implementation plan.

**Context:** Discovered while reviewing the "packaging / real distribution" design. The packaging goal is "a downloaded build's SDK agent works," but a reviewer who installed the pinned SDK found the real sidecar has never actually run end-to-end: the e2e substitutes a **fake** sidecar (`UAW_AGENT_SDK_SIDECAR` → a `/tmp` script), so the real `index.mjs` + the real spawn path are untested. Three bugs make the real SDK agent non-functional today — on Windows entirely, and on every OS for the permission mode. Packaging must not ship that. This slice fixes the sidecar; packaging is the next slice.

---

## Background — the three bugs (all confirmed against the code)

1. **`permissionMode: "dontAsk"` is invalid in the pinned SDK `0.1.0`.** `sidecar/claude-agent-sdk/index.mjs:68` sets it; the comment (lines 22-24) calls it "the SDK's documented locked-down pattern." But `@anthropic-ai/claude-agent-sdk@0.1.0`'s grandchild CLI rejects it at arg-parse: *"argument 'dontAsk' is invalid. Allowed choices are acceptEdits, bypassPermissions, default, plan."* (`grep dontAsk cli.js` = 0; the type union is `default | acceptEdits | bypassPermissions | plan`). So **every real SDK run fails today**, regardless of OS or bundling.

2. **The sidecar is spawned by direct-exec of the script, relying on the shebang.** `src-tauri/src/services/agent/sdk.rs:186` (`spawn`) and `:235` (`spawn_oneshot`) do `Command::new(program)` where `program` is the `.mjs` path. On **Windows there is no shebang mechanism**, so a `.mjs` cannot be launched as a program → the SDK agent is dead on Windows. On macOS/Linux it works only if `node` is on PATH *and* the file carries the exec bit.

3. **`list-models.mjs` is committed non-executable (`100644`).** `git ls-files -s` confirms `index.mjs` is `100755` but `list-models.mjs` is `100644`. Spawned via `Command::new(program)` it fails with `EACCES`/`ENOEXEC` on Unix — so the real model picker is broken too.

The e2e never caught these because its fakes are direct-exec'd **bash** scripts that don't use `dontAsk` and don't import the SDK.

## Decisions

1. **Invoke the sidecar via `node <script>`**, never by direct-exec. This fixes Windows (no shebang needed), removes the exec-bit dependency (bug 3 disappears — `node` reads the script as a file arg), and is the same on all three OSes. `node` is resolved via PATH — the documented prerequisite (the PTY agents already require their CLIs on PATH; the SDK agent already required Node). A missing `node` keeps the existing fail-legible error.
2. **Replace `dontAsk` with `bypassPermissions`** — the valid SDK-0.1.0 mode for a headless, non-interactive run (no permission prompts to a UI that can't answer them). The locked-down boundary is preserved by the existing per-mode `allowedTools` allowlist and the edit-mode `PreToolUse` worktree hook — **not** by the mode name.
3. **The deny-by-default behavior must be empirically verified, not assumed.** Under `bypassPermissions`, confirm against the real SDK 0.1.0 that a non-allowlisted tool (e.g. `Bash`) is actually refused. If `allowedTools` alone is not deny-by-default under bypass, reinforce with `disallowedTools` (`Bash`, `WebFetch`, `WebSearch`, `Task`) and/or a deny `PreToolUse` hook. The slice's value is a *provably* locked-down real run.
4. **Keep the SDK pinned at `0.1.0`.** Only bug 1's mode string is wrong; the fix is a valid same-version mode. A version bump (with its own re-validation + lockfile churn) is avoided unless verification shows `bypassPermissions` is also unavailable (it is in the 0.1.0 type union, so this is not expected).
5. **A `node` binary seam** (`UAW_AGENT_NODE`, default `"node"`) lets the Rust unit tests stay hermetic (point it at a fake "node") and gives the later packaging slice a hook for node-path discovery — without doing that discovery here.

---

## Changes

### `src-tauri/src/services/agent/sdk.rs`
- **`spawn`** and **`spawn_oneshot`**: build the command as `Command::new(node_bin()).arg(program)` followed by the existing args (`goal`/`mode`/`model` for `spawn`; `args` for `spawn_oneshot`), where `fn node_bin() -> String` returns `std::env::var("UAW_AGENT_NODE").unwrap_or_else(|_| "node".into())`. Everything else (env injection, `cwd`, stdio, `process_group`, the oneshot watcher/timeout, the opaque error strings) is unchanged.
- The script's own argv contract is **unchanged**: invoked as `node <script> <goal> <mode> <model>`, the script still sees `process.argv[2]=goal`, `[3]=mode`, `[4]=model` (node is argv[0], the script argv[1]) — exactly as the shebang form produced. `index.mjs`/`list-models.mjs` argv parsing needs no change.
- **Tests** (`spawn_injects_env_overriding_inherited`, `spawn_forwards_mode_as_second_arg`, `spawn_missing_program_is_opaque`, `spawn_oneshot_captures_stdout`, `spawn_oneshot_nonzero_exit_is_err`, `spawn_oneshot_times_out`): set `UAW_AGENT_NODE` to a fake-`node` shell script that consumes the leading script-path arg (`script="$1"; shift`) and then reproduces each test's old fake behavior (echo the injected env, forward the mode arg, exit non-zero, sleep past the timeout). This preserves every existing assertion without requiring a real `node` in the unit-test environment. `spawn_missing_program_is_opaque` instead points `UAW_AGENT_NODE` at a nonexistent binary so the spawn itself fails → the same opaque error.

### `sidecar/claude-agent-sdk/index.mjs`
- Line 68: `permissionMode: "dontAsk"` → `permissionMode: "bypassPermissions"`.
- If verification (Decision 3) shows the allowlist is not deny-by-default under bypass, add `disallowedTools: ["Bash", "WebFetch", "WebSearch", "Task"]` to `options` and/or extend the `PreToolUse` hook to deny non-allowlisted tools. The edit-mode worktree-boundary hook stays.
- Rewrite the comment (lines 22-24): the boundary is the allowlist + the worktree hook under `bypassPermissions`; drop the false "dontAsk … documented" claim.

### `scripts/run-e2e.sh`
- Rewrite the two SDK fakes from bash to **node** scripts, because the backend now invokes them via `node <script>`:
  - `/tmp/uaw-fake-sdk`: a `.mjs`-style node script reading `process.argv[2]=goal`, `[3]=mode` (default `"plan"`), `[4]=model`; emitting the **identical** canned NDJSON (the `assistant` plan line, the `Read` tool line, the `$ANTHROPIC_API_KEY` echo line proving redaction, the non-JSON garbage line proving no-crash, the `KEY:set/unset` probe, the `MODEL:` probe, the `result`); and in `edit` mode writing `AGENT_EDIT.md` into `cwd` via `fs.writeFileSync` (plan mode leaves the tree clean).
  - `/tmp/uaw-fake-list-models`: a node script emitting the same canned `/v1/models` JSON.
  - The `chmod +x` lines for these two become unnecessary (node reads them as file args) but are harmless if kept.
- The PTY fake (`/tmp/uaw-fake-agent`) is invoked by the **PTY** path (`pty.rs`), not the SDK path — **unchanged**.

### `docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md`
- Correct the note asserting `dontAsk` is "the documented locked-down pattern" → it is invalid in SDK 0.1.0; the locked-down run uses `bypassPermissions` + the allowlist + the worktree hook. Point to this spec.

---

## Verification

- **Rust unit tests** (above) pass with the `node`-prefixed command shape via the `UAW_AGENT_NODE` seam.
- **Keyless real-sidecar proof** (the central new check): with a dummy `ANTHROPIC_API_KEY`, run `node sidecar/claude-agent-sdk/index.mjs "test goal" plan` and `… edit` from a scratch dir. The run must reach the SDK's **API auth error** (not an arg-parse rejection) — proving the permission mode is now *accepted*. The `dontAsk` bug fails *before* the API call, so reaching auth is the proof. (The SDK must be installed in the sidecar dir for this — `npm install --ignore-scripts --omit=optional`, not committed here.)
- **Deny-by-default check:** confirm a non-allowlisted tool (e.g. prompt the sidecar toward a `Bash` call) is refused under the chosen mode — empirically, locking Decision 3.
- **(Manual, optional) real-key end-to-end:** the product owner runs the real sidecar against a real account to confirm a real plan/edit message stream + the worktree-write boundary. This is the only check needing a real key and is not automatable here.
- **Docker e2e green** with the node-ified fakes — the regression gate (the agent-sdk specs still drive plan/edit completion, redaction, the model picker).

---

## Security notes
- The locked-down tool surface is **preserved**, just expressed correctly: `bypassPermissions` skips the interactive prompt (meaningless headless) while the `allowedTools` allowlist + the edit-mode `PreToolUse` worktree hook remain the actual boundary — reinforced by `disallowedTools`/a deny hook if verification requires. Decision 3 makes "no shell, no egress, no subagents" a *tested* property, not a comment.
- The key-handling path is untouched: the backend still injects the key into the child's env (`resolve_session_env`), still blanks ambient `ANTHROPIC_AUTH_TOKEN`/`CLAUDE_CODE_OAUTH_TOKEN`, and never logs it. Switching from direct-exec to `node <script>` changes only how the process is launched, not what env it gets.
- `node_bin()` reads `UAW_AGENT_NODE` from the environment of the trusted backend process (not agent-controllable), defaulting to PATH `node` — no new injection surface.

## Scope / honesty
This makes the **real** SDK agent run, locked-down, on all three OSes — the prerequisite the packaging slice assumed but that did not hold. It does **not** bundle the sidecar, resolve it from the resource dir, commit a lockfile, or add any release CI — all of that is the next slice. It still requires **Node on PATH** (documented prerequisite, same model as the PTY CLIs). The real-key end-to-end is a manual check; CI proves the mode is accepted (keyless) + no regression (Docker e2e with fakes).

## Out of scope
All packaging — `bundle.resources`, `resource_dir` resolution, the committed sidecar lockfile, the reusable/build-check/release workflows (the next spec, now unblocked) · node-path discovery for a GUI (PATH-less) launch · bumping the SDK pin · any change to the PTY agent path.

## Review findings incorporated
The packaging review found, and this spec addresses: the invalid `dontAsk` mode (every real run fails — bug 1) · the direct-exec/shebang spawn that is dead on Windows and exec-bit-fragile on Unix (bug 2) · the non-executable `list-models.mjs` (bug 3, subsumed by `node <script>`) · the e2e fakes must become node-runnable or the spawn change breaks the e2e · the deny-by-default behavior must be empirically verified under the new mode, not assumed · keep the SDK pin · the `node` seam for hermetic tests (and a future node-discovery hook) · correct the false edit-mode-doc claim.
