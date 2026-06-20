# Claude Agent SDK Edit Mode + Review-the-Diff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the headless Claude Agent SDK agent a Plan/Edit mode toggle; in Edit mode it applies file edits to its worktree, and on completion the feed offers a "Review changes" button that runs the existing completion-to-review flow.

**Architecture:** A `mode` string flows form → store → `start_agent_session` → `start_sdk_session` → `sdk::spawn` → sidecar `argv[3]`. The sidecar runs both modes under an explicit `permissionMode: "dontAsk"` + `allowedTools` allowlist (edit adds Write/Edit + a PreToolUse hook bounding writes to the worktree). `mode` is persisted on `agent_sessions`. `SdkRunView` gates a completion footer on `mode === "edit"` + a dirty worktree, routing to `complete_coding_workspace`.

**Tech Stack:** Rust (rusqlite, Tauri 2), Node (Claude Agent SDK sidecar), Vue 3 + Pinia + TypeScript, WebdriverIO e2e.

**Spec:** `docs/superpowers/specs/2026-06-20-agent-sdk-edit-mode-design.md`

---

## File Structure

- `src-tauri/src/db/migrations/0013_agent_session_mode.sql` (new) — add nullable `mode` column.
- `src-tauri/src/db/mod.rs` — register migration 0013.
- `src-tauri/src/models/workspace.rs` — bump migration-count assertion 12 → 13.
- `src-tauri/src/models/agent_session.rs` — `mode` field through struct/COLUMNS/from_row/create + call sites/tests.
- `src-tauri/src/services/agent/sdk.rs` — `normalize_sdk_mode` (pure), `spawn` mode arg, tests.
- `src-tauri/src/commands/agent_sessions.rs` — `start_agent_session` + `start_sdk_session` mode plumbing.
- `sidecar/claude-agent-sdk/index.mjs` — mode arg, dontAsk + allowlist, edit-mode write hook.
- `src/types/agentSession.ts`, `src/api/agentSessions.ts`, `src/stores/agentSessions.ts` — `mode` plumbing.
- `src/components/AgentsView.vue` — mode `<select>`, resets, `:session` prop.
- `src/components/SdkRunView.vue` — `session` prop + completion footer + Review CTA.
- `scripts/run-e2e.sh`, `e2e/specs/agent-sdk.e2e.ts` — edit-mode fake sidecar + scenarios.

Commands used throughout:
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- Rust lint gate: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- Frontend typecheck: `pnpm typecheck`
- e2e typecheck: `pnpm e2e:typecheck`
- Full e2e: `pnpm e2e:docker`

---

## Task 1: Persist `mode` on agent_sessions

**Files:**
- Create: `src-tauri/src/db/migrations/0013_agent_session_mode.sql`
- Modify: `src-tauri/src/db/mod.rs:9-70` (MIGRATIONS array)
- Modify: `src-tauri/src/models/workspace.rs:144`
- Modify: `src-tauri/src/models/agent_session.rs` (struct, COLUMNS, from_row, create, test helpers)
- Modify: `src-tauri/src/commands/agent_sessions.rs:214-225` and `:362-374` (the two `create` calls)

- [ ] **Step 1: Create the migration**

Create `src-tauri/src/db/migrations/0013_agent_session_mode.sql`:

```sql
-- Per-session SDK permission mode ("plan" | "edit"); NULL for PTY sessions, which
-- have no SDK permission concept. Drives the completion review affordance.
ALTER TABLE agent_sessions ADD COLUMN mode TEXT;
```

- [ ] **Step 2: Register the migration**

In `src-tauri/src/db/mod.rs`, append to the `MIGRATIONS` array (after the version-12 entry at lines 65-69):

```rust
    (
        13,
        "agent_session_mode",
        include_str!("migrations/0013_agent_session_mode.sql"),
    ),
```

- [ ] **Step 3: Bump the idempotency assertion**

In `src-tauri/src/models/workspace.rs:144`, change:

```rust
        assert_eq!(version, 12);
```

to:

```rust
        assert_eq!(version, 13);
```

