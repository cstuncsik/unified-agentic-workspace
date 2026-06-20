# Milestone 10b-2b (slice 1) — Claude Agent SDK sidecar adapter (plan-only)

## Goal

Add a headless **Claude Agent SDK** agent: pick a worktree + a provider account + a
goal, and watch the SDK agent **analyze the repo and propose a plan** in a read-only
structured feed. It runs in a Node sidecar the backend spawns (never the renderer),
reusing M10b-2a's account → key injection. **Plan-only this slice** — the agent
does NOT edit files or run side-effecting shell; the plan is the reviewable output.
Done when a user can run a plan-only SDK session bound to an account, see the
streamed plan, and the key never reaches the DB/transcript/events/frontend/errors.

This slice proves the whole SDK pipeline (sidecar, structured NDJSON events,
persistence, render, hardened key injection) on the safest possible footing. Edit
mode, dispatch-as-goal, the resulting diff, and completion→review are later slices.

This spec folds in a 5-discipline design review (security/rust/frontend/testing/
product). The review changed three load-bearing things — marked **[review]**.

## Decisions (review-driven)

- **Plan-only** **[review: CRITICAL footgun]**. `permissionMode: "plan"` — no
  autonomous edits, no side-effecting Bash. An unattended file-editing agent is not
  an acceptable first default for a non-expert (`cwd` is **not** a sandbox).
