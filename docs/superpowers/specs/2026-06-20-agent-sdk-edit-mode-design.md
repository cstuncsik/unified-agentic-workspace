# M10b-2b slice 2 — Claude Agent SDK edit mode + review-the-diff

**Status:** design (spec)
**Branch:** `cstuncsik/milestone-10b-2b-slice2`
**Builds on:** slice 1 (`2026-06-19-agent-sdk-sidecar-design.md`, plan-only SDK sidecar, shipped in PR #14)

## Goal

Let the headless Claude Agent SDK agent **apply file edits** to its worktree (not
just plan), then funnel the resulting changes into the existing review flow. The
launch form gains a **Plan / Edit** toggle (default Plan). On completion of an
edit-mode run that changed the worktree, the SDK feed offers a **Review changes**
button that runs the existing `complete_coding_workspace` flow (snapshot → project
check → create Review).

This closes the slice-1 gap: plan-only produced no actionable artifact; edit mode +
review delivers the "agent edits → human reviews the diff" loop.

## Scope

**In scope**
- A `mode` ("plan" | "edit") threaded form → store → command → sidecar.
- Sidecar runs **both** modes under a hardened, explicit tool surface
  (`permissionMode: "dontAsk"` + an `allowedTools` allowlist). Edit mode also adds
  Write/Edit so the agent can change files, plus a PreToolUse hook bounding writes
  to the worktree.
- `mode` persisted on `agent_sessions` (nullable column; NULL for PTY).
- Completion footer in `SdkRunView`: for an **edit** session that finished and left
  the worktree dirty, show "Agent changed N files" + a **Review changes** button →
  `complete_coding_workspace`.
- e2e: fake sidecar writes a file in edit mode; spec asserts the CTA + a persisted
  Review; plan mode stays clean → no CTA.

**Out of scope (later slices)**
- Full Bash / shell autonomy (needs an OS sandbox — see security analysis).
- Auto-navigation from the feed to the Reviews view (cross-view nav stays manual,
  consistent with the board and CodingView).
- Dispatched-task-as-goal, multi-turn steering, model picker, rich inspector.
- Idempotency guard on `complete_coding_workspace` (pre-existing M9 behavior — see
  Known Limitations).
- Bundling Node / the SDK as a packaged resource.

---

## Architecture

### 1. Sidecar (`sidecar/claude-agent-sdk/index.mjs`)

Today the sidecar hardcodes `permissionMode: "plan"` and reads only `argv[2]` (goal).
Slice 2:

- Read `mode` from `argv[3]`; anything other than the literal `"edit"` → `"plan"`
  (fail-safe; the Rust side already normalizes, this is defense-in-depth).
- **Both** modes use `permissionMode: "dontAsk"` + an explicit `allowedTools`
  allowlist. Per the SDK docs, `dontAsk` + `allowedTools` is the documented
  locked-down pattern: *"Listed tools are approved; anything else is denied
  outright"* — no reliance on enumerating every dangerous tool, robust to future
  SDK tools.
  - plan → `allowedTools: ["Read", "Glob", "Grep"]` (read-only; no edits, no shell,
    no egress).
  - edit → `allowedTools: ["Read", "Glob", "Grep", "Edit", "Write"]`.
- Edit mode additionally registers a **PreToolUse hook** (matcher `"Write|Edit"`)
  that resolves the target `file_path` against `process.cwd()` (the worktree) and
  returns `permissionDecision: "deny"` for any path outside it. `dontAsk` skips
  `canUseTool`, so a hook (which runs first and can deny) is the correct mechanism
  to bound writes.
- Preserve slice-1 invariants in **both** branches: `settingSources: []`,
  `maxTurns: 30`, and `env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "",
  CLAUDE_CODE_OAUTH_TOKEN: "" }`.

Shape:

```js
import path from "node:path";
const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const cwd = process.cwd();

const allowedTools =
  mode === "edit"
    ? ["Read", "Glob", "Grep", "Edit", "Write"]
    : ["Read", "Glob", "Grep"];

const withinWorktree = (p) => {
  if (!p) return false;
  const resolved = path.resolve(cwd, p);
  return resolved === cwd || resolved.startsWith(cwd + path.sep);
};
const boundToWorktree = async (input) => {
  const ti = input.tool_input ?? {};
  if (!withinWorktree(ti.file_path ?? ti.path)) {
    return {
      hookSpecificOutput: {
        hookEventName: input.hook_event_name,
        permissionDecision: "deny",
        permissionDecisionReason: "Edits are restricted to the worktree",
      },
    };
  }
  return {};
};

const options = {
  cwd,
  permissionMode: "dontAsk",
  allowedTools,
  settingSources: [],
  maxTurns: 30,
  env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  ...(mode === "edit" && {
    hooks: { PreToolUse: [{ matcher: "Write|Edit", hooks: [boundToWorktree] }] },
  }),
};
```

The message-streaming loop (assistant / tool / result → NDJSON) is unchanged.

> **Behavior note:** plan mode moves from the SDK's `permissionMode: "plan"` to a
> read-only `dontAsk` allowlist. This is stricter (it removes the egress tools
> `WebFetch`/`WebSearch` that plan mode previously left available) and uniform with
> edit mode. The headless feed is unaffected — a read-only agent still reads and
> emits analysis as the "plan."

### 2. Backend plumbing + persistence

- **Migration `0013_agent_session_mode.sql`**: `ALTER TABLE agent_sessions ADD
  COLUMN mode TEXT;` (nullable; NULL for PTY sessions). Mirrors the
  `0012_agent_session_kind` pattern. Bump `migrations_are_idempotent` (workspace.rs)
  to expect `MAX(version) == 13`.
- **`agent_session::create`** gains a `mode: Option<&str>` parameter; add `mode` to
  the `AgentSession` struct, `COLUMNS`, `from_row`, and the INSERT. Update all call
  sites: the PTY branch passes `None`; the SDK branch passes the normalized mode;
  test helpers (`make`, `create_records_kind`, `deleting_account_nulls_session_binding`)
  pass `None`.
- **`sdk::normalize_sdk_mode(mode: Option<&str>) -> &'static str`** (pure,
  unit-tested): `Some("edit") => "edit"`, everything else (`None`, `"plan"`,
  `"EDIT"`, garbage) → `"plan"`. Fail-safe: never silently grant edit. Returning
  `&'static str` (not the caller's bytes) also blocks argv injection via the mode
  slot.
- **`sdk::spawn`** gains a `mode: &str` parameter and appends it as the second argv:
  `cmd.arg(goal).arg(mode)`. Update the two existing spawn unit tests to pass a mode.
- **`start_agent_session`** gains `mode: Option<String>` (9th positional param,
  mirroring `prompt`/`account_id`; the existing `#[allow(clippy::too_many_arguments)]`
  covers it). PTY branch ignores it. SDK branch normalizes via `normalize_sdk_mode`
  and threads it to `start_sdk_session`.
- **`start_sdk_session`** gains `mode: &str`, persists it via `create`, and passes it
  to `sdk::spawn`. Everything else (key redaction, lock discipline, status
  derivation) is mode-independent and unchanged.

The Rust unit test covers **normalization**; the actual `permissionMode`/allowlist/hook
behavior lives in the JS sidecar and is exercised only by the e2e (and only against
the real SDK, which is not in CI — see Testing).

### 3. Frontend — the completion footer

- **`AgentSession` type** gains `mode: string | null`.
- **`api.startAgentSession`** + **`store.start`** gain a `mode: string | null`
  parameter (positional after `prompt`, before `cols`): `(codingWorkspaceId,
  adapterId, accountId, prompt, mode, cols, rows)`.
- **`AgentsView`**: a `<select aria-label="Agent mode">` with Plan / Edit options
  (`v-if="selectedIsSdk"`, default `"plan"` via a `newMode` ref). Reset `newMode` to
  `"plan"` in **both** existing reset watches (the `newAdapterId` watch and the
  `workspaces.currentId` watch), alongside the existing `newGoal`/`newAccountId`
  resets. `mode` does **not** enter `canStart` (it always has a valid default).
  `openTerminal` passes `selectedIsSdk.value ? newMode.value : null`. Update the goal
  placeholder to "What should the agent do?".
- **`SdkRunView`** prop changes from `sessionId: string` to `session: AgentSession`
  (the call site already has `t.session`). It needs `session.status` (completion
  signal), `session.coding_workspace_id` (to query the diff), and `session.mode`
  (to gate the CTA) — none reachable from `sessionId` alone. Events read from
  `store.sdkEvents[props.session.id]`; `onMounted` still replays the transcript.
- Completion logic in `SdkRunView`:
  - `finished = computed(() => props.session.status !== "running")` — authoritative
    (the store-global `agent-exit` listener sets status for every kind).
  - A guarded one-shot `watch(finished, ..., { immediate: true })`: when finished
    **and** `props.session.mode === "edit"`, call `coding.refreshDiff(coding_workspace_id)`
    once (reuses the existing store state in `coding.diffs[id]`; covers reopening an
    already-finished session). Plan sessions never query the diff.
  - `diff = computed(() => coding.diffs[props.session.coding_workspace_id])`.
  - Footer renders only when `props.session.mode === "edit" && finished && diff &&
    !diff.is_clean`:
    - `diff === undefined` → "Checking for changes…"
    - `diff.error` → a small error line (non-blocking)
    - `diff.is_clean` → no footer (agent changed nothing)
    - else → "Agent changed {{ diff.changed_files.length }} files" + **Review
      changes** button.
  - **Review changes** → `coding.complete(coding_workspace_id)` →
    `reviews.insert(review)` → `toast.success("Review created — see Reviews")`, with
    a `completing` ref guard (`:disabled` + "Creating review…" label) to prevent
    double-submit. Mirrors `CodingView.completeAndReview`.
- **No auto-navigation** to Reviews (the toast names the destination, matching
  CodingView). Use `changed_files.length` for the count — for an untracked-only
  edit, `git diff --stat` is empty, so `diff_stat`/`diff_text` are blank;
  `changed_files` (from `git status --porcelain`) is the only reliable count.

---

## Security analysis

Slice-1 invariants carry over unchanged: API key is env-only, masked at the sink
(`sdk::redact`) in every transcript line and `agent-sdk-event`; account required
(fail-closed); ambient `ANTHROPIC_AUTH_TOKEN`/`CLAUDE_CODE_OAUTH_TOKEN` blanked;
fixed opaque error strings. New, edit-specific analysis (verified against the live
Agent SDK docs — permissions, hooks, TypeScript reference):

1. **"Edit-only, no shell" is enforced by an explicit allowlist, not a single
   deny.** `disallowedTools: ["Bash"]` removes only `Bash`; `Monitor`, `PowerShell`,
   `Agent`, `Workflow`, `WebFetch`, `WebSearch`, `NotebookEdit` would remain. Using
   `permissionMode: "dontAsk"` + `allowedTools` (the documented locked-down pattern)
   denies everything not listed — no shell, no egress, no subagents, no notebook —
   robustly and future-proof.
2. **Writes are bounded to the worktree** by the PreToolUse `Write|Edit` hook (deny
   outside `cwd`). `dontAsk` auto-approves listed tools at any path, so this hook is
   what closes the "Write to ~/.ssh" escape. Hooks run first and `deny` wins.
   Residual: symlink/path-confusion canonicalization edge (a worktree symlink
   pointing out) — `path.resolve` handles `..` but not symlinked dirs; bounded
   because there is no egress channel to exfiltrate through.
3. **Injected key cannot reach the diff.** With no shell, the agent cannot read its
   own process env (no `printenv`; macOS has no `/proc`), so it cannot write the
   injected key into a file that would surface in the review diff. Redaction still
   covers the feed/transcript. Residual: an agent could surface *other* repo secrets
   it `Read` into a diff — inherent to any diff of a repo; generic diff
   secret-scanning is deferred.
4. **Check-command tampering is bounded.** `complete_coding_workspace` runs the
   project's test command from the **project DB row** (`project.settings_json`), not
   from any worktree file, and the agent has no DB access. The command still executes
   the worktree's own scripts (e.g. `npm test` runs `package.json`'s script) — the
   inherent M9 property; "Review changes" stays a manual, user-initiated action
   (never auto-run on completion).

Plan mode is hardened the same way (read-only allowlist under `dontAsk`), closing
the egress surface (`WebFetch`/`WebSearch`) that slice-1 plan mode left open.

---

## Known limitations (documented, not bugs)

- **Edit mode can't build, test, run, or self-verify.** The agent applies edits but
  cannot execute anything (no Bash). It writes code it cannot compile or test; the
  first verification is the review-time project check. Surface this near the Edit
  toggle ("Edit mode applies file changes but can't run builds or tests; the review
  verifies"). Full self-correcting autonomy needs the deferred Bash-sandbox slice.
- **`complete_coding_workspace` is non-idempotent.** Each call creates a new Review
  and re-sets the workspace to `needs-review`; an SDK "Review changes" plus a later
  CodingView "Complete and review" produces two reviews. This is pre-existing M9
  behavior shared with CodingView; the action is manual, so double-completion is a
  deliberate user action. No new guard in this slice.
- **An SDK edit run "produces reviewable changes," not "completes" the workspace.**
  The worktree may receive more edits afterward; "Review changes" reuses the full
  completion flow (per the chosen "use the existing review flow"), which moves
  status to `needs-review`.

---

## Testing strategy

**Rust unit tests**
- `normalize_sdk_mode`: `None`/`"plan"`/`"edit"`/`"garbage"`/`"EDIT"` → expected
  (`edit` only for the exact `"edit"`).
- `sdk::spawn` forwards `[goal, mode]` (update the two existing spawn tests to pass a
  mode arg; the `printenv` test still asserts the injected value).
- `agent_session::create` persists `mode` (extend a model test: create an `sdk`
  session with `Some("edit")`, assert round-trip; PTY create with `None`).

**e2e (`e2e/specs/agent-sdk.e2e.ts`, extended; CI uses the fake bash sidecar)**
- Fake sidecar (`scripts/run-e2e.sh`) reads `mode="$2"`; in edit mode writes a
  **relative** file into `cwd` (the worktree) — `printf 'edit' > AGENT_EDIT.md` —
  to simulate an edit, then emits NDJSON. **Keep** the slice-1 raw-key echo line
  (proves redaction). Plan mode (and the existing mode-less slice-1 flow) writes
  nothing → clean worktree.
- New scenarios:
  - **Edit mode**: launch SDK in Edit (`selectByVisibleText("Edit")` on
    `[aria-label="Agent mode"]`, single-line goal), wait for the feed result, then
    `browser.waitUntil` on the scoped "Review changes" CTA (two-hop async: exit
    event → diff fetch → render). Click it; navigate to Reviews
    (`button*=Reviews`); assert `[data-testid="review-row"]` exists (persistence —
    the real assertion, not just CTA presence). Optionally assert the review detail
    lists `AGENT_EDIT.md`.
  - **Plan mode**: clean worktree → assert the "Review changes" CTA is absent.
- Use a fresh worktree per scenario (separate branches, e.g. `feat/sdk-edit` and
  `feat/sdk-plan`) so worktree state can't cross-contaminate. Scope all button
  lookups (no combined `[attr] button*=Text` selectors).

The tool-surface allowlist + the worktree-write hook are **real-SDK behavior** and
are not exercised by the fake sidecar; the e2e proves the *plumbing* (mode reaches
the sidecar, edit dirties the worktree, the CTA fires, a Review persists). This
boundary is stated, not hidden.

---

## Files touched

- `sidecar/claude-agent-sdk/index.mjs` — mode arg, dontAsk + per-mode allowlist,
  edit-mode write hook.
- `src-tauri/src/db/migrations/0013_agent_session_mode.sql` (new) + `db/mod.rs`
  registration.
- `src-tauri/src/models/agent_session.rs` — `mode` column (struct/COLUMNS/from_row/
  create + call sites/tests).
- `src-tauri/src/services/agent/sdk.rs` — `normalize_sdk_mode`, `spawn` mode arg,
  tests.
- `src-tauri/src/commands/agent_sessions.rs` — `start_agent_session` mode param,
  `start_sdk_session` mode param + persist.
- `src-tauri/src/models/workspace.rs` — bump migration count assertion to 13.
- `src/types/agentSession.ts` — `mode`.
- `src/api/agentSessions.ts`, `src/stores/agentSessions.ts` — `mode` plumbing.
- `src/components/AgentsView.vue` — mode select, resets, `:session` prop.
- `src/components/SdkRunView.vue` — `session` prop, completion footer + Review CTA.
- `scripts/run-e2e.sh`, `e2e/specs/agent-sdk.e2e.ts` — edit-mode fake + scenarios.
