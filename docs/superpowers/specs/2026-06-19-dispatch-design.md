# Milestone 11 — Dispatch From Artifact To Coding Tasks

## Goal

Connect research mode to execution mode: turn a markdown artifact into one or more
coding sessions, each with a git worktree, and link those sessions back to the
artifact that spawned them. Done when a user can turn a markdown spec into coding
sessions, and sessions link back to the artifact. Sets up M12 (parallel board).

## Decisions

- **Per task, Dispatch creates a session + a worktree.** Each selected task →
  a `sessions` row (mode `code`, status starts `todo`, `created_from_artifact_id`
  set) and a git worktree (coding workspace) from the chosen project/repo/base,
  branch from the task title, with `coding_workspaces.session_id` linking them.
- **Extraction is deterministic** (no LLM until M10b): markdown task-list items
  (`- [ ]` / `- [x]`) if present, else `##`/`###` headings. The user edits the
  seeded list before dispatching.
- **Shared project/repo/base** for the whole dispatch (per-task title + branch +
  include only). Per-task repo/base override is explicitly **deferred** — a
  recorded divergence from the roadmap's "per task" bullet, kept forward-compatible.

This spec folds in a multi-discipline design review (1 non-blocking critical, 13
important, 15 minor); the hardening below comes from it.

## Data model

Migration `0009_session_artifact_link.sql`:

```sql
ALTER TABLE sessions ADD COLUMN created_from_artifact_id TEXT
    REFERENCES artifacts(id) ON DELETE SET NULL;
CREATE INDEX idx_sessions_created_from_artifact ON sessions(created_from_artifact_id);
```

`ADD COLUMN` with a `REFERENCES … ON DELETE SET NULL` and an implicit NULL default
is the supported SQLite form; `init_db` enables `PRAGMA foreign_keys = ON` before
migrations, so the SET NULL fires (verified by an explicit test). No schema change
to `coding_workspaces` — `session_id` already exists as a nullable
`REFERENCES sessions(id) ON DELETE SET NULL` from migration `0004`.

## Models

- `models/session.rs`: add `created_from_artifact_id: Option<String>` to `Session`
  + `from_row`. `create` gains it as the **last** positional param
  (`created_from_artifact_id: Option<&str>`). Add
  `list_by_artifact(conn, workspace_id, artifact_id) -> Vec<Session>` (scoped to
  workspace + the artifact, ordered `created_at DESC`). Call sites to update:
  `commands/sessions.rs::create_session` (pass `None`) + 7 in-module test calls
  (pass `None`). Tests: link round-trip; SET NULL on artifact delete; `list_by_artifact`.
- `models/coding_workspace.rs`: `create` gains `session_id: Option<&str>` as the
  **last** positional param; the INSERT binds it (replacing the literal `NULL`).
  Call sites: the `create_worktree_inner` refactor + 2 in-module test calls (pass
  `None`). Test: create with a `session_id` round-trips.

## Worktree refactor

Extract the M7 worktree-creation core into
`create_worktree_inner(app: &AppHandle, state: &State<'_, Mutex<Connection>>,
project_id, repository_source_id, base_branch, branch_name, session_id: Option<&str>)
-> Result<CodingWorkspace, String>` in `commands/coding_workspaces.rs`. It MUST
preserve M7's invariants verbatim:
1. **Validate** `base_branch` + `branch_name` (trim, non-empty, reject leading `-`
   — option-injection guard) — moved **into** this chokepoint so no worktree can
   be created unvalidated.
2. Resolve project + repo (and the shared workspace) under the lock, then **drop
   the lock before any git**.
3. `worktrees_base` + `git::create_worktree`.
4. Re-acquire the lock, insert the `coding_workspaces` row (with `session_id`); on
   insert failure, **remove the on-disk worktree** (`git::remove_worktree`) to
   avoid an orphan.

`create_coding_workspace` (M7 command) becomes a thin wrapper calling it with
`session_id: None`. The dispatch loop calls it per task with the new session id.
There is intentionally **no** single transaction spanning the whole dispatch; each
task's lock hold is short and git never runs under the lock.

## Dispatch service + commands

Pure (`services/dispatch.rs`, unit-tested, char-safe parsing — never byte-index):
- `extract_tasks(markdown: &str) -> Vec<String>` — task-list items if any, else
  `##`/`###` headings; trimmed, blanks dropped.
- `slugify_branch(title: &str) -> String` — git-ref-safe: lowercase, map any run
  of non-`[a-z0-9]` to a single `-`, strip leading/trailing `-`; `""` when nothing
  remains.

Commands (`commands/dispatch.rs`):
- `extract_artifact_tasks(state, artifact_id) -> Vec<String>` — load the artifact,
  return `extract_tasks(content)`.
- `validate_dispatch(conn, workspace_id, project_id, repository_source_id,
  base_branch, tasks: &[DispatchTask]) -> Result<(), String>` — **conn-testable**
  (no Tauri State): workspace exists; project belongs to workspace; repo belongs to
  workspace; `base_branch` non-empty; each **included** task's `branch_name`
  non-empty + no leading `-`; **no duplicate branch names** among included tasks
  (rejected up front, before any side effects). Unit-tested.
