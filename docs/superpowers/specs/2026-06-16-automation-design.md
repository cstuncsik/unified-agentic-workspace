# Milestone 9 — First Automation

## Goal

Implement the mandatory coding-completion automation: when the user completes a
coding workspace, UAW collects the diff, runs the configured check command,
creates a review with the captured output and risk flags, records an event, and
moves the workspace to Needs Review — all in one action.

## Decisions

- **Check execution — shell + timeout.** Run the project-configured command via
  `sh -c "<command>"` with `cwd = worktree`, a 600s timeout that kills the
  process, and combined stdout+stderr captured. Test commands are inherently
  shell-composed (`pnpm test && cargo test`). The command is **user-authored
  project configuration** (the trust boundary); no repo-derived string (path,
  branch, diff) is ever interpolated into it. UAW still never auto-runs the
  repo's own scripts.
- **Keep both buttons.** M8's *Create review* stays an interim snapshot (no
  checks, no status change). M9 adds *Complete and review* — the completion
  automation.
- **Per-project config field.** A project's check command is set from a *Test
  command* field in the Projects view, persisted into `project.settings_json`.
- **Check failure completes anyway.** A non-zero exit, timeout, or spawn error
  still creates the review, stores the output, adds a risk flag, and moves to
  Needs Review. Surfacing the failure to a human reviewer is the point. Only a
  `review_snapshot` failure or a DB error aborts.

## Architecture

A new command `complete_coding_workspace` orchestrates the flow. It composes
existing M8 pieces (`git::review_snapshot`, `services::review::{summarize,
compute_risk_notes}`, `review::create`) with two new services (`check`,
`completion`) and a new `events` record. Pure logic lives in services
(unit-testable); the command does the lock dance + IO orchestration.

### Data flow

```
Complete and review (UI)
  → complete_coding_workspace(coding_workspace_id)
     1. [lock] load coding workspace (workspace_id, worktree_path, project_id)
              + project.settings_json → test_command ; [release lock]
     2. snapshot = git::review_snapshot(worktree)          (Err → abort)
     3. outcome  = test_command ? check::run_check(worktree, cmd, 600s)
                                : CheckOutcome::not_run()
     4. test_output = completion::format_test_output(cmd, outcome)
        risk_notes  = completion::augment_risk_notes(
                        review::compute_risk_notes(snapshot), outcome)
     5. [lock] review = review::create(..., test_command, test_output, risk_notes)
              coding_workspace::update_status(id, "needs-review")
              event::create(workspace_id, "coding_workspace.completed", payload)
       [release lock]
     6. → return review
  → UI: row status flips to needs-review, review inserted into reviews store,
        toast points to Reviews
```

## Components

### 1. Check runner — `services/check.rs`

```rust
pub struct CheckOutcome {
    pub ran: bool,            // false when no command was configured
    pub exit_code: Option<i32>, // None on timeout or spawn failure
    pub timed_out: bool,
    pub output: String,       // combined stdout+stderr (or spawn error text)
}
```

- `run_check(worktree: &Path, command: &str, timeout: Duration) -> CheckOutcome`
  spawns `Command::new("sh").arg("-c").arg(command).current_dir(worktree)`, with
  stdout and stderr both redirected to one temp file (`File` + `try_clone`) to
  avoid pipe-buffer deadlock. Polls `try_wait()` on a short sleep loop; on
  timeout `kill()`s the child and sets `timed_out`. Reads the temp file into
  `output`, then removes it. A spawn failure returns `ran: true, exit_code: None,
  output: <error>` (treated as a failed check, per the decision).
- `CheckOutcome::not_run()` helper for the no-command case.
- `passed()` convenience: `ran && !timed_out && exit_code == Some(0)`.
- Tests (real `sh`, temp dir): `echo` → passed + output captured; `exit 3` →
  `exit_code == Some(3)`, not passed; `sleep 5` with 1s timeout → `timed_out`,
  killed.

### 2. Completion helpers — `services/completion.rs` (pure)

- `format_test_output(command: &str, outcome: &CheckOutcome) -> String` — empty
  string when `!outcome.ran`; otherwise a header (`$ <command>`) + the output +
  a trailer (`[exit N]`, `[timed out]`, or `[no exit code]`).
- `augment_risk_notes(notes: Vec<String>, outcome: &CheckOutcome) -> Vec<String>`
  — appends `Checks timed out` when timed out, else `Checks failed` when ran and
  not passed; unchanged when not run or passed.
- Table tests for each branch.

