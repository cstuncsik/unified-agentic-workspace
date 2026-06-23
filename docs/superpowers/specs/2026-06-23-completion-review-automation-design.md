# Completion → review automation — Design

**Goal:** When an edit-mode Claude Agent SDK agent finishes and has changed files, automatically create the review (instead of requiring the manual "Review changes" click) — the review appears instantly from the diff, and the project's check/test results fill in asynchronously. Closes M10's last "Done when": *"Completion triggers review automation."*

**Milestone:** M10 (Agent Adapter MVP) — final done-when criterion.

**Status:** Approved design (post 5-discipline review). Ready for an implementation plan.

---

## Background

The "review automation" already exists as `complete_coding_workspace`: it snapshots the worktree diff, runs the project's configured check/test command, and creates a `Review` (moving the worktree to `needs-review`, which surfaces it on the board). There is **no rules engine** — M9's "automation" *is* this flow. Today it is triggered **manually**:

- `SdkRunView.vue` shows a "Review changes" CTA after an edit-mode SDK run dirties the worktree.
- `CodingView.vue` has a "Complete and review" button (and a separate "Create review").

SDK completion is already detected reliably in `SdkRunView` (the slice-2 result-event keying), and tabs persist (`v-show`) while `closeTab` on a running session kills the agent — so an SDK edit completion is never observed off-screen. The gap is only the manual click.

## Decisions (fixed by the product owner; some set during the review)