- [ ] **Step 4: Add `mode` to the model**

In `src-tauri/src/models/agent_session.rs`:

Add the field to the struct (after `kind` at line 18):

```rust
    pub kind: String,
    pub mode: Option<String>,
    pub created_at: String,
    pub updated_at: String,
```

Update `COLUMNS` (lines 23-24) to include `mode`:

```rust
const COLUMNS: &str = "id, workspace_id, coding_workspace_id, adapter_id, command, status, \
                       exit_code, transcript_path, account_id, model_id, kind, mode, created_at, updated_at";
```

Update `from_row` (add after the `kind` line at line 38):

```rust
        kind: row.get("kind")?,
        mode: row.get("mode")?,
        created_at: row.get("created_at")?,
```

Update `create`'s signature (add `mode` after `kind` at line 55) and its INSERT:

```rust
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    adapter_id: &str,
    command: &str,
    transcript_path: &str,
    account_id: Option<&str>,
    model_id: Option<&str>,
    kind: &str,
    mode: Option<&str>,
) -> rusqlite::Result<AgentSession> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO agent_sessions
           (id, workspace_id, coding_workspace_id, adapter_id, command, status,
            exit_code, transcript_path, account_id, model_id, kind, mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', NULL, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id, workspace_id, coding_workspace_id, adapter_id, command,
            transcript_path, account_id, model_id, kind, mode, now
        ],
    )?;
    Ok(get(conn, id)?.expect("agent session exists immediately after insert"))
}
```

- [ ] **Step 5: Update the model test helpers + add a mode round-trip test**

In `src-tauri/src/models/agent_session.rs` tests, update `make` (line 160) to pass `None`:

```rust
    fn make(conn: &Connection, ws: &str, cw: &str) -> AgentSession {
        create(conn, &new_id(), ws, cw, "claude-code", "claude", "/tmp/t.log", None, None, "pty", None).unwrap()
    }
```

Update `create_records_kind` (the `create` call at lines 167-169) to pass `None` as the trailing mode arg:

```rust
        let s = create(
            &conn, &new_id(), &ws, &cw, "claude-agent-sdk", "sidecar", "/tmp/t.log", None, None, "sdk", None,
        )
        .unwrap();
```

Update `deleting_account_nulls_session_binding` (the `create` call at lines 227-239) to pass `None` as the trailing mode arg (after `"pty"`):

```rust
        let s = create(
            &conn,
            &new_id(),
            &ws,
            &cw,
            "claude-code",
            "claude",
            "/tmp/t.log",
            Some(&acct_id),
            Some("some-model"),
            "pty",
            None,
        )
        .unwrap();
```

Add a new test (after `create_records_kind`, around line 174):

```rust
    #[test]
    fn create_persists_mode() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = create(
            &conn, &new_id(), &ws, &cw, "claude-agent-sdk", "sidecar", "/tmp/t.log",
            None, None, "sdk", Some("edit"),
        )
        .unwrap();
        assert_eq!(s.mode.as_deref(), Some("edit"));
        assert_eq!(get(&conn, &s.id).unwrap().unwrap().mode.as_deref(), Some("edit"));
        // PTY sessions carry no mode.
        assert_eq!(make(&conn, &ws, &cw).mode, None);
    }
```

- [ ] **Step 6: Update the two command call sites to pass `None` (temporary for PTY + SDK)**

In `src-tauri/src/commands/agent_sessions.rs`, the PTY-branch `create` call (lines 214-226) — add `None` after `"pty"`:

```rust
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &program,
            &transcript_str,
            account_row_id,
            None,
            "pty",
            None,
        )
        .map_err(|e| e.to_string())?
```

And the SDK-branch `create` call inside `start_sdk_session` (lines 362-374) — add `None` after `"sdk"` (Task 4 replaces this with the real mode):

```rust
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &sidecar,
            &transcript_str,
            account_row_id.as_deref(),
            None,
            "sdk",
            None,
        )
        .map_err(|e| e.to_string())?
```