### 3. Events — migration `0006_events.sql` + `models/event.rs`

```sql
CREATE TABLE events (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    type         TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_events_workspace ON events(workspace_id);
```

`Event` model with `create(conn, id, workspace_id, type, payload_json)` and
`list_by_workspace` (used in tests / future event surfacing). Cascade-tested.
No UI this milestone.

### 4. Review insert gains `test_output`

`review::create` adds a `test_output: &str` parameter (currently always inserts
`''`). M8's `create_review_for_coding_workspace` passes `""`; completion passes
the captured output. The model test helper updates accordingly.

### 5. Project test-command config

- `project::update_settings_json(conn, id, settings_json: &str) -> Option<Project>`.
- Command `set_project_test_command(state, id, test_command: Option<String>)`:
  parse the project's existing `settings_json`, set the `test_command` key (or
  remove it when `None`/blank, preserving other keys), serialize, persist. A pure
  helper `merge_test_command(settings_json, Option<&str>) -> String` holds the
  JSON logic and is unit-tested (set, remove, preserve-other-keys, malformed-input
  → fresh object). Returns the updated `Project`.

### 6. Completion command — `commands/coding_workspaces.rs`

`complete_coding_workspace(state, coding_workspace_id) -> Result<Review, String>`
implements the data flow above with the established lock discipline (read under
lock → release before git/shell → re-acquire for the review/status/event writes).
The `checks_passed` payload field uses `outcome.passed()`; `checks_ran` uses
`outcome.ran`. Registered in `commands/mod.rs` (already pub) and `lib.rs`.

### 7. Frontend

- `api/codingWorkspaces.ts`: `completeCodingWorkspace(id) -> Review`.
- `stores/codingWorkspaces.ts`: `complete(id)` — calls the command, then sets the
  matching workspace's `status` in the local list to `needs-review` (completion
  deterministically moves it there), and returns the created `Review`.
- `api/projects.ts` + `stores/projects.ts`: `setTestCommand(id, cmd)`.
- `stores/reviews.ts`: `insert(review)` — unshift if absent, replace if present
  (so a freshly-completed review shows in Reviews without a reload).
- `components/CodingView.vue`: a *Complete and review* button per row. A
  `completingId` ref drives the in-flight state (label `Running checks…`,
  disabled) — this is the "automation progress". On success: insert the returned
  review into the reviews store, toast `Completed — review ready in Reviews`, and
  the row badge reflects `needs-review`.
- `components/ProjectsView.vue`: a per-project *Test command* input, initialized
  from `JSON.parse(project.settings_json).test_command ?? ""`, saved via
  `projects.setTestCommand` on submit/blur.

## Error handling

- `review_snapshot` error → `Err`; nothing persisted (no review, no status change,
  no event).
- Check spawn failure / non-zero exit / timeout → **not** an error; the review is
  created with the output and a risk flag, status moves, event records
  `checks_passed: false`.
- DB errors at any write → `Err` (the command surfaces the string; partial writes
  are avoided by doing the three writes under one lock acquisition).
- Frontend surfaces `Err` via the toast composable; the row leaves its
  `Running checks…` state in a `finally`.

## Security

- The only string passed to `sh -c` is the user-configured `test_command`. No
  path, branch name, diff, or other repo/argument-derived value is interpolated
  into the command. `cwd` is the worktree; env is inherited; the 600s timeout
  bounds runtime.
- The command is user-authored project configuration — the same trust model as
  any local dev tool running a project script. UAW does not discover or run the
  repository's own scripts (consistent with M6's "do not run setup commands
  automatically").

## Testing

- **Rust unit:** `check::run_check` (real `sh`, temp dir: pass / non-zero /
  timeout / output capture); `completion::{format_test_output, augment_risk_notes}`
  table tests; `event` model CRUD + cascade; `merge_test_command` helper (set /
  remove / preserve / malformed); `review::create` updated for the new param.
- **e2e:** extend `coding.e2e.ts` — set a project *Test command* of
  `echo myCheck; exit 1`, create a worktree with a change, click *Complete and
  review*; assert the coding row becomes `needs-review`, and in Reviews a review
  shows `myCheck` in its test output **and** a `Checks failed` risk flag (one
  path exercising output capture + failure flag + complete-anyway).

## Out of scope (M10+)

- Replacing manual worktree changes with a real agent (Claude Code CLI / API
  adapter).
- Linking a session row to a coding workspace / review (session↔worktree linkage).
- Streaming live check output to the UI (progress is a simple in-flight state);
  surfacing the events log in the UI.
