# Completion → review automation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When an edit-mode Claude Agent SDK agent finishes with changed files, auto-create the review instantly from the diff snapshot, then run the project's check asynchronously and update the review — closing M10's "Completion triggers review automation".

**Architecture:** Two-step, renderer-driven. `complete_coding_workspace` gains a `run_check` flag (auto path passes `false` → instant, check-less review); a new `recheck_coding_workspace` re-runs the check and updates the review in place. `SdkRunView` auto-fires both on completion; `CodingView`'s manual button is gated to prevent duplicates. A shared `compute_review_fields` helper backs both commands.

**Tech Stack:** Rust (rusqlite, Tauri commands), Vue 3 `<script setup>` + Pinia, WebdriverIO e2e.

**Spec:** `docs/superpowers/specs/2026-06-23-completion-review-automation-design.md`

**Conventions:** Branch `cstuncsik/completion-review-automation`. Every commit ends with the trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` (omitted from the brief commit commands below — always append it). Rust from `src-tauri/` (`cargo test`, `cargo build`); frontend from repo root (`pnpm typecheck`).

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src-tauri/src/models/review.rs` | `update_results` — update a review's computed fields in place | Modify |
| `src-tauri/src/commands/coding_workspaces.rs` | `compute_review_fields` helper; `run_check` flag on `complete_coding_workspace`; new `recheck_coding_workspace` | Modify |
| `src-tauri/src/lib.rs` | Register `recheck_coding_workspace` | Modify |
| `src/api/codingWorkspaces.ts` | `completeCodingWorkspace(id, runCheck)`, `recheckCodingWorkspace(reviewId)` | Modify |
| `src/stores/codingWorkspaces.ts` | `complete(id, runCheck)`, `recheck(reviewId)` | Modify |
| `src/stores/reviews.ts` | `rechecking` map + `setRechecking` | Modify |
| `src/components/SdkRunView.vue` | Auto-trigger (folded watch), async recheck, footer states | Modify |
| `src/components/CodingView.vue` | Gate "Complete and review" on status | Modify |
| `src/components/ReviewsView.vue` | "running checks…" row indicator | Modify |
| `e2e/specs/agent-sdk.e2e.ts` | Auto-create on completion (no click) + plan-mode no-review | Modify |

---

## Task 1: `review::update_results` (model)

**Files:**
- Modify/Test: `src-tauri/src/models/review.rs`

