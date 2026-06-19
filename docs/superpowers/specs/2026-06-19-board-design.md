# Milestone 12 — Parallel Agent Board

## Goal

Make simultaneous work visible: a board, grouped by stage, of every coding
workspace with its health, review state, changed-file count, agent status, and
last activity — plus a side-by-side compare of multiple reviews. Done when a user
can monitor more than one coding task without losing track. Closes the
research → dispatch → code → review → **monitor** loop.

## Decisions

- **Columns**: a derived 3-stage grouping — **In progress / Needs review /
  Reviewed**.
- **Health**: live git per worktree on board load, via a **lightweight**
  status-only path.
- **Compare**: multiple **reviews** side-by-side from the M8 stored snapshots.

This spec folds in a multi-discipline design review (0 critical, 11 important).

## Backend

### Lightweight worktree health — `services/git.rs`

`worktree_diff` spawns 3 git subprocesses (status + 2 diffs); the board only needs
clean/dirty + a count. Add:

```rust
pub struct WorktreeHealth { pub is_clean: bool, pub changed_files: usize, pub error: Option<String> }
pub fn worktree_health(worktree: &Path) -> WorktreeHealth
```

It runs only `git status --porcelain` (through the hardened `run_git`); on a status
error it returns `error: Some(..)` (health "unknown"), never panics — mirroring
`worktree_diff`'s degrade-don't-fail contract.

### Review query — `models/review.rs`

```rust
pub fn latest_for_coding_workspace(conn, coding_workspace_id) -> Option<Review>
// SELECT {COLUMNS} FROM reviews WHERE coding_workspace_id = ?1
// ORDER BY created_at DESC, rowid DESC LIMIT 1
```

