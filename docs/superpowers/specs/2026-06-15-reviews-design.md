# Milestone 8 — Review Records & Review View

## Goal

Make completed coding work easy to judge. Turn a worktree's diff into a
persisted, decidable **review record** so the user can inspect changes and set a
verdict without reading any chat history.

## Decisions

- **Test command — deferred to M9.** The `reviews` table carries `test_command`
  (captured from project settings) and `test_output`, but M8 never executes the
  command. M9's automation runner runs it safely. This builds the
  command-execution security surface once, in M9.
- **Snapshot model, many reviews per coding workspace.** Each review is an
  immutable snapshot of the diff/summary at creation time. Generating a review
  again after more work creates a new record, so a workspace accumulates a
  review history. Status is set per-review.
- **Risk notes are heuristic flags.** A computed list of plain-text flags from
  the diff, cheap and deterministic.

## Data model

New table `reviews` (migration `0005_reviews.sql`):

| column                | type | notes |
|-----------------------|------|-------|
| `id`                  | TEXT PK | uuid |
| `workspace_id`        | TEXT NOT NULL | denormalized from the coding workspace's project; scopes `list_reviews`. FK → `workspaces(id)` ON DELETE CASCADE |
| `coding_workspace_id` | TEXT NOT NULL | the worktree snapshotted. FK → `coding_workspaces(id)` ON DELETE CASCADE |
| `status`              | TEXT NOT NULL DEFAULT `'pending'` | one of `pending`, `approved`, `rejected`, `changes-requested`, `done` |
| `summary`             | TEXT NOT NULL | one-line size summary, e.g. `3 files changed, 42 insertions(+), 5 deletions(-)`; `No changes` when clean |
| `status_short`        | TEXT NOT NULL | `git status --short` snapshot |
| `diff_stat`           | TEXT NOT NULL | `git diff --stat HEAD` |
| `files_json`          | TEXT NOT NULL | JSON array of changed paths (tracked + untracked), default `[]` |
| `test_command`        | TEXT NULL | captured from `project.settings_json`; not run in M8 |
| `test_output`         | TEXT NOT NULL DEFAULT `''` | filled by M9 |
| `risk_notes_json`     | TEXT NOT NULL | JSON array of flag strings, default `[]` |
| `created_at`          | TEXT NOT NULL | RFC3339 |
| `updated_at`          | TEXT NOT NULL | RFC3339 |

Indexes on `workspace_id` and `coding_workspace_id`.

Cascade behavior: deleting a coding workspace removes its reviews; deleting a UAW
workspace cascades through workspace → project → coding workspace → review, and
also directly via the `reviews.workspace_id` FK.

## Review generation

Deterministic, reusing the existing hardened git layer. Every git invocation
keeps the established flags: `-c core.fsmonitor= -c core.hooksPath=/dev/null`
before `-C <worktree>`, and `--no-ext-diff --no-textconv` on diffs.

- `git::review_snapshot(worktree_path) -> ReviewSnapshot` collects:
  - `status --short` → `status_short`
  - `diff --stat HEAD` → `diff_stat`
  - `diff --numstat HEAD` → per-file `(added, deleted, path)`, where binary files
    report `-`/`-`
  - changed-file list = tracked (name-only / numstat paths) ∪ untracked (`??`
    entries from `status --short`)
  - `is_clean`, `error` (mirrors `worktree_diff`: a failed `status` returns an
    error rather than falsely reporting clean)
- `compute_risk_notes(snapshot) -> Vec<String>` — a **pure** function (no git, so
  fully unit-testable) producing flags:
  - **Large change** — total added + deleted lines > 300
  - **Many files changed** — file count > 20
  - **Migration file changed** — a path under a `migrations/` directory or a
    `.sql` file under one
  - **Lockfile changed** — basename in `Cargo.lock`, `pnpm-lock.yaml`,
    `package-lock.json`, `yarn.lock`
  - **Files deleted** — any `D` status entry
  - **Binary / non-text change** — any numstat binary (`-`) marker