**Context:** `recheck` needs to update an existing review's computed fields (everything except identity/status) in place. The file already has `update_status`, the `migrated_conn`/`fixtures`/`make` test harness, and `get`.

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/models/review.rs`, inside `mod tests` (after `update_status_changes_verdict`), add:

```rust
    #[test]
    fn update_results_replaces_computed_fields_keeping_status() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let review = make(&conn, &ws, &cw); // status "pending", test_output "", risk ["Large change"]

        let updated = update_results(
            &conn,
            &review.id,
            "2 files changed",
            " M a\n M b",
            " a | 1 +",
            &["a".to_string(), "b".to_string()],
            Some("pnpm test"),
            "PASS (2 tests)",
            &["Checks passed".to_string()],
        )
        .unwrap()
        .expect("updated review");

        assert_eq!(updated.id, review.id);
        assert_eq!(updated.status, "pending"); // status is NOT touched
        assert_eq!(updated.summary, "2 files changed");
        assert_eq!(updated.files, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(updated.test_output, "PASS (2 tests)");
        assert_eq!(updated.risk_notes, vec!["Checks passed".to_string()]);

        assert!(update_results(&conn, "missing", "", "", "", &[], None, "", &[])
            .unwrap()
            .is_none());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib models::review::tests::update_results_replaces_computed_fields_keeping_status`
Expected: FAIL to compile — `cannot find function update_results`.

- [ ] **Step 3: Implement `update_results`**

In `src-tauri/src/models/review.rs`, after the `update_status` function, add:

```rust
/// Update a review's computed fields (everything except identity + verdict status)
/// in place. Used by `recheck` to fill in check results after an instant,
/// check-less creation. `files`/`risk_notes` re-serialize to their JSON columns.
#[allow(clippy::too_many_arguments)]
pub fn update_results(
    conn: &Connection,
    id: &str,
    summary: &str,
    status_short: &str,
    diff_stat: &str,
    files: &[String],
    test_command: Option<&str>,
    test_output: &str,
    risk_notes: &[String],
) -> rusqlite::Result<Option<Review>> {
    let now = now_rfc3339();
    let files_json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());
    let risk_json = serde_json::to_string(risk_notes).unwrap_or_else(|_| "[]".to_string());
    let affected = conn.execute(
        "UPDATE reviews SET summary = ?2, status_short = ?3, diff_stat = ?4, files_json = ?5,
           test_command = ?6, test_output = ?7, risk_notes_json = ?8, updated_at = ?9
         WHERE id = ?1",
        params![
            id, summary, status_short, diff_stat, files_json, test_command, test_output,
            risk_json, now
        ],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib models::review`
Expected: PASS (the new test + the existing review tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models/review.rs
git commit -m "feat(completion-review): review::update_results (update computed fields in place)"
```

---

## Task 2: `compute_review_fields` + `run_check` flag + `recheck_coding_workspace`

**Files:**
- Modify/Test: `src-tauri/src/commands/coding_workspaces.rs`
- Modify: `src-tauri/src/lib.rs`

**Context:** Extract the snapshot-(+ optional check)-→-fields computation into a `State`-free helper (the testable seam), reuse it from `complete_coding_workspace` (now with a `run_check` flag) and a new `recheck_coding_workspace`. The file already imports `git`, `check`, `completion`, `review as review_svc`, `review` (with `Review`), `project`, `event`, `new_id`, `Path`, `Duration`, and defines `const CHECK_TIMEOUT: Duration = Duration::from_secs(600);`. The test module has `temp_git_repo()`.

- [ ] **Step 1: Write the failing tests for the helper**

In `src-tauri/src/commands/coding_workspaces.rs`, inside `mod tests` (after `worktree_cleanup_removes_dir`), add:

```rust
    #[test]
    fn compute_review_fields_runs_check_when_enabled() {
        let repo = temp_git_repo();
        std::fs::write(repo.join("new.txt"), "x\n").unwrap(); // dirty the worktree
        let pass = compute_review_fields(&repo, Some("true"), true).unwrap();
        assert!(pass.checks_ran);
        assert!(pass.checks_passed);
        let fail = compute_review_fields(&repo, Some("false"), true).unwrap();
        assert!(fail.checks_ran);
        assert!(!fail.checks_passed);
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn compute_review_fields_skips_check_when_disabled_or_no_command() {
        let repo = temp_git_repo();
        // run_check=false skips even a configured command.
        let disabled = compute_review_fields(&repo, Some("true"), false).unwrap();
        assert!(!disabled.checks_ran);
        // No command configured → not run.
        let none = compute_review_fields(&repo, None, true).unwrap();
        assert!(!none.checks_ran);
        std::fs::remove_dir_all(&repo).ok();
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib commands::coding_workspaces::tests::compute_review_fields_runs_check_when_enabled`
Expected: FAIL to compile — `cannot find function compute_review_fields` / `ReviewFields`.

- [ ] **Step 3: Add the helper**

In `src-tauri/src/commands/coding_workspaces.rs`, immediately above the `complete_coding_workspace` doc comment (which begins `/// ... the workspace to Needs Review`), add:

```rust
/// The computed fields of a review: the diff snapshot plus (optionally) the
/// project's check outcome. No DB, no lock — the testable seam shared by
/// `complete_coding_workspace` and `recheck_coding_workspace`.
struct ReviewFields {
    summary: String,
    status_short: String,
    diff_stat: String,
    files: Vec<String>,
    test_output: String,
    risk_notes: Vec<String>,
    checks_ran: bool,
    checks_passed: bool,
}

fn compute_review_fields(
    worktree: &Path,
    test_command: Option<&str>,
    run_check: bool,
) -> Result<ReviewFields, String> {
    let snapshot = git::review_snapshot(worktree);
    if let Some(e) = snapshot.error {
        return Err(e);
    }
    let outcome = if run_check {
        match test_command {
            Some(cmd) => check::run_check(worktree, cmd, CHECK_TIMEOUT),
            None => check::CheckOutcome::not_run(),
        }
    } else {
        check::CheckOutcome::not_run()
    };
    let summary = review_svc::summarize(&snapshot);
    let risk_notes =
        completion::augment_risk_notes(review_svc::compute_risk_notes(&snapshot), &outcome);
    let test_output = completion::format_test_output(test_command.unwrap_or(""), &outcome);
    Ok(ReviewFields {
        summary,
        status_short: snapshot.status_short,
        diff_stat: snapshot.diff_stat,
        files: snapshot.files,
        test_output,
        risk_notes,
        checks_ran: outcome.ran,
        checks_passed: outcome.passed(),
    })
}
```

- [ ] **Step 4: Run the helper tests to verify they pass**

Run: `cd src-tauri && cargo test --lib commands::coding_workspaces`
Expected: PASS (both new tests + `worktree_cleanup_removes_dir`).

- [ ] **Step 5: Rewrite `complete_coding_workspace` to take `run_check` and use the helper**

In `src-tauri/src/commands/coding_workspaces.rs`, replace the entire `complete_coding_workspace` function (from `#[tauri::command]` through its closing `}`) with:

```rust
#[tauri::command]
pub fn complete_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    run_check: bool,
) -> Result<Review, String> {
    // Resolve the workspace + worktree path + configured command under the lock,
    // then release it before the (potentially slow) git + check work.
    let (workspace_id, worktree_path, test_command) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        let test_command = project::get(&conn, &cw.project_id)
            .map_err(|e| e.to_string())?
            .and_then(|p| project::test_command_from_settings(&p.settings_json));
        (cw.workspace_id, cw.worktree_path, test_command)
    };

    let fields =
        compute_review_fields(Path::new(&worktree_path), test_command.as_deref(), run_check)?;

    let review_id = new_id();
    let payload = serde_json::json!({
        "coding_workspace_id": coding_workspace_id,
        "review_id": review_id,
        "checks_ran": fields.checks_ran,
        "checks_passed": fields.checks_passed,
    })
    .to_string();

    // Persist the review, status move, and event together under one lock.
    let conn = state.lock().map_err(|e| e.to_string())?;
    let review = review::create(
        &conn,
        &review_id,
        &workspace_id,
        &coding_workspace_id,
        &fields.summary,
        &fields.status_short,
        &fields.diff_stat,
        &fields.files,
        test_command.as_deref(),
        &fields.test_output,
        &fields.risk_notes,
    )
    .map_err(|e| e.to_string())?;
    coding_workspace::update_status(&conn, &coding_workspace_id, "needs-review")
        .map_err(|e| e.to_string())?;
    event::create(
        &conn,
        &new_id(),
        &workspace_id,
        "coding_workspace.completed",
        &payload,
    )
    .map_err(|e| e.to_string())?;

    Ok(review)
}

/// Re-run the project's check command for an existing review and update it in place.
/// The async second half of completion → review automation: the auto path creates the
/// review instantly (`run_check=false`), then calls this to fill in check results
/// without blocking the review's appearance.
#[tauri::command]
pub fn recheck_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    review_id: String,
) -> Result<Review, String> {
    let (worktree_path, test_command) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(review) = review::get(&conn, &review_id).map_err(|e| e.to_string())? else {
            return Err(format!("Review '{review_id}' does not exist"));
        };
        let Some(cw) = coding_workspace::get(&conn, &review.coding_workspace_id)
            .map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{}' does not exist",
                review.coding_workspace_id
            ));
        };
        let test_command = project::get(&conn, &cw.project_id)
            .map_err(|e| e.to_string())?
            .and_then(|p| project::test_command_from_settings(&p.settings_json));
        (cw.worktree_path, test_command)
    };

    let fields = compute_review_fields(Path::new(&worktree_path), test_command.as_deref(), true)?;

    let conn = state.lock().map_err(|e| e.to_string())?;
    review::update_results(
        &conn,
        &review_id,
        &fields.summary,
        &fields.status_short,
        &fields.diff_stat,
        &fields.files,
        test_command.as_deref(),
        &fields.test_output,
        &fields.risk_notes,
    )
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("Review '{review_id}' does not exist"))
}
```

(The old multi-line doc comment above `complete_coding_workspace` describing "the workspace to Needs Review… only a snapshot or DB error aborts" is replaced by the function above — drop that stale comment.)

- [ ] **Step 6: Register `recheck_coding_workspace`**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![…]` list, add a line immediately after `commands::coding_workspaces::complete_coding_workspace,`:

```rust
            commands::coding_workspaces::recheck_coding_workspace,
```

(If the existing entry's exact name differs, add `recheck_coding_workspace` adjacent to the other `coding_workspaces::` commands.)

- [ ] **Step 7: Build + full backend test suite**

Run: `cd src-tauri && cargo build && cargo test`
Expected: clean build (both commands compile + registered); all tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands/coding_workspaces.rs src-tauri/src/lib.rs
git commit -m "feat(completion-review): run_check flag + recheck_coding_workspace + shared compute helper"
```

---

## Task 3: Frontend plumbing (api + stores)

**Files:**
- Modify: `src/api/codingWorkspaces.ts`
- Modify: `src/stores/codingWorkspaces.ts`
- Modify: `src/stores/reviews.ts`

**Context:** The Rust `complete_coding_workspace` now requires `run_check`. Default the JS wrappers to `true` so the existing `CodingView` caller stays correct; the SDK auto path passes `false`. `reviews.insert` already upserts by id (reused for the async update); add a `rechecking` map for the "running checks…" indicator. No Vitest in the repo — verify with `pnpm typecheck`.

- [ ] **Step 1: Update the api wrappers**

In `src/api/codingWorkspaces.ts`, replace the existing `completeCodingWorkspace` function with:

```ts
export function completeCodingWorkspace(id: string, runCheck = true): Promise<Review> {
  return invoke<Review>("complete_coding_workspace", { codingWorkspaceId: id, runCheck });
}

export function recheckCodingWorkspace(reviewId: string): Promise<Review> {
  return invoke<Review>("recheck_coding_workspace", { reviewId });
}
```

(`Review` is already imported at the top of this file.)

- [ ] **Step 2: Update the coding store**

In `src/stores/codingWorkspaces.ts`, replace the existing `complete` function with the two below, and add `recheck` to the returned object:

```ts
  async function complete(id: string, runCheck = true): Promise<Review> {
    const review = await api.completeCodingWorkspace(id, runCheck);
    // Completion deterministically moves the workspace to needs-review.
    const i = list.value.findIndex((c) => c.id === id);
    if (i >= 0) list.value[i] = { ...list.value[i], status: "needs-review" };
    return review;
  }

  async function recheck(reviewId: string): Promise<Review> {
    return api.recheckCodingWorkspace(reviewId);
  }
```

Then add `recheck` to the store's `return { … }` list (next to `complete`).

- [ ] **Step 3: Add `rechecking` to the reviews store**

In `src/stores/reviews.ts`, after the `insert` function, add:

```ts
  // Per-review "checks are running" flags, for the async recheck after an instant,
  // check-less auto-review. Keyed by review id.
  const rechecking = ref<Record<string, boolean>>({});
  function setRechecking(id: string, value: boolean) {
    rechecking.value = { ...rechecking.value, [id]: value };
  }
```

Then add `rechecking, setRechecking` to the store's `return { … }` list.

- [ ] **Step 4: Typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api/codingWorkspaces.ts src/stores/codingWorkspaces.ts src/stores/reviews.ts
git commit -m "feat(completion-review): api/store run_check + recheck + rechecking state"
```

---

## Task 4: SdkRunView auto-trigger + async recheck

**Files:**
- Modify: `src/components/SdkRunView.vue`

**Context:** Today `SdkRunView` shows a manual "Review changes" CTA. Make it auto-fire: when an edit run completes and the worktree is dirty, create the review instantly (`runCheck=false`), then recheck asynchronously. Fold the trigger into `watch([completed, diff])` so it also re-fetches the diff after a workspace switch wiped `coding.diffs` (the Frontend review's B1 bug). The manual button survives only as a retry if creation errors. `Review` type lives in `../types/review`.

- [ ] **Step 1: Import the Review type + add recheck state**

In `src/components/SdkRunView.vue` `<script setup>`, add to the imports:

```ts
import type { Review } from "../types/review";
```

After the `const completing = ref(false);` line, add:

```ts
// The review this run created, and whether its async checks are still running
// (drives the footer + the Reviews "running checks…" indicator via the store).
const createdReviewId = ref<string | null>(null);
const rechecking = computed(
  () => createdReviewId.value !== null && reviews.rechecking[createdReviewId.value] === true,
);
```

- [ ] **Step 2: Replace the completion watch with a folded `[completed, diff]` watch**

In `src/components/SdkRunView.vue`, replace the existing `watch(completed, …, { immediate: true })` block with:

```ts
// One ordered place for completion handling: when an edit run has finished, ensure
// the diff is loaded (re-fetching if a workspace switch wiped coding.diffs), then —
// if the worktree is dirty — auto-create the review. `showReview` stays purely
// presentational. Guards (reviewed/completing) keep this to one review per session.
watch(
  [completed, diff],
  async ([done, d]) => {
    if (!done || !isEdit.value) return;
    if (!d) {
      await coding.refreshDiff(props.session.coding_workspace_id);
      return;
    }
    if (!d.is_clean && !d.error && !reviewed.value && !completing.value) {
      await reviewChanges();
    }
  },
  { immediate: true },
);
```

- [ ] **Step 3: Rewrite `reviewChanges` to auto-create + async recheck**

In `src/components/SdkRunView.vue`, replace the existing `reviewChanges` function with:

```ts
// Auto-create the review from the diff snapshot (instant, no check), then run the
// project's check asynchronously and update the review in place. Also the manual
// retry if the creation step errors. Idempotent per session via completing/reviewed.
async function reviewChanges() {
  if (completing.value) return;
  completing.value = true;
  let review: Review;
  try {
    review = await coding.complete(props.session.coding_workspace_id, false);
  } catch (e) {
    toast.error(String(e));
    return;
  } finally {
    completing.value = false;
  }
  reviews.insert(review);
  createdReviewId.value = review.id;
  reviewed.value = true;
  toast.success("Review created — see Reviews");

  // Async checks: fill in check results without blocking the review. Best-effort —
  // the review stands even if the check can't run.
  reviews.setRechecking(review.id, true);
  try {
    reviews.insert(await coding.recheck(review.id));
  } catch {
    /* leave the review without check results */
  } finally {
    reviews.setRechecking(review.id, false);
  }
}
```

- [ ] **Step 4: Show the recheck state in the "review created" footer**

In `src/components/SdkRunView.vue` template, replace the `sdk-review-done` footer:

```html
    <footer v-else-if="reviewed" class="sdk-foot" data-testid="sdk-review-done">
      <span>✓ Review created — see Reviews</span>
    </footer>
```

with:

```html
    <footer v-else-if="reviewed" class="sdk-foot" data-testid="sdk-review-done">
      <span>✓ Review created — {{ rechecking ? "running checks…" : "see Reviews" }}</span>
    </footer>
```

(The `sdk-review-cta` footer above it is unchanged — it now renders only transiently while `completing`, and as the manual retry if creation errors.)

- [ ] **Step 5: Typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/SdkRunView.vue
git commit -m "feat(completion-review): SdkRunView auto-creates review + async recheck"
```

---

## Task 5: Prevent duplicates (CodingView) + Reviews indicator

**Files:**
- Modify: `src/components/CodingView.vue`
- Modify: `src/components/ReviewsView.vue`

**Context:** After auto-review flips a worktree to `needs-review`, `CodingView`'s "Complete and review" button must not be able to mint a second review. Gate it on `status !== 'needs-review'` (matching the existing "Mark ready" guard) — the shared store status flip hides it reactively. Add a small "running checks…" indicator to the Reviews row while a review is rechecking.

- [ ] **Step 1: Gate the "Complete and review" button**

In `src/components/CodingView.vue`, the "Complete and review" button currently starts:

```html
            <button
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="completingId === cw.id"
              @click="completeAndReview(cw.id)"
            >
              {{ completingId === cw.id ? "Running checks…" : "Complete and review" }}
            </button>
```

Add a `v-if` so it only shows before completion (mirrors the "Mark ready" guard on the sibling button):

```html
            <button
              v-if="cw.status !== 'needs-review'"
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="completingId === cw.id"
              @click="completeAndReview(cw.id)"
            >
              {{ completingId === cw.id ? "Running checks…" : "Complete and review" }}
            </button>
```

(`completeAndReview` calls `coding.complete(id)` — `runCheck` defaults to `true`, so the manual path stays synchronous and unchanged.)

- [ ] **Step 2: Add the Reviews "running checks…" indicator**

In `src/components/ReviewsView.vue`, the review row is:

```html
        <li
          v-for="r in ordered"
          :key="r.id"
          class="re-card review"
          :class="{ 'review--active': r.id === selectedId }"
          data-testid="review-row"
          @click="selectedId = r.id"
        >
          <span class="review__summary">{{ r.summary }}</span>
          <span class="re-badge" :data-tone="reviewTone(r.status)">{{ r.status }}</span>
        </li>
```

Add a muted indicator before the status badge:

```html
        <li
          v-for="r in ordered"
          :key="r.id"
          class="re-card review"
          :class="{ 'review--active': r.id === selectedId }"
          data-testid="review-row"
          @click="selectedId = r.id"
        >
          <span class="review__summary">{{ r.summary }}</span>
          <span v-if="reviews.rechecking[r.id]" class="review__checks" data-testid="review-rechecking">
            running checks…
          </span>
          <span class="re-badge" :data-tone="reviewTone(r.status)">{{ r.status }}</span>
        </li>
```

Then add the style inside the `<style scoped>` block (after the `.review__summary` rule):

```css
.review__checks {
  font-size: 0.72rem;
  color: var(--re-color-text-muted);
  white-space: nowrap;
}
```

- [ ] **Step 3: Typecheck**

Run: `pnpm typecheck`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/components/CodingView.vue src/components/ReviewsView.vue
git commit -m "feat(completion-review): gate duplicate completion + Reviews rechecking indicator"
```

---

## Task 6: End-to-end test

**Files:**
- Modify: `e2e/specs/agent-sdk.e2e.ts`

**Context:** Two behaviours to lock in: (1) an edit-mode run auto-creates exactly one review with no click, and (2) a plan-mode run creates no review. The `describe` already sets up `SdkProj`/`SdkFixture`/`SDK Acct` and runs SDK sessions. `SdkProj` has **no** check command, so the review is instant + final (the recheck path is a no-op here; the recheck/update logic is covered by the Task 1/2 Rust tests). Reuse the existing `feedText`/`allFeedsText` helpers; multi-line writes via `browser.execute`; poll async DOM with `waitUntil`.

**Update the existing edit-mode test** "edit mode changes the worktree and offers a review that persists":

- [ ] **Step 1: Replace the edit-mode test body to assert auto-creation (no click)**

In `e2e/specs/agent-sdk.e2e.ts`, replace the body of the `it("edit mode changes the worktree and offers a review that persists", …)` test with the following. Note the ordering: count reviews **first**, then fill the Agents form and launch **without navigating away** (navigating away from Agents mid-setup could reset the launch form):

```ts
    // Count existing reviews before this run so we can assert exactly one is added.
    await (await $("button*=Reviews")).click();
    const reviewsBefore = await $$('[data-testid="review-row"]').length;

    // Fill the launch form and start the edit run in one go (no nav in between).
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk-edit");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");
    await (await $('[aria-label="Agent mode"]')).selectByVisibleText("Edit");
    await (await $('[aria-label="Agent goal"]')).setValue("edit the readme");
    await (await $("button*=New terminal")).click();

    // No click: the review is created automatically once the agent completes and the
    // worktree diff resolves dirty. The "Review created" footer appears on its own.
    await (await $('[data-testid="sdk-review-done"]')).waitForExist({ timeout: 25_000 });

    // Exactly one new review persisted, and it captured the agent's file write.
    await (await $("button*=Reviews")).click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="review-row"]').length) === reviewsBefore + 1,
      { timeout: 10_000, timeoutMsg: "expected exactly one new review from the edit run" },
    );
    await (await $$('[data-testid="review-row"]'))[0].click();
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => document.querySelector('[data-testid="review-detail"]')?.textContent ?? "",
        )).includes("AGENT_EDIT.md"),
      { timeout: 10_000, timeoutMsg: "expected the review to list the agent's changed file" },
    );