- `dispatch_artifact(app, state, artifact_id, project_id, repository_source_id,
  base_branch, tasks: Vec<DispatchTask>) -> DispatchResult`, where
  `DispatchTask { title, branch_name, include }`:
  1. Resolve the artifact (→ its `workspace_id`); error if missing.
  2. `validate_dispatch(...)` — structural errors (bad project/repo/base/dup
     branch) abort before any side effects.
  3. For each included task, **resilient + best-effort**:
     - create a `session` (mode `code`, status `todo`, title,
       `created_from_artifact_id = artifact_id`) — always.
     - `create_worktree_inner(... session_id = Some(session.id))`. On success, set
       the session status to `worktree-created`. On git failure (e.g. the branch
       already exists in the repo), keep the session at `todo` and record the
       error.
  - Returns `{ results: [{ title, session_id, coding_workspace_id?, error? }] }`.
    An empty/all-deselected task set returns `{ results: [] }` with no error.

Even if every worktree fails, the sessions-linked-to-artifact done-criterion holds.

## Frontend

- `composables/useRepositoryBranches.ts` — extract CodingView's inline branch
  loading into `{ branches, baseBranch, loading, selectRepo(repoId) }` (owns the
  monotonic token + `listRepositoryBranches` + default-base logic). **Refactor
  CodingView to use it** (no behavior change; the coding e2e proves it).
- `utils/slug.ts` — `slugifyBranch(title): string` mirroring the backend contract,
  for seeding editable branch names.
- `api/dispatch.ts` — `extractArtifactTasks`, `dispatchArtifact`, plus
  `listArtifactSessions(artifactId)` (the back-link).
- `components/DispatchDialog.vue` — a dedicated `re-dialog` (own ref,
  `showModal`/`close`, `@close` = cancel; **not** the `useConfirm` singleton),
  `data-testid="dispatch-dialog"`. On open: `extractArtifactTasks` seeds editable
  rows (`data-testid="dispatch-task-row"`: a `re-checkbox` include + a title input
  + a slug branch input). Shared **project** (code/mixed), **repo**, and **base**
  (`useRepositoryBranches`) pickers. Prerequisites: if no code/mixed project or no
  repo, show the same hint as CodingView and disable Dispatch; Dispatch is also
  disabled when nothing is included. On dispatch → render the per-task result list
  **inline** in the dialog (✓ created / error), not just a toast.
- `components/ArtifactsView.vue` — a **Dispatch** button (`data-testid="dispatch-button"`)
  on the selected artifact; mounts `DispatchDialog`; and a small **Dispatched
  sessions** list for the selected artifact (`listArtifactSessions` → title +
  status), the visible, actionable back-link. After a dispatch, refresh the coding
  workspaces store (so Coding shows the new worktrees) and this list.

## Error handling

- Structural validation errors (missing project/repo, cross-workspace, empty base,
  bad/duplicate branch) abort the dispatch with a single error (no partial side
  effects).
- Per-task git failures are reported in the result list; the task's session
  survives. A worktree row-insert failure removes the on-disk worktree (M7
  invariant) and is reported.
- No code/mixed project or repo → Dispatch disabled with a hint.

## Security

- Branch names are derived from **untrusted** artifact content. `slugify_branch`
  yields only git-ref-safe characters; `create_worktree_inner` re-validates
  (non-empty, no leading `-`) for **every** task regardless of source — the guard
  lives at the chokepoint, not per caller. `base_branch` is validated too. git is
  invoked argv-only (never a shell), via the already-hardened `run_git`.
- `extract_tasks` parses untrusted markdown with char-safe ops (no byte slicing →
  no UTF-8 panic) and returns plain strings (no path/command surface).
- All SQL parameterized; `created_from_artifact_id`/`session_id` are bound params.

## Testing

- **Rust unit:** `extract_tasks` (checkbox doc, heading doc, mixed, empty,
  adversarial/non-ASCII); `slugify_branch` (spaces/punct/unicode → safe, empty
  fallback); `validate_dispatch` (happy, missing workspace/project/repo,
  cross-workspace project/repo, empty base, empty/`-`-leading branch, duplicate
  branches); session link round-trip + SET NULL on artifact delete +
  `list_by_artifact`; `coding_workspace::create` with `session_id`;
  `create_worktree_inner` orphan-cleanup on insert failure (temp repo, forced
  duplicate id → assert no worktree dir remains).
- **e2e** (`e2e/specs/dispatch.e2e.ts`, **its own** `fixture-repo-dispatch` so
  branch names never collide with other specs; `textOf`/`waitUntil` + scoped
  selectors): create an artifact with `- [ ]` tasks → **Dispatch** → pick the
  fixture project/repo/base → dispatch → assert the inline result list shows the
  created worktrees → close → the artifact shows "Dispatched: N" → the Coding view
  lists the new `dispatch/*` worktrees.

## Out of scope (later)

- Per-task repo/base override, LLM-based extraction (M10b), re-dispatch/dedupe of
  already-dispatched tasks, auto-starting an agent on each new worktree, and
  converging the manual M7 "create worktree" path (session-less) with dispatched
  ones.