- `summary` — derived from the `diff --stat` summary line, with untracked files
  reflected; `No changes` when the tree is clean. A clean tree is allowed to be
  reviewed (informational), not blocked.

## Commands (`commands/reviews.rs`)

All return `Result<_, String>` and follow the existing lock discipline: acquire
the `Mutex<Connection>` for DB reads, **release before any git call**, re-acquire
to insert. No `AppHandle` needed — the worktree path stored on the coding
workspace is already absolute.

- `create_review_for_coding_workspace(state, coding_workspace_id)`
  - loads the coding workspace; errors if missing
  - derives `workspace_id` via its project; reads optional `test_command` from
    `project.settings_json` (stored, not run)
  - runs `git::review_snapshot` on the worktree; if `error` is set, returns `Err`
    (no row inserted)
  - computes `summary` + risk notes, inserts the review, returns it
  - does **not** mutate the coding workspace's status (separation of concerns;
    M9 owns the auto-transition to Needs Review)
- `list_reviews(state, workspace_id)` — ordered `created_at` descending
- `get_review(state, id)` — `Option<Review>`
- `update_review_status(state, id, status)` — validates `status` against the
  allowed set (`Err` on unknown), updates `updated_at`, returns the review

Registered in `commands/mod.rs` and the Tauri `invoke_handler`.

## Models (`models/review.rs`)

`Review` struct mirroring the table, with:
- `create(conn, NewReview) -> Review`
- `list_by_workspace(conn, workspace_id) -> Vec<Review>`
- `get(conn, id) -> Option<Review>`
- `update_status(conn, id, status) -> Option<Review>`

Tests: CRUD round-trip; status update; cascade on coding-workspace delete; cascade
on workspace delete; `files_json` / `risk_notes_json` (de)serialize as arrays.

## Frontend

- `types/review.ts` — `Review` type (mirrors the row; `files`/`risk_notes` parsed
  to `string[]` at the API boundary, or kept as JSON strings parsed in the store —
  match the existing `coding_workspace` pattern).
- `api/reviews.ts` — `invoke` wrappers for the four commands.
- `stores/reviews.ts` — Pinia setup store: `list`, `loading`, `error`, `current`;
  `load(workspaceId)` (monotonic token like other stores), `get(id)`,
  `createForCodingWorkspace(id)`, `updateStatus(id, status)`.
- `components/ReviewsView.vue` — added to the nav beside Coding. Pending-first
  list → detail panel showing: `summary`, files changed, `diff_stat`, risk-note
  flags, test output (`"Not run yet"` when empty), a status badge, and four
  actions: **Approve** / **Reject** / **Request changes** / **Mark done**.
- `CodingView.vue` — each worktree row gains a **"Create review"** button that
  calls `reviews.createForCodingWorkspace(cw.id)`, toasts success, and the new
  review appears in the Reviews view.
- Nav wiring follows the existing view registration (the same mechanism that
  lists Inbox / Projects / Sources / Coding).

## Error handling

- `create_review_for_coding_workspace` returns `Err` when the coding workspace is
  missing or `review_snapshot` reports an error; the UI toasts it. A clean tree
  is **not** an error.
- `update_review_status` returns `Err` for an unknown status value.
- Store actions surface errors via the existing toast composable.

## Testing

- **Rust unit**: `review` model CRUD + cascade; `compute_risk_notes` table-driven
  tests over synthetic snapshots (each flag on/off); `git::review_snapshot`
  temp-repo lifecycle (commit → edit tracked → add untracked → delete tracked →
  assert status/files/numstat).
- **e2e**: extend `coding.e2e.ts` — after the worktree has an untracked file,
  click **Create review**, open the Reviews view, assert a pending review with the
  expected file and a risk flag, click **Approve**, assert the status badge
  updates.

## Out of scope (M9+)

- Executing the configured test command and capturing `test_output`.
- Auto-transitioning a session/coding workspace to Needs Review on completion
  (the mandatory completion automation is M9).
- Linking reviews to sessions (arrives with the agent-adapter milestone).