```

- [ ] **Step 2: Strengthen the plan-mode test to assert no review is auto-created**

In `e2e/specs/agent-sdk.e2e.ts`, in the test `it("runs a plan-only SDK session, streams the feed, never exposes the key", …)`, the body already waits for `"Planning:"` then `"Done"` and asserts `sdk-review-cta` count is 0. Immediately after the existing `expect((await $$('[data-testid="sdk-review-cta"]')).length).toBe(0);` line, add:

```ts
    // Plan mode never edits, so completion must NOT auto-create a review. The "Done"
    // wait above already gated on a positive completion signal, so this is not racy.
    expect((await $$('[data-testid="sdk-review-done"]')).length).toBe(0);
    await (await $("button*=Reviews")).click();
    expect(await $$('[data-testid="review-row"]').length).toBe(0);
    await (await $("button*=Agents")).click();
```

- [ ] **Step 3: Run the e2e suite**

Run: `pnpm e2e:docker`
Expected: PASS — all 12 spec files, including the updated `agent-sdk` tests. (Authoritative gate; builds the Docker image with the fake sidecars. `pnpm e2e` runs locally if iterating.)

- [ ] **Step 4: Commit**

```bash
git add e2e/specs/agent-sdk.e2e.ts
git commit -m "test(completion-review): e2e auto-create on edit completion + plan no-review"
```

---

## Self-Review

**Spec coverage**
- Auto-create instant review (diff only) on edit completion → Task 4 (`reviewChanges` `complete(…, false)`) + Task 2 (`run_check=false`). ✓
- Async check that updates the review → Task 2 (`recheck_coding_workspace`) + Task 1 (`update_results`) + Task 4 (recheck orchestration). ✓
- Worktree → `needs-review`, toast, board surfacing → reused `complete_coding_workspace` (unchanged status move) + existing toast. ✓
- Fix the workspace-switch strand bug → Task 4 (folded `watch([completed, diff])` re-fetches diff when missing). ✓
- Duplicate prevention via CodingView gating (not a backend guard) → Task 5. ✓
- "running checks…" indicator → Task 4 footer + Task 5 ReviewsView. ✓
- SDK edit only, no toggle → no plan/PTY/setting code added. ✓
- Tests: `update_results` (Task 1), `compute_review_fields` run_check gating + check wiring (Task 2), e2e auto-create exactly-one + `AGENT_EDIT.md` + plan no-review (Task 6). ✓
- Security revision documented → in the spec (no code). ✓

**Placeholder scan:** none — every code step has complete code; every run step has a command + expected result.

**Type consistency:** `complete_coding_workspace(state, coding_workspace_id, run_check: bool) -> Review`; `recheck_coding_workspace(state, review_id) -> Review`; `review::update_results(conn, id, summary, status_short, diff_stat, files, test_command, test_output, risk_notes) -> Option<Review>`; `compute_review_fields(&Path, Option<&str>, bool) -> Result<ReviewFields, String>`; api `completeCodingWorkspace(id, runCheck=true)` / `recheckCodingWorkspace(reviewId)`; store `complete(id, runCheck=true)` / `recheck(reviewId)`; `reviews.rechecking` (Record) + `setRechecking(id, value)`; `reviewChanges()` called by both the watch and the retry button. Names/signatures match across tasks and the `{ codingWorkspaceId, runCheck }` / `{ reviewId }` invoke keys match the Rust snake_case params.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-23-completion-review-automation.md`.