- [ ] **Step 7: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent_session migrations_are_idempotent`
Expected: PASS (incl. the new `create_persists_mode` and the version-13 idempotency assertion).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/db src-tauri/src/models/agent_session.rs src-tauri/src/models/workspace.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): persist agent_sessions.mode column"
```

---

## Task 2: `normalize_sdk_mode` pure helper

**Files:**
- Modify: `src-tauri/src/services/agent/sdk.rs` (add fn + test)

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/services/agent/sdk.rs`, add inside the `#[cfg(test)] mod tests` block (after `redact_masks_only_when_present`):

```rust
    #[test]
    fn normalize_mode_fails_safe_to_plan() {
        assert_eq!(normalize_sdk_mode(Some("edit")), "edit");
        assert_eq!(normalize_sdk_mode(Some("plan")), "plan");
        assert_eq!(normalize_sdk_mode(None), "plan");
        // Fail safe: case-sensitive, and anything unrecognized is plan, never edit.
        assert_eq!(normalize_sdk_mode(Some("EDIT")), "plan");
        assert_eq!(normalize_sdk_mode(Some("garbage")), "plan");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml normalize_mode_fails_safe_to_plan`
Expected: FAIL to compile — `cannot find function normalize_sdk_mode`.

- [ ] **Step 3: Implement the helper**

In `src-tauri/src/services/agent/sdk.rs`, add after the `redact` function (after line 20):