1. **On completion (edit mode + changed files):** auto-create the review + notify (toast). No forced navigation — the review appears in Reviews and on the board.
2. **Checks run asynchronously:** the review is created **instantly from the diff snapshot**; the check/test command runs **after** and **updates** the review when done.
3. **SDK edit mode only** (plan mode never edits; PTY/CLI agents have no structured completion signal).
4. **No toggle/setting** (YAGNI).
5. **Renderer-driven** (consistent with today's manual completion path; no backend thread/event machinery).

### Security note (consciously revised)

The edit-mode design doc (`2026-06-20-agent-sdk-edit-mode-design.md`) stated *"'Review changes' stays a manual, user-initiated action (never auto-run on completion)"* — because the check command is the one moment agent-authored content (scripts the edit agent wrote: `package.json`, `Makefile`, test files) gets **executed**. This feature revises that: the check auto-runs **asynchronously** on completion. Accepted because this is a single-user local app, the check **command string** comes from project settings (not agent-controllable — confirmed via `project::test_command_from_settings`), and the user explicitly chose edit mode + configured the command. The spec records this revision; the edit-mode doc's note will be updated to point here.

---

## Architecture & data flow

Two-step, renderer-orchestrated:

```
SdkRunView: edit agent completes → worktree dirty (watch [completed, diff])
   │  step 1 (instant)
   ├─▶ coding.complete(cw, runCheck=false)
   │        └─ complete_coding_workspace(cw, run_check=false)
   │             snapshot diff → create Review (checks not-run) → status=needs-review
   │        → reviews.insert(review), toast, board shows needs-review
   │  step 2 (async, background)
   └─▶ coding.recheck(review.id)            // mark review "rechecking"
            └─ recheck_coding_workspace(review.id)
                 re-snapshot → run check (CHECK_TIMEOUT) → recompute → review::update_results
            → reviews.update(updatedReview)  // checks now present
```

No migration (the `reviews` columns exist from M8). No network, no key handling on this path.

---

## Backend (`src-tauri/src`)

### `commands/coding_workspaces.rs`
- **`complete_coding_workspace(state, coding_workspace_id, run_check: bool) -> Review`** — add the `run_check` param.
  - `run_check == false`: skip the check (use `check::CheckOutcome::not_run()` regardless of a configured command), snapshot the diff, create the review, move to `needs-review`, return immediately.
  - `run_check == true`: today's behavior (snapshot + run check inline + create). Only `CodingView`'s "Complete and review" passes `true` (synchronous, unchanged). The SDK auto path — **and its retry-on-error** — passes `false`, then calls `recheck_coding_workspace` for the async check, so the two stay consistent.
- **`recheck_coding_workspace(state, review_id: String) -> Review`** (new) — the async-check step:
  - Under the lock: load the review by id → its `coding_workspace_id` → cw → `worktree_path` + the project's `test_command`. Release the lock.
  - `git::review_snapshot(worktree)` + `check::run_check(worktree, cmd, CHECK_TIMEOUT)` (if a command is configured; else `not_run` → returns the review effectively unchanged).
  - Recompute `summary` / `risk_notes` (augmented with the outcome) / `test_output` via the existing `review_svc` + `completion` helpers.
  - Under the lock: `review::update_results(...)` → return the updated review.
  - A small shared helper factors the "snapshot (+ optional check) → review fields" computation so `complete` and `recheck` don't duplicate it.

### `models/review.rs`
- **`update_results(conn, review_id, summary, status_short, diff_stat, files, test_command, test_output, risk_notes) -> Review`** (new) — `UPDATE` the review's computed fields by id, return the updated row. No new columns.

### Registration
Register `recheck_coding_workspace` in `lib.rs`. No migration.

---

## Frontend (`src`)

### Stores
- `codingWorkspaces.ts`: `complete(id, runCheck)` gains the flag; add `recheck(reviewId)` → `invoke("recheck_coding_workspace", { reviewId })`.
- `reviews.ts`: add `update(review)` (replace by id; the store already dedupes by id) and a non-persisted `rechecking` set/flag keyed by review id.

### `components/SdkRunView.vue`
- Replace the separate completion watch with **`watch([completed, diff], …, { immediate: true })`**: when `completed && isEdit`, if `diff` is missing → `coding.refreshDiff` (this also **re-fetches after a workspace switch wiped `diffs`** — fixes the review's Frontend B1 strand bug); when `diff` is loaded, dirty, no error, and not yet reviewed/in-flight → run the auto-review. `showReview` stays purely presentational.
- **Auto-review** (reusing/renaming `reviewChanges`): `complete(cw, runCheck=false)` → `reviews.insert`, toast, `reviewed = true`; then `recheck(review.id)` in the background (mark `rechecking`) → `reviews.update` on resolve. Guarded by `completing` (re-entry) + `reviewed` (one per session).
- **Footer states:** while creating → "Creating review…"; created + checks running → "✓ Review created — running checks…"; created + checks done → "✓ Review created — see Reviews"; diff-read error → existing diff-error line; **creation error** → the "Review changes" button as a manual retry (so a transient failure isn't a dead end).

### `components/CodingView.vue`
- Gate the "Complete and review" button on `status !== 'needs-review'` (matching the existing "Mark ready" guard) so it can't mint a **duplicate** review after auto-review flips the worktree to `needs-review` (the shared store status update hides it reactively). The button stays synchronous (`runCheck=true`). *(Frontend gating — not a backend idempotency guard, which would wrongly block a legitimate review of a second edit iteration.)*

### `components/ReviewsView.vue`
- Show a small "running checks…" indicator on a review while it is `rechecking`.

---

## Testing

### Rust unit (`commands/coding_workspaces.rs` / `models/review.rs`)
- `complete_coding_workspace(run_check = false)` creates a review whose checks are `not_run` (even when a command is configured), worktree → `needs-review`.
- `recheck_coding_workspace` updates an existing review's results: with a trivial passing check command (e.g. `true`) → checks ran + passed; with a failing one (e.g. `false`) → ran + not passed; the review row reflects the new `test_output`/`risk_notes`.
- `review::update_results` round-trips the updated fields.

### e2e (`e2e/specs/agent-sdk.e2e.ts`)
- **Edit completion auto-creates the review with no click:** record the `review-row` count before launching, run the edit agent to completion, `waitUntil` the "Review created" footer (`sdk-review-done`) appears on its own, then assert the Reviews list grew by **exactly one** and that row's detail lists `AGENT_EDIT.md`. (`SdkProj` has no check command, so the review is instant + final; the recheck path is a no-op here and is covered by the Rust test.) Removes the old toast-overlay JS-click.
- **Plan completion creates no review:** after the plan run reaches a positive "Done" signal (gating against an early-check race), assert `sdk-review-cta`/`sdk-review-done` are absent **and** the Reviews count is unchanged.

No Vitest (the repo has none); the watch logic is covered by e2e, the async-check logic by the Rust `recheck` test.

---

## Out of scope / deferred
- PTY/CLI and plan-mode auto-review.
- A per-project / global "auto-review on completion" or "auto-run checks" setting.
- A backend completion hook (renderer-driven is sufficient — closing a tab kills the agent, tabs persist, so completion is never off-screen).
- Diff-identical dedupe / superseding prior reviews; a data-layer idempotency guard.

## Review findings incorporated
Async checks instead of blocking the review on a 600 s test run (Security + Product) · documented revision of the "never auto-run on completion" control · duplicate prevention via `CodingView` button gating, **not** a backend guard that would block second-iteration reviews (Rust) · fold the trigger into `watch([completed, diff])` + re-fetch the diff when missing to fix the workspace-switch strand bug, keep `showReview` presentational (Frontend) · count-based plan-mode negative gated on "Done", exactly-one-review idempotency assertion + `AGENT_EDIT.md` content check, Rust `recheck` test (Testing).