- **Goal via argv, NOT stdin** **[review: the real SDK contradicts stdin]**.
  Verified: `query({ prompt: "<goal>" })` takes the goal as a string the SDK passes
  to its own subprocess via IPC — the Node sidecar never reads its own stdin. The
  backend passes the goal as the sidecar's **argv[1]** (the goal is not secret; the
  key still goes via env). This also removes any stdin/pipe-backpressure concern
  (the child's stdin is `null`).
- **Require a bound account** for the SDK adapter (fail closed) **[review]**.
  Headless + no account = the agent silently authenticates as whatever ambient
  token it finds (a stale `ANTHROPIC_AUTH_TOKEN` outranks the key; or on-disk
  OAuth) — wrong identity/billing, no visible login. The three PTY CLIs keep the
  M10b-2a optional-account behavior.
- **Redact the key at the sink** **[review: the agent authors output we persist]**.
  Unlike a PTY (which only echoes the user's keystrokes), the SDK agent generates
  content (assistant text, tool summaries) that UAW writes to the transcript and
  streams to the feed — a prompt-injected repo/task could make it print the key.
  The streaming thread masks the injected key value in **every line before writing
  the transcript and before emitting** the event.
- **Status from the `result` event, not the exit code** **[review]**. A sidecar can
  exit 0 having emitted an error result, or crash without a result. Status is a
  pure function of `(saw_result, saw_error, exit_code)`.
- **The SDK dependency stays OUT of the pnpm workspace** **[review: Docker e2e has
  Node 22 + `pnpm install --frozen-lockfile`]**. CI must never install
  `@anthropic-ai/claude-agent-sdk`; the e2e proves the contract entirely via a
  **fake sidecar** (`UAW_AGENT_SDK_SIDECAR`). `Dockerfile.e2e` is unchanged.

## Data model — migration `0012_agent_session_kind.sql`

```sql
-- Distinguishes interactive PTY sessions from headless SDK runs so the frontend
-- can pick the right view without re-deriving from the live adapter registry.
ALTER TABLE agent_sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'pty';
```

`AgentSession` struct + `COLUMNS`/`from_row`/`create` gain `kind: String`; bump
`migrations_are_idempotent` to `version == 12`. `start_agent_session` records the
adapter's kind. The frontend dispatches on `session.kind` **[review: persist kind,
don't derive from store.adapters which races first paint / a renamed adapter]**.

## Adapter descriptor — `services/agent/mod.rs`

`AgentAdapter` gains:
```rust
pub kind: &'static str,           // "pty" | "sdk"
pub requires_account: bool,       // true for the SDK adapter
```
The three CLIs: `kind: "pty"`, `requires_account: false`. New adapter:
```rust
AgentAdapter {
    id: "claude-agent-sdk", name: "Claude Agent SDK", program: "",  // resolved at runtime
    args: vec![], kind: "sdk", requires_account: true,
    provider: Some("anthropic"), api_key_env: Some("ANTHROPIC_API_KEY"),
    clear_env: vec!["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"],
    capabilities: full_capabilities(),
}
```
`resolve_sdk_sidecar() -> String`: `UAW_AGENT_SDK_SIDECAR` override (e2e injects the
fake) else the bundled sidecar path — mirrors `resolve_program`/`UAW_AGENT_BIN`.

## The Node sidecar — `sidecar/claude-agent-sdk/index.mjs`

A small script (its own `node_modules`, NOT in the pnpm workspace):
```js
#!/usr/bin/env node
import { query } from "@anthropic-ai/claude-agent-sdk";
const goal = process.argv[2] ?? "";                 // argv[2] = first CLI arg (argv[0]=node, [1]=script); not stdin
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");
try {
  for await (const m of query({ prompt: goal, options: {
    cwd: process.cwd(),                              // backend sets current_dir = worktree
    permissionMode: "plan",                          // no edits, no side-effecting shell
    settingSources: [],                              // ignore repo-supplied .claude settings
    maxTurns: 30,
    env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  } })) {
    if (m.type === "assistant") { /* emit {type:"assistant",text} per text block; {type:"tool",name,summary} per tool_use */ }
    else if (m.type === "result") emit({ type: "result", status: (m.subtype === "success" && !m.is_error) ? "success" : "error", summary: m.result ?? "" });
  }
} catch { emit({ type: "error", message: "Agent run failed" }); process.exit(1); }
```
The sidecar owns the SDK→schema mapping; it never echoes env. `env` is spread
(`...process.env`) so the grandchild CLI inherits the injected key while ambient
higher-precedence tokens are blanked **[review I4: TS SDK `options.env` replaces
the subprocess env]**. The backend additionally sets an isolated `CLAUDE_CONFIG_DIR`
so the SDK's own on-disk session files don't land in `~/.claude` (outside redaction).

NDJSON schema (sidecar → backend), one object per line:
`{"type":"assistant","text":...}` · `{"type":"tool","name":...,"summary":...}` ·
`{"type":"result","status":"success"|"error","summary":...}` · `{"type":"error","message":...}`.

## Backend runtime — `commands/agent_sessions.rs` + `services/agent/sdk.rs`

`start_agent_session` gains `prompt: Option<String>`; after `load_session_account`,
**validate**: `adapter.requires_account && account.is_none()` →
`Err("This agent requires a provider account")` (fixed string). Then branch on
`adapter.kind`: `"pty"` → today's path; `"sdk"` → `start_sdk_session`.

**Lock discipline (reuse M10b-2a):** `keystore::resolve()` before the lock; account
load under the existing short lock; **key resolution + sidecar spawn OUTSIDE the
lock**; row insert / registry insert / `session.started` under short locks.

`start_sdk_session` (its own fn, sharing the exit-tail helper with the PTY path
**[review M4]**):
1. `let env = resolve_session_env(&adapter, account.as_ref(), store.as_ref())?` —
   reused unchanged (key + `clear_env` blanks). For SDK, account is required so the
   key is present. Extract the injected key value (the `api_key_env` pair's value)
   for redaction; add `("CLAUDE_CONFIG_DIR", <per-session temp dir>)`.
2. Spawn `std::process::Command(resolve_sdk_sidecar())` with `.current_dir(worktree)`,
   `.arg(goal)`, the env pairs, `stdin(null)`, `stdout(piped)`, `stderr(piped)`,
   `process_group(0)` (Unix) so stop can kill the whole tree (the SDK spawns a
   grandchild CLI) **[review M2]**. **Spawn the reader thread before** anything that
   could block.
3. Reader thread: `BufReader(stdout)` + `read_until(b'\n')` (byte-oriented — NDJSON
   lines can be long / non-UTF8; never `lines()` which errors the whole stream)
   **[review M1]**. Per line: `redact(line, &key)` (mask the key value) → append the
   masked bytes to the transcript (NDJSON) → `parse_sdk_line(masked)` → emit
   `agent-sdk-event` (masked); track `saw_result`/`saw_error`. On EOF: `child.wait()`
   (reap), `status = sdk_status(saw_result, saw_error, exit_code)`, then the shared
   exit-tail: `mark_exited` (only moves a still-running row — preserves a user
   `stop`), re-read, `agent.exited` event, remove from registry. `command` stored =
   the sidecar path (program-only, no key/goal).

**Pure, unit-testable seams** (the impure transcript-write/emit stays a closure,
e2e-covered — **[review C1: pump's call-site closure is the untested part]**):
```rust
pub enum SdkLine { Event { kind: String, raw: String }, Error(String), Skip }
pub fn redact(line: &str, key: &str) -> String           // mask key value; "" key → unchanged
pub fn parse_sdk_line(line: &str) -> SdkLine             // serde_json; bad/unknown type → Error; never panics
pub fn pump_ndjson<R: BufRead, F: FnMut(SdkLine)>(r: R, on: F) -> SdkOutcome  // {saw_result, saw_error}
pub fn sdk_status(saw_result: bool, saw_error: bool, exit: Option<i64>) -> &'static str
//   saw_result && !saw_error && exit == Some(0)  -> "exited"  else "failed"
```

**Process registry** generalizes to an enum **[review H1]**:
```rust
pub enum AgentProc { Pty(pty::PtyHandle), Sdk(sdk::SdkHandle) }   // SdkHandle: kill (process group)
pub struct AgentProcesses(pub Mutex<HashMap<String, AgentProc>>);
```
`stop_agent_session`: `match` → Pty kills the child, Sdk kills the process group.
`write_agent_session`: Pty writes; `Sdk => Err("This agent does not accept input")`
(never panics). `resize_agent_session`: Pty resizes; `Sdk => Ok(())` (no-op).

## Frontend

- `types/agentSession.ts`: `AgentAdapter` += `kind: string`, `requires_account:
  boolean`; `AgentSession` += `kind: string`.
- `api/agentSessions.ts` / `stores/agentSessions.ts`: `start` gains
  `prompt: string | null` (positional, `null` for pty — mirrors M10b-2a's
  `account_id`). `agent-sdk-event` listener registered **in the store at init**, not
  per-component, into `sdkEvents: ref<Record<string, SdkEvent[]>>` **[review I2]**.
- `components/AgentsView.vue`:
  - When the selected adapter `kind === "sdk"`: render a **goal `<textarea>`**
    (`aria-label="Agent goal"`); `canStart` additionally requires a non-empty goal
    AND (since `requires_account`) a selected account. Reset the goal on **both**
    adapter change and workspace switch (alongside `newAccountId`) **[review I6]**.
    `openTerminal` passes `newGoal.value || null`.
  - Per session, render `SdkRunView` when `session.kind === "sdk"`, else
    `TerminalTab`. An unknown kind falls back to **neither** (a placeholder), never a
    terminal **[review C1]**. `data-testid` on the chosen branch.
- `components/SdkRunView.vue`: a read-only feed reading `store.sdkEvents[id]`;
  replay-once on session adoption via a `get_agent_sdk_transcript` command that
  returns **parsed `Vec<SdkEvent>`** (server-side, skip-bad-line — never the lossy
  raw-bytes PTY transcript reader) **[review I4]**. Rows: `data-testid="sdk-event"`
  + `data-kind="assistant|tool|result|error"`, rendered **plain text via `{{ }}`**
  (no `v-html`, never `renderMarkdown`) **[review M9]**; scrolls internally. Status
  flows through the existing `setStatus`/`agent-exit` path so the tab badge + Stop
  button work unchanged.

## Security

- Key resolved at the call site (reused `resolve_session_env`), env-only on the
  sidecar child; never in argv (`command` is program-only), the goal (argv, not
  secret), the transcript/events (masked at the sink), errors (fixed strings), or
  any frontend payload. Account **required** (no silent ambient identity). Ambient
  `ANTHROPIC_AUTH_TOKEN`/`CLAUDE_CODE_OAUTH_TOKEN` blanked (backend + sidecar).
  Isolated `CLAUDE_CONFIG_DIR`. **Plan-only** → no autonomous edits / side-effecting
  shell. `settingSources: []` so a repo-planted `.claude` config can't grant tools
  or steer auth. Sidecar **stderr is not relayed** to the UI; on nonzero exit a
  fixed `{"type":"error"}` is emitted. `maxTurns: 30` bounds an unbounded loop.
- **New opaque-error branches** (fixed strings, never raw / never the key): spawn
  failure → `"Failed to start the agent sidecar"`; require-account →
  `"This agent requires a provider account"`; resolution/missing-key reuse M10b-2a's
  fixed strings; a malformed NDJSON line → dropped/`SdkLine::Error`, never
  `serde_json::Error` text.
- **Known limitations (documented):** plan mode still lets the agent *read* the
  filesystem (`cwd` is not a sandbox) — reads can't mutate/exfiltrate without Bash,
  and key-redaction covers the key-in-output path; a hostile repo reading and
  printing other files into the plan is a residual info-leak bounded by plan mode.
  macOS target has no `/proc`, so env isn't exposed via procfs.

## Testing

### Rust (pure seams + the piped-spawn analog)
- `redact`: key present → masked; absent → unchanged; empty key → unchanged
  **[review C1: redaction proof]**.
- `parse_sdk_line`: assistant/tool/result/error classified; non-JSON and unknown
  `type` → `Error`, **never panics** **[review I3]**.
- `pump_ndjson`: canned NDJSON incl. a blank line and a garbage line → correct
  events; `saw_result`/`saw_error` flags.
- `sdk_status`: table over `(saw_result, saw_error, exit)` incl. `exited-0-no-result
  → failed`, `error-line → failed` **[review I2]**.
- **Piped-spawn env override + blank-override** (the PTY `spawn_env_*` tests have no
  piped analog) **[review I5]**: `std::process::Command("sh", ["-c", "printf %s
  \"$VAR\""])` with `.stdin(null).stdout(piped)`, poison the parent var, inject a
  different value, assert the injected wins; and the empty-string blank wins over an
  inherited non-empty.
- `resolve_sdk_sidecar` resolver test (`UAW_AGENT_SDK_SIDECAR`).
- `validate_account_required(adapter, account)`: sdk + None → Err; sdk + Some → Ok;
  pty + None → Ok. Sentinel test: the SDK-path errors never contain the key.
- Bump `migrations_are_idempotent` to 12.

### E2e — `e2e/specs/agent-sdk.e2e.ts`
- A **fake bash sidecar** added to `scripts/run-e2e.sh`, set via
  `UAW_AGENT_SDK_SIDECAR`, spawned directly (shebang + `chmod +x`): reads the goal
  from **`$1`** (argv, matching the real contract), emits canned NDJSON on stdout —
  an `assistant`, a `tool`, a deliberate line **echoing its `$ANTHROPIC_API_KEY`
  value** (to prove redaction), a non-JSON garbage line (to prove no-crash), a
  boolean `{"type":"tool","summary":"KEY:set"}` presence marker, and a `result` —
  then exits 0 (NOT `exec cat`):
  ```bash
  cat >/tmp/uaw-fake-sdk <<'SDK'
  #!/usr/bin/env bash
  goal="$1"
  km=KEY:unset; [ -n "${ANTHROPIC_API_KEY:-}" ] && km=KEY:set
  printf '{"type":"assistant","text":"Planning: %s"}\n' "${goal//\"/}"
  printf '{"type":"tool","name":"Read","summary":"README.md"}\n'
  printf '{"type":"tool","name":"echo","summary":"%s"}\n' "${ANTHROPIC_API_KEY:-none}"
  printf 'this line is not json\n'
  printf '{"type":"tool","name":"probe","summary":"%s"}\n' "$km"
  printf '{"type":"result","status":"success","summary":"Done"}\n'
  SDK
  chmod +x /tmp/uaw-fake-sdk
  ```
- `wdio.conf.ts beforeSession`: set `process.env.UAW_AGENT_SDK_SIDECAR =
  "/tmp/uaw-fake-sdk"` alongside the existing overrides.
- Spec: code project + repo + worktree + an Anthropic account → pick the worktree +
  **Claude Agent SDK** adapter + the account + a **single-line** goal (wdio `\n`
  gotcha) → launch → assert the feed (`[data-testid="sdk-event"]`, queried by
  `data-kind` via **separate** selectors — not a combined `[attr] *=text`) shows the
  assistant text + a tool row + a `result` "Done"; assert the raw fixture key value
  **never appears** (redaction) while `KEY:set` is present (injection proven); assert
  selecting the SDK adapter with **no account** blocks launch / errors. Keep the
  sidecar out of the pnpm workspace; **`Dockerfile.e2e` unchanged**.

## Packaging
- `sidecar/claude-agent-sdk/` has its own `package.json` + `node_modules`
  (`@anthropic-ai/claude-agent-sdk` pinned exactly), installed in the app-packaging
  path only — **never** added to the root `package.json`/lockfile/workspace, so the
  Docker e2e (which runs `pnpm install --frozen-lockfile`) never fetches it.
- **Requires Node on the machine** for real runs (documented; macOS dev). If
  `resolve_sdk_sidecar()` / Node is missing, the spawn fails → a fixed
  `"Failed to start the agent sidecar"` surfaced as the session's error event (fail
  legibly, no dead "running" feed) **[review 4b]**.

## Out of scope (later slices)
Edit mode (`acceptEdits` + the resulting **worktree diff** + permission UI),
dispatched-task-as-goal (launch from a Coding/Board worktree row, task title = goal),
completion→review automation, multi-turn steering, a model picker, a rich inspector,
and bundling Node.

## Review findings folded in (traceability)
- Goal via argv not stdin; fake sidecar reads argv (sec/rust C1·C2).
- Plan-only default; require account; redact-at-sink; clear ambient tokens;
  `settingSources:[]`; isolated `CLAUDE_CONFIG_DIR`; `maxTurns`; opaque errors; no
  stderr relay (security C1·C2·C3·I4·I6).
- Status from `result`; byte-oriented reader; enum registry; process-group kill;
  program-only `command`; `start_sdk_session` sharing the exit tail (rust H1·H3·M1·M2·M3·M4).
- Persist `kind`; store-owned event accumulator; parsed-transcript replay; goal
  reset + SDK-only `canStart`; plain-text feed; distinct view not a fake terminal
  (frontend C1·I2·I3·I4·I6·M9).
- Pure `parse_sdk_line`/`pump_ndjson`/`sdk_status`/`redact` tests; piped-spawn env
  test; require-account test; fake-sidecar-via-argv e2e; sidecar out of workspace;
  Docker has Node (testing C1·I2·I3·I4·I5·M6·M8).
- Surface kept in the Agents picker for a standalone plan-only slice (worktree-row
  launch + diff is the edit-mode slice); rendered as a distinct feed for coherence
  (product 2·3·5).