```rust
/// Normalize a caller-supplied mode to the sidecar contract. Unknown/None → "plan"
/// (fail safe: never silently grant edit). Returns 'static so a caller cannot smuggle
/// arbitrary argv into the sidecar through the mode slot.
pub fn normalize_sdk_mode(mode: Option<&str>) -> &'static str {
    match mode {
        Some("edit") => "edit",
        _ => "plan",
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml normalize_mode_fails_safe_to_plan`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs
git commit -m "feat(m10b-2b): normalize_sdk_mode fail-safe helper"
```

---

## Task 3: `sdk::spawn` forwards the mode as the second argv

**Files:**
- Modify: `src-tauri/src/services/agent/sdk.rs:132-165` (spawn) + its tests
- Modify: `src-tauri/src/commands/agent_sessions.rs:354-358` (call site — temporary `"plan"`)

- [ ] **Step 1: Change `spawn` to take a mode arg**

In `src-tauri/src/services/agent/sdk.rs`, update `spawn` (lines 132-143). Change the signature and add `.arg(mode)`:

```rust
/// Spawn the sidecar as a plain piped child in `cwd`; goal as argv[2], mode as
/// argv[3], env injected, stdin null (the goal is argv, not stdin), stderr
/// discarded (never relayed).
pub fn spawn(
    program: &str,
    goal: &str,
    mode: &str,
    cwd: &Path,
    env: &[(String, String)],
) -> Result<SdkSpawned, String> {
    let mut cmd = Command::new(program);
    cmd.arg(goal)
        .arg(mode)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
```

(The rest of `spawn` is unchanged.)

- [ ] **Step 2: Update the call site to keep the tree compiling**

In `src-tauri/src/commands/agent_sessions.rs`, update the `sdk::spawn` call (line 354-358). Pass a literal `"plan"` for now (Task 4 wires the real mode):

```rust
    let sdk::SdkSpawned {
        stdout,
        mut child,
        handle,
    } = sdk::spawn(&sidecar, &goal, "plan", Path::new(&worktree_path), &sdk_env)?;
```

- [ ] **Step 3: Update the two existing spawn tests + add a mode-forwarding test**

In `src-tauri/src/services/agent/sdk.rs` tests:

`spawn_injects_env_overriding_inherited` (line 228-234) — add `"plan"` after the goal arg:

```rust
        let mut sp = spawn(
            "printenv",
            "UAW_SDK_PROBE", // argv (the goal slot) = the var name printenv echoes
            "plan",          // mode slot (printenv ignores the extra unset name)
            &dir,
            &[("UAW_SDK_PROBE".into(), "INJECTED".into())],
        )
        .expect("spawn printenv");
```

`spawn_missing_program_is_opaque` (line 244) — add `"plan"`:

```rust
        let err = match spawn("/no/such/sidecar-xyz", "goal", "plan", &std::env::temp_dir(), &[]) {
```

Add a new test (after `spawn_injects_env_overriding_inherited`):

```rust
    #[test]
    fn spawn_forwards_mode_as_second_arg() {
        let dir = std::env::temp_dir();
        // `echo` joins its argv with spaces, so the goal + mode round-trip on stdout.
        let mut sp = spawn("echo", "GOAL", "edit", &dir, &[]).expect("spawn echo");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        assert_eq!(out.trim(), "GOAL edit");
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib sdk`
Expected: PASS (incl. `spawn_forwards_mode_as_second_arg`; `printenv` test still asserts `INJECTED`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): sdk::spawn forwards mode as argv[3]"
```

---

## Task 4: Thread `mode` through the commands

**Files:**
- Modify: `src-tauri/src/commands/agent_sessions.rs:136-197` (`start_agent_session`)
- Modify: `src-tauri/src/commands/agent_sessions.rs:323-374` (`start_sdk_session`)

- [ ] **Step 1: Add the `mode` parameter to `start_agent_session` and thread it to the SDK branch**

In `src-tauri/src/commands/agent_sessions.rs`, add `mode: Option<String>` to the signature (after `prompt`, line 144):

```rust
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    adapter_id: String,
    account_id: Option<String>,
    prompt: Option<String>,
    mode: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<AgentSession, String> {
```

Update the SDK-branch dispatch (lines 182-197) to normalize + pass the mode:

```rust
    // Headless SDK adapters take a different runtime (piped child + NDJSON).
    if adapter.kind == "sdk" {
        return start_sdk_session(
            app,
            state,
            adapter,
            env,
            account_row_id.map(|s| s.to_string()),
            workspace_id,
            worktree_path,
            coding_workspace_id,
            prompt.unwrap_or_default(),
            sdk::normalize_sdk_mode(mode.as_deref()),
            id,
            transcript_path,
            transcript_str,
        );
    }
```

- [ ] **Step 2: Add `mode` to `start_sdk_session`, persist it, and pass it to spawn**

In `src-tauri/src/commands/agent_sessions.rs`, update `start_sdk_session`'s signature (add `mode: &str` after `goal`, line 333):

```rust
#[allow(clippy::too_many_arguments)]
fn start_sdk_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    adapter: AgentAdapter,
    env: Vec<(String, String)>,
    account_row_id: Option<String>,
    workspace_id: String,
    worktree_path: String,
    coding_workspace_id: String,
    goal: String,
    mode: &str,
    id: String,
    transcript_path: PathBuf,
    transcript_str: String,
) -> Result<AgentSession, String> {
```

Replace the temporary `"plan"` in the spawn call (from Task 3 Step 2) with `mode`:

```rust
    } = sdk::spawn(&sidecar, &goal, mode, Path::new(&worktree_path), &sdk_env)?;
```

Replace the `None` mode in the SDK-branch `create` call (from Task 1 Step 6) with `Some(mode)`:

```rust
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &sidecar,
            &transcript_str,
            account_row_id.as_deref(),
            None,
            "sdk",
            Some(mode),
        )
        .map_err(|e| e.to_string())?
```

- [ ] **Step 3: Build, test, and lint**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (the whole suite compiles and is green; the SDK command now defaults to plan when no mode is sent).

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): thread mode through start_agent_session to the sidecar"
```

---

## Task 5: Sidecar edit mode (dontAsk + allowlist + worktree-write hook)

**Files:**
- Modify: `sidecar/claude-agent-sdk/index.mjs` (full rewrite of the options builder)

- [ ] **Step 1: Rewrite the sidecar**

Replace the entire contents of `sidecar/claude-agent-sdk/index.mjs` with:

```js
#!/usr/bin/env node
// Headless Claude Agent SDK runner. Goal via argv[2], mode via argv[3]
// ("plan" | "edit"); key via env (injected by the backend). Emits one compact
// NDJSON object per message on stdout.
import path from "node:path";
import { query } from "@anthropic-ai/claude-agent-sdk";

const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const cwd = process.cwd();
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");

// Explicit tool surface: dontAsk + an allowlist denies everything not listed (no
// shell, no egress, no subagents) — the SDK's documented locked-down pattern. Edit
// mode adds Write/Edit; plan mode is read-only.
const allowedTools =
  mode === "edit"
    ? ["Read", "Glob", "Grep", "Edit", "Write"]
    : ["Read", "Glob", "Grep"];

// Bound Write/Edit to the worktree (cwd). dontAsk skips canUseTool, so a PreToolUse
// hook (runs first, can deny) is the mechanism that scopes writes.
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
  // Spread our env so the grandchild CLI inherits the injected key, and blank
  // ambient tokens that would otherwise outrank it.
  env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  ...(mode === "edit" && {
    hooks: { PreToolUse: [{ matcher: "Write|Edit", hooks: [boundToWorktree] }] },
  }),
};

try {
  for await (const m of query({ prompt: goal, options })) {
    if (m.type === "assistant") {
      for (const block of m.message?.content ?? []) {
        if (block.type === "text" && block.text) {
          emit({ type: "assistant", text: block.text });
        } else if (block.type === "tool_use") {
          emit({
            type: "tool",
            name: block.name,
            summary: JSON.stringify(block.input ?? {}).slice(0, 200),
          });
        }
      }
    } else if (m.type === "result") {
      emit({
        type: "result",
        status: m.subtype === "success" && !m.is_error ? "success" : "error",
        summary: typeof m.result === "string" ? m.result : "",
      });
    }
  }
} catch {
  emit({ type: "error", message: "Agent run failed" });
  process.exit(1);
}
```

- [ ] **Step 2: Syntax-check the sidecar**

Run: `node --check sidecar/claude-agent-sdk/index.mjs`
Expected: no output, exit 0 (valid syntax). (The real SDK behavior is not exercised in CI; the fake sidecar in Task 9 covers the plumbing.)

- [ ] **Step 3: Commit**

```bash
git add sidecar/claude-agent-sdk/index.mjs
git commit -m "feat(m10b-2b): sidecar edit mode — dontAsk allowlist + worktree-write hook"
```

---

## Task 6: Frontend types + api + store plumbing

**Files:**
- Modify: `src/types/agentSession.ts:31-45` (AgentSession)
- Modify: `src/api/agentSessions.ts:12-28` (startAgentSession)
- Modify: `src/stores/agentSessions.ts:68-81` (start)

- [ ] **Step 1: Add `mode` to the AgentSession type**

In `src/types/agentSession.ts`, add `mode` to `AgentSession` (after `kind` at line 42):

```ts
  kind: string; // "pty" | "sdk"
  mode: string | null; // "plan" | "edit" for sdk; null for pty
  created_at: string;
  updated_at: string;
```

- [ ] **Step 2: Add `mode` to the api call**

In `src/api/agentSessions.ts`, update `startAgentSession` (lines 12-28):

```ts
export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  accountId: string | null,
  prompt: string | null,
  mode: string | null,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", {
    codingWorkspaceId,
    adapterId,
    accountId,
    prompt,
    mode,
    cols,
    rows,
  });
}
```

- [ ] **Step 3: Thread `mode` through the store**

In `src/stores/agentSessions.ts`, update `start` (lines 68-81):

```ts
  async function start(
    codingWorkspaceId: string,
    adapterId: string,
    accountId: string | null,
    prompt: string | null,
    mode: string | null,
    cols: number,
    rows: number,
  ) {
    await ensureListeners();
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, accountId, prompt, mode, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }
```

- [ ] **Step 4: Typecheck**

Run: `pnpm typecheck`
Expected: PASS (no type errors). Note: `AgentsView.openTerminal` still calls `store.start` with the old arity and will be a type error until Task 7 — so run typecheck after Task 7 if it fails here. To keep this task self-contained, proceed to Task 7 before typechecking, or temporarily verify with `pnpm typecheck` expecting the single known AgentsView arity error.

- [ ] **Step 5: Commit**

```bash
git add src/types/agentSession.ts src/api/agentSessions.ts src/stores/agentSessions.ts
git commit -m "feat(m10b-2b): thread mode through frontend api + store"
```

---

## Task 7: AgentsView mode select + resets + `:session` prop

**Files:**
- Modify: `src/components/AgentsView.vue` (refs, watches, template select, openTerminal, SdkRunView prop)

- [ ] **Step 1: Add the `newMode` ref**

In `src/components/AgentsView.vue`, add after `const newGoal = ref("");` (line 20):

```ts
const newMode = ref("plan");
```

- [ ] **Step 2: Reset `newMode` in both reset watches**

In the `watch(newAdapterId, ...)` body (lines 55-58), add the reset:

```ts
watch(newAdapterId, () => {
  newAccountId.value = "";
  newGoal.value = "";
  newMode.value = "plan";
});
```

In the `watch(() => workspaces.currentId, ...)` body (after the `newGoal.value = "";` at line 78), add:

```ts
    newWorktreeId.value = "";
    newAccountId.value = "";
    newGoal.value = "";
    newMode.value = "plan";
