# Dispatch task → SDK agent goal — Design

**Goal:** When a user opens a Claude Agent SDK session on a *dispatched* worktree, pre-fill the agent's goal with the dispatched task title plus the source artifact's content — so a researched plan flows straight into SDK-agent execution instead of being retyped.

**Milestone:** M10b-2b (the deferred "dispatched-task-as-goal", from slices 1–2).

**Status:** Approved design (post multi-discipline review). Ready for an implementation plan.

---

## Background

Dispatching an artifact (`dispatch_artifact`) creates, per included task:

- a planning **Session** whose `title` is the one-line task and whose `created_from_artifact_id` points at the source artifact, and
- a git worktree (**coding_workspace**) whose `session_id` points at that session.

So the chain `coding_workspace.session_id → session.title + session.created_from_artifact_id → artifact.content` already exists in the DB. What's missing: when the user later opens an SDK agent on that worktree (AgentsView), the goal textarea is **empty** — the researched plan does not carry over.

`coding_workspace.session_id` is set only through `create_worktree_inner`; the M7 "Create worktree" path passes `None`, dispatch passes `Some(session.id)`. A plain worktree therefore has no session and seeds nothing.

## Decisions (fixed by the product owner)

1. **Goal content** = task title **+ the full artifact content** (not a title alone, not a per-task section).
2. **Trigger** = auto-prefill the existing Agents launch form's goal textarea. No cross-view navigation (deferred).
3. **Resolution** = on-demand backend read (no migration; always reflects the artifact's *current* content).
4. **Scope** = the SDK adapter only. PTY/CLI agents type into the terminal and are out of scope.

---

## Architecture & data flow

```
AgentsView: user selects an SDK adapter + a dispatched worktree
        │  (watch newWorktreeId / newAdapterId)
        ▼
api.getDispatchedGoal(codingWorkspaceId)         ── invoke ──▶  #[command] get_dispatched_goal
        │                                                              │ one short Mutex<Connection> lock
        │                                                              ▼
        │                                            services::dispatch::resolve_dispatched_goal(conn, cw_id)
        │                                              cw.session_id → session.title
        │                                              session.created_from_artifact_id → artifact.content
        │                                              → Some(format_dispatched_goal(title, content)) | None
        ▼
prefillGoal(): seed newGoal (dirty-checked, race-guarded, error-swallowed)
```

A single unit of work, resolved on demand. No new table or column; no network; no secret.

---

## Backend

### `services/dispatch.rs` (currently pure helpers; extend, mirroring `services/board.rs`)

The codebase puts conn-testable, multi-model read assembly in `services/` (see `services/board.rs::assemble_cards`), with the thin `#[tauri::command]` wrapper in `commands/`. We follow that split. Update the module's doc comment from "No IO" to "pure helpers + the conn-testable goal resolver".

**Pure formatter** (unit-tested beside `extract_tasks`):

```
format_dispatched_goal(task_title: &str, artifact_content: &str) -> String
```

Produces exactly:

```
Task: {task_title}

Context — the plan this task was dispatched from:

{artifact_content}
```

The `Task:` line frames the specific task; the `Context —` header marks the rest as the plan. (The title legitimately also appears inside the artifact — it was extracted from it — which is fine: N worktrees dispatched from one artifact each show their own `Task:` line above the shared plan.) Take `&str` (borrow, no forced clone); the title is trimmed by the resolver, content is passed through as-is (its internal markdown structure matters).

**Resolver** (conn-testable, `migrated_conn` harness copied from `services/board.rs` tests):

```
resolve_dispatched_goal(conn: &Connection, coding_workspace_id: &str)
    -> rusqlite::Result<Option<String>>
```

Degrade ladder — return `Some(format_dispatched_goal(title, content))` **iff** the whole chain resolves *and* both fields are non-empty after trim; otherwise `Ok(None)`:

| State | Result |
|---|---|
| cw missing | `None` |
| `cw.session_id` is `None` (plain worktree) | `None` |
| session row missing | `None` |
| `session.created_from_artifact_id` is `None` (artifact deleted post-dispatch — `ON DELETE SET NULL` — or never linked) | `None` |
| artifact row missing | `None` |
| `session.title` empty/whitespace **or** `artifact.content` empty/whitespace | `None` |
| full chain resolves, both non-empty | `Some(goal)` |

This rule is deliberately strict — seed only when there is a real dispatched artifact with content. It drops the originally-sketched "title-only" degrade (marginal value; the review noted a "dangling artifact id" is impossible under `SET NULL`, so the only no-artifact state is `created_from_artifact_id = None`, which is rare and handled as "no prefill"). It needs no assumption about which flow set `session_id`: anything lacking resolvable artifact content yields `None`.

### `commands/dispatch.rs` — thin command (mirrors `get_board`)

```
#[tauri::command]
pub fn get_dispatched_goal(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
) -> Result<Option<String>, String>
```

Acquire one short lock, call `resolve_dispatched_goal`, `.map_err(|e| e.to_string())`. Only DB reads under the lock — no keychain, subprocess, or network IO. Register `commands::dispatch::get_dispatched_goal` in the `lib.rs` `invoke_handler!` list beside `dispatch_artifact`.

**Error handling:** the standard `.map_err(|e| e.to_string())`, consistent with sibling read commands (`get_board`, `get_session`). The fixed-opaque-string convention is reserved for *credential* paths (keystore, model listing); this read touches no secret, so a raw SQL error string is acceptable and consistent. (The frontend additionally swallows the error — see below — so prefill stays best-effort.)

**No workspace-scoping — and a doc comment explaining why.** Unlike `list_account_models` (which is workspace-scoped to gate cross-workspace *account/credential* access), this read only touches objects transitively owned by the cw (`cw → its session → that session's artifact`). There is no credential boundary to enforce. A one-line comment records this so the asymmetry with `list_account_models` is not "fixed" later.

**Security note (doc comment):** the goal — including the full artifact — reaches the SDK sidecar as `argv[2]`, as goals already do. Acceptable: single-user local host, the artifact is the user's own data, and credentials never travel in argv (they are env-only and redacted at the sink). The frontend large-goal warning (below) mitigates the practical `argv`-size failure mode.

---

## Frontend

### `api/codingWorkspaces.ts`

```ts
export function getDispatchedGoal(id: string): Promise<string | null> {
  return invoke<string | null>("get_dispatched_goal", { codingWorkspaceId: id });
}
```

Called **directly** from AgentsView (no store passthrough — the coding store does not wrap pure, stateless reads; cf. `getCodingWorkspace`, and `ArtifactsView` calling `dispatchApi.*` directly).

### `components/AgentsView.vue`

The goal textarea is already `v-if="selectedIsSdk"` bound to `newGoal`, so prefill is SDK-only by construction. Add:

**State**
- `seededValue = ref("")` — the value we last prefilled, for dirty-checking. `""` means "not currently seeded".
- `seeded = computed(() => newGoal.value === seededValue.value && seededValue.value !== "")` — true while the box holds an unedited seed; drives the hint and the `rows` bump below.
- a module-scoped monotonic `goalToken` (the `codingWorkspaces.loadToken` idiom).

**Helper**

```ts
async function prefillGoal(id: string) {
  if (!id) return;
  const token = ++goalToken;
  let goal: string | null = null;
  try {
    goal = await codingApi.getDispatchedGoal(id);
  } catch {
    return; // best-effort: a failed fetch never clobbers or toasts
  }
  if (token !== goalToken) return;                 // a newer prefill superseded us
  if (newWorktreeId.value !== id || !selectedIsSdk.value) return;
  // Dirty-check: only (re)seed a pristine or still-seeded box — never stomp edits.
  if (newGoal.value === seededValue.value || newGoal.value.trim() === "") {
    newGoal.value = goal ?? "";
    seededValue.value = goal ?? "";
  }
}
```

**Watch wiring** (extends the slice-3 model-load watches):
- `watch(newWorktreeId, val)`: when `val && selectedIsSdk` → existing `accountModels.loadModels(...)` (when an account is set) **and** `prefillGoal(val)`.
- `watch(newAdapterId, …)`: change the handler to receive the new id — `watch(newAdapterId, (val) => { …existing resets… })`. After the resets (which already set `newGoal = ""`; also reset `seededValue = ""`), add `if (adapterKind(val) === "sdk" && newWorktreeId.value) prefillGoal(newWorktreeId.value)`. Use `adapterKind(val)` — the pure `(id) => kind` helper — not `selectedIsSdk.value`, to avoid coupling correctness to lazy-computed evaluation timing.
- `watch(workspaces.currentId)`: the existing reset already clears `newGoal`; also clear `seededValue = ""`.

**Behaviour:** the goal follows the selected SDK worktree — a dispatched worktree fills it, a plain worktree clears it — **except** once the user edits the box (`newGoal ≠ seededValue`), their text is protected from any reseed/clear. Switching worktrees is deliberate, so reseeding a pristine box is expected; protecting edits prevents an "it ate my prompt" surprise.

**Three small hints** (reuse the existing `.new__hint` pattern; all SDK-only):
1. **Seeded indicator** — when the box holds an unedited seed (`newGoal === seededValue && seededValue !== ""`): *"Prefilled from the dispatched task — editable."* Converts the silent prefill into an understood feature; disappears once edited.
2. **Reviewable size** — bump the textarea when seeded: `:rows="seeded ? 8 : 2"`, so a full plan is visible rather than a two-line porthole. (`.new__goal` is already `resize: vertical`.)
3. **Large-goal warning** — when the SDK goal's byte length exceeds ~100 KB: *"Plan is large (~N KB) — trim before starting; very large goals can fail to launch."* The goal becomes `argv[2]`; an oversized argument makes `spawn` fail as the opaque "Failed to start the agent sidecar". No truncation, no backend change — a computed warning that gives the user a path forward.

`openTerminal` is unchanged: it already sends `newGoal.value.trim() || null` as the prompt for SDK sessions. Prefill is a **seed, not a binding** — the user edits freely and the edited value is what launches.

---

## Testing

### Rust unit (in `services/dispatch.rs` tests, `migrated_conn` copied from `services/board.rs`)
- `format_dispatched_goal`: exact assembled string (pin the blank-line separators); a multibyte title/content case (match the `extract_tasks` non-ASCII rigor); confirm it is a pure function with no surprises.
- `resolve_dispatched_goal` over the full ladder, each branch its own case:
  - plain cw (`session_id` None) → `None` — the semantic boundary (dispatched vs ordinary worktree).
  - session present but `created_from_artifact_id` None → `None`.
  - empty-content artifact → `None`.
  - full chain with non-empty title + content → `Some(expected exact goal)`.
  - (missing cw → `None`.)
  Fixtures reuse `workspace/project/repository/session/coding_workspace::create` plus `artifact::create` + `artifact::update(conn, id, title, content)` to set content.

### e2e (`e2e/specs/agent-sdk.e2e.ts` — already has the project/repo/account/SDK-adapter + fake-sidecar setup)
One test proving the full chain end to end:
1. Create an artifact with a checklist task and content, save it (set the markdown via `browser.execute` — `setValue` types `\n` as Enter), dispatch it with an explicit branch name into a worktree (reuse the dispatch-dialog flow from `dispatch.e2e.ts`).
2. In Agents, select that dispatched worktree + the SDK adapter + the account.
3. `browser.waitUntil` the goal textarea `.getValue()` `.includes()` **both** the task title and an artifact-content snippet (the prefill is an async `invoke` — poll, never single-read; this is the slice-3 CI-flake class).
4. Overwrite the goal via `browser.execute`, launch, and assert via the existing feed markers that the **edited** goal reached the sidecar — proving seed-not-binding.

No Vitest: the repo has no frontend unit-test toolchain; standing one up for one watcher is out of scope. The watch logic is covered by the e2e; the assembly logic lives in the Rust pure formatter.

---

## Out of scope / deferred
- Seeding PTY/CLI agents (they have no goal field — a separate, larger mechanism).
- A "Run with SDK agent" button on the worktree / cross-view navigation (deferred earlier).
- Backend size enforcement / truncation, per-task artifact sectioning, snapshot-at-dispatch.

## Review findings incorporated
Resolver in `services/` (board precedent), not the command file · simplified strict degrade ladder (no impossible "dangling artifact" branch) · standard backend error mapping + frontend swallow · documented no-scoping decision + argv note · direct api call (no store passthrough) · `adapterKind(val)` over lazy `selectedIsSdk` in the adapter watch · monotonic token + dirty-check + empty-id guard · seeded hint, reviewable rows, large-goal warning · resolver branch tests incl. the dispatched-vs-plain boundary; e2e proves seed-not-binding and polls for the async prefill.