"Latest" is by `created_at` (snapshot time), `rowid` as a deterministic tiebreak
(so a re-opened older review can't mask a newer one). The index
`idx_reviews_coding_workspace` already exists (migration 0005) — no new migration.

### Board service — `services/board.rs` (pure + conn-testable)

```rust
pub fn board_stage(cw_status: &str, latest_review_status: Option<&str>) -> &'static str
```
Review verdict dominates the coding-workspace status:
- latest review `Some("pending")` → `"needs-review"`
- latest review `Some("changes-requested")` → `"in-progress"` (work bounced back, resumes)
- latest review `Some("approved" | "done" | "rejected")` → `"reviewed"`
- latest review `None` → `"needs-review"` if `cw_status == "needs-review"`, else `"in-progress"`
- any unknown status → `"in-progress"` (safe default)

```rust
pub struct BoardCardBase { /* everything except live git health */
    coding_workspace_id, branch_name, base_branch, project_name, repo_name,
    status, latest_review_status: Option<String>, agent_status: Option<String>,
    last_activity: String, stage: String,
}
pub fn assemble_cards(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<(BoardCardBase, String)>>
// returns each base card paired with its worktree_path (kept backend-only, used
// for live git then dropped — never sent to the frontend).
```
`assemble_cards` lists the workspace's coding workspaces and, **memoizing**
`project::get`/`repository::get` in a `HashMap`, attaches per card: project/repo
names, `latest_for_coding_workspace(...).map(|r| r.status)`, the latest agent
session status (`agent_session::list_by_coding_workspace(...).first()`),
`last_activity = max(coding_workspace.updated_at, latest review.updated_at, latest
agent_session.updated_at)`, and `stage = board_stage(status, latest_review_status)`.
Conn-only → unit-testable without Tauri/git.

### Command — `commands/board.rs`

`get_board(state, workspace_id) -> Vec<BoardCard>`:
1. Under the lock, `assemble_cards(&conn, &workspace_id)`; **release the lock**.
2. For each `(base, worktree_path)`, run `git::worktree_health(worktree_path)` (no
   lock held) and merge `is_clean` / `changed_files` / a `health: "clean" | "dirty"
   | "unknown"` (unknown when `error.is_some()`) into the `BoardCard` returned to
   the frontend. `worktree_path` is **not** included in `BoardCard`.

`BoardCard = BoardCardBase + { is_clean, changed_files, health }`.

## Frontend

- `types/board.ts` (`BoardCard`), `api/board.ts` (`getBoard`), `stores/board.ts`
  (`load(workspaceId)` clearing `list = []` before the await with a monotonic
  `loadToken`, `loading`, `error`).
- `utils/reviewTone.ts` — extract ReviewsView's `badgeVariant` map
  (`approved|done→success, rejected→danger, changes-requested→warning, else info`)
  to a shared `reviewTone(status)`; ReviewsView imports it (so Board + Reviews
  can't drift).
- `components/BoardView.vue`:
  - Loads on mount **and on `workspaces.currentId` change** (it is deliberately
    NOT in App.vue's watch, to avoid git when not viewing it); a **Refresh** button
    re-pulls.
  - States: loading (`Loading board…`), error (`.error`), empty (no cards →
    "No coding work yet — dispatch from an artifact or create a worktree").
  - Three columns (`data-testid="board-column"` + `data-stage="in-progress|needs-review|reviewed"`),
    each labeled and rendered even when empty. Cards (`data-testid="board-card"`):
    branch · project/repo; a status/stage badge; **health** (clean / N changed /
    unknown); a **review** badge via `reviewTone`; an **agent** badge
    (running→info, exited→neutral, failed→danger, none hidden); last-activity time.
  - A **Compare reviews** button (`data-testid="board-compare"`) opens the compare
    dialog.
- `components/ReviewCompareDialog.vue` — a `re-dialog`
  (`data-testid="compare-dialog"`) listing the workspace's reviews (via the M8
  reviews store / `list_reviews`), each labeled by **its coding workspace** (branch
  · project/repo, resolved from the coding store) since reviews are workspace-keyed
  and a worktree can have several. Multi-select (`re-checkbox`); selecting 2+
  renders their stored snapshots (summary / status / diff_stat / changed files /
  risk notes) in side-by-side columns. All diff/stat/summary text rendered via
  `<pre>` / `{{ }}` **only — never `v-html`** (the artifact preview remains the sole
  `v-html` sink).
- App.vue: add `"board"` to `ActiveView`, a **Board** sidebar button (after
  Reviews) with `:aria-current`, and a `<BoardView v-else-if="activeView === 'board'" />`
  branch. (No board load in the workspace watch.)

## Error handling

- A worktree whose `git status` fails → that card's `health = "unknown"`
  (`is_clean`/`changed_files` not treated as clean); the rest of the board still
  loads. Store load errors surface via `board.error` (existing pattern).

## Security

- The board adds no new injection surface: `worktree_health` goes through the
  hardened `run_git` (argv, `core.fsmonitor=`/`core.hooksPath=/dev/null`); no
  repo-derived string is interpolated. All SQL parameterized. `worktree_path` is
  backend-only. Compare/board render only plain text (`<pre>`/`{{ }}`), so review
  diff/stat content never reaches an HTML sink.

## Testing

- **Rust:** `board_stage` table tests over the full `(cw.status × {None, pending,
  approved, rejected, changes-requested, done})` matrix (incl. verdict-dominates,
  changes-requested→in-progress, unknown→in-progress); `assemble_cards`
  (empty workspace → `[]`; a worktree with no review → in-progress; agent-status
  mapping none/running/exited; project/repo memoization correctness);
  `review::latest_for_coding_workspace` ordering (two reviews, newest wins; tiebreak).
- **e2e** (`e2e/specs/board.e2e.ts`, dedicated `fixture-repo-board` + board-unique
  `board/*` branches; `textOf`/`waitUntil` + scoped selectors): create a code
  project + repo + two worktrees, create a review on one → open **Board** → assert
  the un-reviewed worktree is in the *In progress* column and the reviewed one in
  *Needs review*; **decide that review** (e.g. Request changes in Reviews) →
  Refresh → assert the card moved to *In progress* (stage transition). Then
  **Compare** two reviews and assert both summaries render side-by-side.

## Out of scope (later)

- Per-card click-through to ReviewsView/AgentsView (needs cross-view navigation),
  auto-refresh / live updates, drag-between-columns, comparing two worktrees' live
  diffs, and a true per-card event-log feed (last-activity uses the max of the
  three timestamps).