```

- [ ] **Step 3: Add the mode `<select>` to the form**

In the template, insert the mode select immediately before the goal `<textarea>` (before line 168). Both share the `selectedIsSdk` guard:

```html
        <select
          v-if="selectedIsSdk"
          v-model="newMode"
          class="re-select"
          data-size="sm"
          aria-label="Agent mode"
        >
          <option value="plan">Plan</option>
          <option value="edit">Edit</option>
        </select>
        <textarea
          v-if="selectedIsSdk"
          v-model="newGoal"
          class="re-input new__goal"
          rows="2"
          placeholder="What should the agent do?"
          aria-label="Agent goal"
        ></textarea>
        <p v-if="selectedIsSdk && newMode === 'edit'" class="muted new__hint">
          Edit mode applies file changes but can't run builds or tests; the review verifies.
        </p>
```

(The textarea block replaces the existing one at lines 168-175 — note the placeholder change from "What should the agent plan?". The `new__hint` paragraph is new.)

Add the hint style to the `<style scoped>` block (after the `.new__goal` rule at lines 275-278):

```css
.new__hint {
  flex-basis: 100%;
  font-size: 0.8rem;
  margin: 0;
}
```

- [ ] **Step 4: Pass `mode` from `openTerminal`**

Update the `store.start` call in `openTerminal` (lines 95-102):

```ts
    await store.start(
      newWorktreeId.value,
      newAdapterId.value,
      newAccountId.value || null,
      selectedIsSdk.value ? newGoal.value.trim() || null : null,
      selectedIsSdk.value ? newMode.value : null,
      80,
      24,
    );
```

- [ ] **Step 5: Pass the session object to SdkRunView**

Update the `SdkRunView` render (line 218):

```html
        <SdkRunView v-if="t.session.kind === 'sdk'" :session="t.session" />
```

- [ ] **Step 6: Typecheck**

Run: `pnpm typecheck`
Expected: `SdkRunView` will error ("missing prop session" / "property session-id does not exist") until Task 8 changes its props. This is the single expected error; it clears after Task 8. (Do not "fix" it here — Task 8 owns SdkRunView.)

- [ ] **Step 7: Commit**

```bash
git add src/components/AgentsView.vue
git commit -m "feat(m10b-2b): Plan/Edit mode select in the agent launch form"
```

---

## Task 8: SdkRunView completion footer + Review CTA

**Files:**
- Modify: `src/components/SdkRunView.vue` (prop change + footer)

- [ ] **Step 1: Rewrite SdkRunView**

Replace the contents of `src/components/SdkRunView.vue` with:

```vue
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useReviewsStore } from "../stores/reviews";
import { useToast } from "../composables/useToast";
import type { AgentSession, SdkEvent } from "../types/agentSession";

const props = defineProps<{ session: AgentSession }>();
const store = useAgentSessionsStore();
const coding = useCodingWorkspacesStore();
const reviews = useReviewsStore();
const toast = useToast();

const events = computed(() => store.sdkEvents[props.session.id] ?? []);
onMounted(() => store.loadSdkTranscript(props.session.id));

const tag = (e: SdkEvent) =>
  e.type === "tool" ? `🔧 ${e.name ?? "tool"}` : e.type === "result" ? "✓" : e.type === "error" ? "✗" : "";
const text = (e: SdkEvent) => e.text ?? e.summary ?? e.message ?? "";

// Completion + review-the-diff. Only edit-mode sessions can dirty the worktree, so
// only they query the diff and offer a review — a plan run over a pre-dirty worktree
// must not falsely offer one.
const isEdit = computed(() => props.session.mode === "edit");
const finished = computed(() => props.session.status !== "running");
const diff = computed(() => coding.diffs[props.session.coding_workspace_id]);
const changedCount = computed(() => diff.value?.changed_files.length ?? 0);
const showReview = computed(
  () => isEdit.value && finished.value && !!diff.value && !diff.value.is_clean && !diff.value.error,
);
const completing = ref(false);

// One-shot: when an edit session finishes, fetch the worktree diff once (covers
// reopening an already-finished session via immediate).
watch(
  finished,
  async (done) => {
    if (done && isEdit.value) {
      await coding.refreshDiff(props.session.coding_workspace_id);
    }
  },
  { immediate: true },
);

async function reviewChanges() {
  if (completing.value) return;
  completing.value = true;
  try {
    const review = await coding.complete(props.session.coding_workspace_id);
    reviews.insert(review);
    toast.success("Review created — see Reviews");
  } catch (e) {
    toast.error(String(e));
  } finally {
    completing.value = false;
  }
}
</script>

<template>
  <div class="sdk-wrap">
    <div class="sdk-feed" data-testid="agent-sdk-feed">
      <div
        v-for="(e, i) in events"
        :key="i"
        class="sdk-row"
        data-testid="sdk-event"
        :data-kind="e.type"
      >
        <span class="sdk-row__tag">{{ tag(e) }}</span>
        <span class="sdk-row__text">{{ text(e) }}</span>
      </div>
      <p v-if="events.length === 0" class="muted">Waiting for the agent…</p>
    </div>
    <footer v-if="showReview" class="sdk-foot" data-testid="sdk-review-cta">
      <span>Agent changed {{ changedCount }} file{{ changedCount === 1 ? "" : "s" }}</span>
      <button
        type="button"
        class="re-button"
        data-variant="brand"
        data-size="sm"
        :disabled="completing"
        @click="reviewChanges"
      >
        {{ completing ? "Creating review…" : "Review changes" }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.sdk-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.sdk-feed {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.sdk-row {
  display: flex;
  gap: 0.5rem;
  font-size: 0.85rem;
}
.sdk-row[data-kind="error"] {
  color: var(--re-color-danger-text);
}
.sdk-row__tag {
  flex-shrink: 0;
}
.sdk-row__text {
  white-space: pre-wrap;
  word-break: break-word;
}
.sdk-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.5rem;
  border-top: 1px solid var(--re-color-border);
  font-size: 0.85rem;
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
```

- [ ] **Step 2: Typecheck**

Run: `pnpm typecheck`
Expected: PASS (the AgentsView ↔ SdkRunView prop mismatch from Task 7 is now resolved, and Task 6's store arity matches).

- [ ] **Step 3: Commit**

```bash
git add src/components/SdkRunView.vue
git commit -m "feat(m10b-2b): SdkRunView completion footer + Review changes CTA"
```

---

## Task 9: e2e — fake sidecar edit write + edit/plan scenarios

**Files:**
- Modify: `scripts/run-e2e.sh:69-80` (fake SDK sidecar)
- Modify: `e2e/specs/agent-sdk.e2e.ts` (assertions + new scenarios)

- [ ] **Step 1: Teach the fake sidecar about edit mode**

In `scripts/run-e2e.sh`, replace the fake-sdk heredoc (lines 69-80) with:

```bash
cat >/tmp/uaw-fake-sdk <<'SDK'
#!/usr/bin/env bash
goal="$1"
mode="${2:-plan}"
km=KEY:unset; [ -n "${ANTHROPIC_API_KEY:-}" ] && km=KEY:set
# In edit mode, simulate an agent edit by writing an untracked file into the
# worktree (cwd is the worktree: the backend sets current_dir). Relative path only,
# never escaping the worktree. Plan mode leaves the tree clean.
if [ "$mode" = "edit" ]; then
  printf 'edited by fake sdk\n' > AGENT_EDIT.md
fi
printf '{"type":"assistant","text":"Planning: %s"}\n' "${goal//\"/}"
printf '{"type":"tool","name":"Read","summary":"README.md"}\n'
printf '{"type":"tool","name":"echo","summary":"%s"}\n' "${ANTHROPIC_API_KEY:-none}"
printf 'this line is not json\n'
printf '{"type":"tool","name":"probe","summary":"%s"}\n' "$km"
printf '{"type":"result","status":"success","summary":"Done"}\n'
SDK
chmod +x /tmp/uaw-fake-sdk
```

- [ ] **Step 2: Assert plan mode offers no review CTA**

In `e2e/specs/agent-sdk.e2e.ts`, at the end of the existing `it("runs a plan-only SDK session, ...")` test (after line 87, the `expect(text).not.toContain(KEY_VALUE);`), add:

```ts
    // Plan mode never edits, so the worktree stays clean → no review affordance.
    expect((await $$('[data-testid="sdk-review-cta"]')).length).toBe(0);
```

- [ ] **Step 3: Add the edit-mode worktree + scenario**

In `e2e/specs/agent-sdk.e2e.ts`, add two new `it` blocks before the closing `});` of the `describe` (after the existing `it("requires an account for the SDK adapter", ...)` at line 95):

```ts
  it("creates a second worktree for an edit-mode run", async () => {
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("SdkProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("SdkFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/sdk-edit");
    await (await $("button*=Create worktree")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]')).length >= 2, {
      timeout: 15_000,
      timeoutMsg: "expected a second worktree row",
    });
  });

  it("edit mode changes the worktree and offers a review that persists", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk-edit");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");
    await (await $('[aria-label="Agent mode"]')).selectByVisibleText("Edit");
    await (await $('[aria-label="Agent goal"]')).setValue("edit the readme");
    await (await $("button*=New terminal")).click();

    // The CTA appears only on the edit tab, and only after the run finishes and the
    // worktree diff resolves (exit event → diff fetch → render). Waiting on it is
    // unambiguous even with the earlier plan tab still mounted.
    const cta = await $('[data-testid="sdk-review-cta"]');
    await cta.waitForExist({ timeout: 20_000 });
    expect(await cta.getText()).toContain("changed 1 file");

    // Scope the button lookup to the footer (a combined `[attr] button*=Text` string
    // is not a valid wdio selector).
    await cta.$("button*=Review changes").click();

    // The review was created by the existing completion flow and persists — find it
    // in the Reviews view.
    await (await $("button*=Reviews")).click();
    await (await $('[data-testid="review-row"]')).waitForExist({ timeout: 10_000 });
  });
```

- [ ] **Step 4: e2e typecheck**

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 5: Run the full e2e suite**

Run: `pnpm e2e:docker`
Expected: PASS — all specs green, including the existing plan-only SDK spec (now with the no-CTA assertion) and the new edit-mode spec (CTA shows "changed 1 file", Review changes creates a persisted review row).

- [ ] **Step 6: Commit**

```bash
git add scripts/run-e2e.sh e2e/specs/agent-sdk.e2e.ts
git commit -m "test(m10b-2b): edit-mode fake sidecar + edit/plan review e2e"
```

---

## Final verification

- [ ] **Run the full Rust suite + lint**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: all green, no warnings.

- [ ] **Run frontend typecheck + the full e2e**

Run: `pnpm typecheck`
Run: `pnpm e2e:docker`
Expected: all green.
