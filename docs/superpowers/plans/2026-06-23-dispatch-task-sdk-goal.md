# Dispatch task → SDK agent goal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-prefill the Claude Agent SDK launch goal in AgentsView from a *dispatched* worktree's task title + source artifact content, so a researched plan flows straight into SDK-agent execution.

**Architecture:** A pure formatter + a conn-testable resolver in `services/dispatch.rs` (mirroring `services/board.rs`), exposed by a thin `get_dispatched_goal` command. AgentsView calls the api directly and seeds the goal textarea on worktree/adapter change — race-guarded by a monotonic token, dirty-checked so it never stomps user edits, SDK-only, with seeded/size hints. No migration, no network, no secret.

**Tech Stack:** Rust (rusqlite, Tauri commands), Vue 3 `<script setup>` + Pinia, WebdriverIO e2e.

**Spec:** `docs/superpowers/specs/2026-06-23-dispatch-task-sdk-goal-design.md`

**Conventions:** Work on branch `cstuncsik/dispatch-task-sdk-goal`. Every commit ends with the trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` (omitted from the brief commit commands below for readability — always append it). Rust commands run from `src-tauri/` (`cargo test`, `cargo build`); frontend from repo root (`pnpm typecheck`).

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src-tauri/src/services/dispatch.rs` | Pure goal formatter + conn-testable resolver (+ their unit tests) | Modify |
| `src-tauri/src/commands/dispatch.rs` | Thin `get_dispatched_goal` Tauri command | Modify |
| `src-tauri/src/lib.rs` | Register the command | Modify |
| `src/api/codingWorkspaces.ts` | `getDispatchedGoal` invoke wrapper | Modify |
| `src/components/AgentsView.vue` | Prefill state, helper, watches, hints, rows | Modify |
| `e2e/specs/agent-sdk.e2e.ts` | End-to-end: dispatch → prefill → seed-not-binding | Modify |

---

## Task 1: Pure goal formatter

**Files:**
- Modify/Test: `src-tauri/src/services/dispatch.rs`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/services/dispatch.rs`, inside `mod tests { … }` (after the existing `handles_non_ascii_without_panic` test, before the closing `}`), add:

```rust
    #[test]
    fn formats_task_then_context_then_content() {
        let g = format_dispatched_goal("Add login", "## Steps\n- do it\n");
        assert_eq!(
            g,
            "Task: Add login\n\nContext — the plan this task was dispatched from:\n\n## Steps\n- do it\n"
        );
    }

    #[test]
    fn format_trims_title_and_keeps_multibyte() {
        let g = format_dispatched_goal("  Café déjà — vu  ", "café\n");
        assert_eq!(
            g,
            "Task: Café déjà — vu\n\nContext — the plan this task was dispatched from:\n\ncafé\n"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib services::dispatch::tests::formats_task_then_context_then_content`
Expected: FAIL to compile — `cannot find function format_dispatched_goal in this scope`.

- [ ] **Step 3: Write the formatter**

In `src-tauri/src/services/dispatch.rs`, after the `extract_tasks` function and its trailing `//` comment block (before `#[cfg(test)]`), add:

```rust
/// Assemble the SDK agent's goal from a dispatched task: the task title as the
/// instruction, then the source artifact's content as plan context. Pure. The title
/// legitimately repeats inside the artifact (it was extracted from it); the `Task:`
/// line is what distinguishes N worktrees dispatched from one artifact.
pub fn format_dispatched_goal(task_title: &str, artifact_content: &str) -> String {
    format!(
        "Task: {}\n\nContext — the plan this task was dispatched from:\n\n{}",
        task_title.trim(),
        artifact_content
    )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib services::dispatch`
Expected: PASS (the two new tests plus the existing `extract_tasks` tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/dispatch.rs
git commit -m "feat(m10b-2b): pure format_dispatched_goal helper"
```

---

## Task 2: Conn-testable resolver

**Files:**
- Modify/Test: `src-tauri/src/services/dispatch.rs`

**Context:** The resolver walks `cw.session_id → session.title` + `session.created_from_artifact_id → artifact.content`. It returns `Some(goal)` only when the full chain resolves and both fields are non-empty after trim; otherwise `None`. Model `get()` fns all return `rusqlite::Result<Option<T>>`. The test harness mirrors `services/board.rs` (in-memory DB, `PRAGMA foreign_keys = ON`, `run_migrations`).

- [ ] **Step 1: Add the model imports**

At the very top of `src-tauri/src/services/dispatch.rs`, replace the module doc comment block:

```rust
//! Pure helpers for dispatch: extract candidate tasks from an artifact's markdown
//! and derive git-ref-safe branch names. No IO — unit-tested.
```

with:

```rust
//! Dispatch helpers: extract candidate tasks from an artifact's markdown, assemble
//! the dispatched-task goal (pure), and the conn-testable goal resolver.

use rusqlite::Connection;

use crate::models::{artifact, coding_workspace, session};
```

- [ ] **Step 2: Write the failing tests**

In `src-tauri/src/services/dispatch.rs`, inside `mod tests`, add the harness, fixtures, and cases (after the `format_*` tests, before the closing `}`):

```rust
    use crate::models::{project, repository, workspace};
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        conn
    }

    fn base(conn: &Connection) -> (String, String, String) {
        let ws = workspace::create(conn, "WS", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None).unwrap().id;
        (ws, p, r)
    }

    fn cw_with_session(
        conn: &Connection,
        ws: &str,
        p: &str,
        r: &str,
        session_id: Option<&str>,
    ) -> String {
        let id = new_id();
        coding_workspace::create(
            conn, &id, ws, p, r, "/tmp/repo", &format!("/tmp/wt/{id}"), "feat/x", "main",
            session_id,
        )
        .unwrap();
        id
    }

    #[test]
    fn missing_workspace_has_no_goal() {
        let conn = migrated_conn();
        assert_eq!(resolve_dispatched_goal(&conn, "nope").unwrap(), None);
    }

    #[test]
    fn plain_worktree_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let cw = cw_with_session(&conn, &ws, &p, &r, None);
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn dispatched_session_without_artifact_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        // A session not born from an artifact (created_from_artifact_id = None).
        let sess = session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", None).unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn empty_artifact_content_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let art = artifact::create(&conn, &ws, Some(&p), "Plan").unwrap(); // content starts ""
        let sess =
            session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", Some(&art.id))
                .unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn full_chain_seeds_task_plus_artifact() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let art = artifact::create(&conn, &ws, Some(&p), "Plan").unwrap();
        artifact::update(&conn, &art.id, "Plan", "## Steps\n- do it\n").unwrap();
        let sess =
            session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", Some(&art.id))
                .unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(
            resolve_dispatched_goal(&conn, &cw).unwrap(),
            Some(
                "Task: Add login\n\nContext — the plan this task was dispatched from:\n\n## Steps\n- do it\n"
                    .to_string()
            )
        );
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib services::dispatch::tests::full_chain_seeds_task_plus_artifact`
Expected: FAIL to compile — `cannot find function resolve_dispatched_goal in this scope`.

- [ ] **Step 4: Write the resolver**

In `src-tauri/src/services/dispatch.rs`, after `format_dispatched_goal` (before `#[cfg(test)]`), add:

```rust
/// Resolve the prefill goal for a (possibly dispatched) coding workspace:
/// `cw.session_id → session.title` + `session.created_from_artifact_id →
/// artifact.content`, assembled by `format_dispatched_goal`. Returns `None` for a
/// plain worktree or any incomplete/empty chain — seed only a real dispatched task
/// with content.
///
/// No workspace-scoping (unlike `list_account_models`, which gates cross-workspace
/// credential access): this reads only objects transitively owned by the cw
/// (cw → its session → that session's artifact), so there is no boundary to enforce.
pub fn resolve_dispatched_goal(
    conn: &Connection,
    coding_workspace_id: &str,
) -> rusqlite::Result<Option<String>> {
    let Some(cw) = coding_workspace::get(conn, coding_workspace_id)? else {
        return Ok(None);
    };
    let Some(session_id) = cw.session_id else {
        return Ok(None);
    };
    let Some(sess) = session::get(conn, &session_id)? else {
        return Ok(None);
    };
    let Some(artifact_id) = sess.created_from_artifact_id else {
        return Ok(None);
    };
    let Some(art) = artifact::get(conn, &artifact_id)? else {
        return Ok(None);
    };
    if sess.title.trim().is_empty() || art.content.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(format_dispatched_goal(&sess.title, &art.content)))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib services::dispatch`
Expected: PASS (all resolver cases + the formatter + `extract_tasks` tests).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/dispatch.rs
git commit -m "feat(m10b-2b): resolve_dispatched_goal (cw → session → artifact)"
```

---

## Task 3: `get_dispatched_goal` command + registration

**Files:**
- Modify: `src-tauri/src/commands/dispatch.rs`
- Modify: `src-tauri/src/lib.rs`

**Context:** A thin wrapper mirroring `commands/board.rs::get_board` — lock, delegate to the service, `map_err`. `commands/dispatch.rs` already imports `std::sync::Mutex`, `rusqlite::Connection`, `tauri::State`, and `crate::services::dispatch as svc`. No new unit test: the wrapper is trivial and the resolver is covered by Task 2; the command is exercised by the Task 5 e2e. Verification is a clean build (it compiles and is registered).

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands/dispatch.rs`, after the `dispatch_artifact` function (immediately before `#[cfg(test)]`), add:

```rust
/// Prefill goal for a dispatched worktree (task title + source artifact content), or
/// null for a plain worktree. Best-effort: the frontend seeds the SDK goal textarea.
/// One short lock, DB reads only. The goal (incl. the full artifact) reaches the
/// sidecar as argv, as goals already do — acceptable on a single-user host;
/// credentials are env-only and never travel in argv.
#[tauri::command]
pub fn get_dispatched_goal(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
) -> Result<Option<String>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    svc::resolve_dispatched_goal(&conn, &coding_workspace_id).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![ … ]` list, add a line immediately after `commands::dispatch::dispatch_artifact,`:

```rust
            commands::dispatch::get_dispatched_goal,
```

- [ ] **Step 3: Build and run the full backend test suite**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds cleanly (command compiles, registered with no arity/type error); all tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/dispatch.rs src-tauri/src/lib.rs
git commit -m "feat(m10b-2b): get_dispatched_goal command + registration"
```

---

## Task 4: Frontend prefill (api + AgentsView)

**Files:**
- Modify: `src/api/codingWorkspaces.ts`
- Modify: `src/components/AgentsView.vue`

**Context:** AgentsView's goal textarea is already `v-if="selectedIsSdk"` bound to `newGoal`, so prefill is SDK-only by construction. We add the api wrapper, prefill state/helper, extend three existing watches, and add the seeded/size hints. No frontend unit-test toolchain exists (no Vitest); verification is `pnpm typecheck`, and the Task 5 e2e proves the behavior.

- [ ] **Step 1: Add the api wrapper**

In `src/api/codingWorkspaces.ts`, after the `completeCodingWorkspace` function, add:

```ts
export function getDispatchedGoal(id: string): Promise<string | null> {
  return invoke<string | null>("get_dispatched_goal", { codingWorkspaceId: id });
}
```

- [ ] **Step 2: Import the api into AgentsView**

In `src/components/AgentsView.vue`, in the `<script setup>` import block, after the line `import { useToast } from "../composables/useToast";`, add:

```ts
import * as codingApi from "../api/codingWorkspaces";
```

- [ ] **Step 3: Add prefill state**

In `src/components/AgentsView.vue`, immediately after the existing ref block (after `const starting = ref(false);`), add:

```ts
// Goal prefill from a dispatched worktree. `seededValue` is the last value we
// seeded (for the dirty-check); `goalToken` (non-reactive, like the store's
// loadToken) makes "last prefill started wins" deterministic across async fetches.
const seededValue = ref("");
const seeded = computed(() => newGoal.value === seededValue.value && seededValue.value !== "");
const goalBytes = computed(() => new TextEncoder().encode(newGoal.value).length);
const goalKb = computed(() => Math.round(goalBytes.value / 1000));
const goalTooLarge = computed(() => selectedIsSdk.value && goalBytes.value > 100_000);
let goalToken = 0;
```

(`computed` is already imported on line 2.)

- [ ] **Step 4: Add the prefill helper**

In `src/components/AgentsView.vue`, immediately before the `async function openTerminal()` declaration, add:

```ts
// Seed the goal from a dispatched worktree (SDK-only). Best-effort: a failed fetch
// never clobbers or toasts. Dirty-checked so it never overwrites the user's edits.
async function prefillGoal(id: string) {
  if (!id) return;
  const token = ++goalToken;
  let goal: string | null = null;
  try {
    goal = await codingApi.getDispatchedGoal(id);
  } catch {
    return;
  }
  if (token !== goalToken) return; // a newer prefill superseded us
  if (newWorktreeId.value !== id || !selectedIsSdk.value) return;
  if (newGoal.value === seededValue.value || newGoal.value.trim() === "") {
    newGoal.value = goal ?? "";
    seededValue.value = goal ?? "";
  }
}
```

- [ ] **Step 5: Extend the adapter watch**

In `src/components/AgentsView.vue`, replace the existing adapter watch:

```ts
watch(newAdapterId, () => {
  newAccountId.value = "";
  newGoal.value = "";
  newMode.value = "plan";
  newModelId.value = "";
});
```

with:

```ts
watch(newAdapterId, (val) => {
  newAccountId.value = "";
  newGoal.value = "";
  newMode.value = "plan";
  newModelId.value = "";
  seededValue.value = "";
  // Switched into the SDK with a worktree already chosen → seed now. Use
  // adapterKind(val) (not the lazy selectedIsSdk) so correctness doesn't depend on
  // computed-evaluation timing.
  if (adapterKind(val) === "sdk" && newWorktreeId.value) prefillGoal(newWorktreeId.value);
});
```

- [ ] **Step 6: Extend the worktree watch**

In `src/components/AgentsView.vue`, replace the existing worktree watch:

```ts
// If the worktree is chosen after the account, fetch models then (cache-hit otherwise).
watch(newWorktreeId, (val) => {
  if (val && selectedIsSdk.value && newAccountId.value) {
    accountModels.loadModels(val, newAccountId.value);
  }
});
```

with:

```ts
// On worktree change (SDK): lazy-load that account's models (if an account is set)
// and seed the goal from the dispatched task.
watch(newWorktreeId, (val) => {
  if (val && selectedIsSdk.value) {
    if (newAccountId.value) accountModels.loadModels(val, newAccountId.value);
    prefillGoal(val);
  }
});
```

- [ ] **Step 7: Clear the seed on workspace switch**

In `src/components/AgentsView.vue`, in the `watch(() => workspaces.currentId, (newId, oldId) => { … })` body, immediately after the line `newGoal.value = "";`, add:

```ts
    seededValue.value = "";
```

- [ ] **Step 8: Make the goal box reviewable + add the hints**

In `src/components/AgentsView.vue` template, change the goal textarea's `rows="2"` to a binding:

```html
        <textarea
          v-if="selectedIsSdk"
          v-model="newGoal"
          class="re-input new__goal"
          :rows="seeded ? 8 : 2"
          placeholder="What should the agent do?"
          aria-label="Agent goal"
        ></textarea>
```

Then immediately after that `</textarea>`, add the two hints:

```html
        <p v-if="seeded" class="muted new__hint" data-testid="goal-seeded-hint">
          Prefilled from the dispatched task — editable.
        </p>
        <p v-if="goalTooLarge" class="muted new__hint" data-testid="goal-too-large">
          Plan is large (~{{ goalKb }} KB) — trim before starting; very large goals can fail to
          launch.
        </p>
```

- [ ] **Step 9: Typecheck**

Run: `pnpm typecheck`
Expected: PASS (no type errors).

- [ ] **Step 10: Commit**

```bash
git add src/api/codingWorkspaces.ts src/components/AgentsView.vue
git commit -m "feat(m10b-2b): prefill SDK goal from dispatched worktree"
```

---

## Task 5: End-to-end test

**Files:**
- Modify: `e2e/specs/agent-sdk.e2e.ts`

**Context:** The spec's `describe` already sets up project `SdkProj`, repo `SdkFixture`, account `SDK Acct`, and runs SDK sessions on hand-created worktrees (which have no dispatch link). This test adds the *dispatched* path: create an artifact, dispatch one task into a worktree `feat/seeded`, then in Agents select it + SDK + account and prove (a) the goal is prefilled with the task title **and** an artifact-content snippet, and (b) the *edited* goal — not the seeded blob — is what reaches the sidecar (seed-not-binding). The prefill is an async `invoke`, so every read is `waitUntil`-polled (the slice-3 CI-flake class). Multi-line textareas are set via `browser.execute` (`setValue` types `\n` as Enter). The fake sidecar echoes the goal as `{"type":"assistant","text":"Planning: <goal>"}`, so the feed carries the launched goal.

Reference the dispatch flow in `e2e/specs/dispatch.e2e.ts` (artifact create → set markdown via `browser.execute` → Save → Dispatch dialog → pick project/repo/base → Dispatch). The dialog exposes `[aria-label="Task 1 branch"]` to override the auto-slugged branch.

- [ ] **Step 1: Write the test**

In `e2e/specs/agent-sdk.e2e.ts`, add this test as the **last** `it(...)` inside the `describe("claude agent sdk (plan-only)", …)` block (immediately before the final closing `});`):

```ts
  it("prefills the goal from a dispatched task and launches the edited goal", async () => {
    // Author an artifact with one checklist task + a content marker.
    await (await $("button*=Artifacts")).click();
    await (await $('[aria-label="New artifact title"]')).setValue("SeedPlan");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="artifact-editor"]')).waitForExist({ timeout: 10_000 });
    // Set the markdown via the DOM (real newlines + input event for v-model).
    await browser.execute((val) => {
      const ta = document.querySelector(
        '[aria-label="Markdown source"]',
      ) as HTMLTextAreaElement | null;
      if (ta) {
        ta.value = val;
        ta.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }, "# Seed Plan\n\nSeed context marker line.\n\n- [ ] Wire the seeded goal\n");
    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    const editor = await $('[data-testid="artifact-editor"]');
    await editor.$("button*=Save").click();
    await browser.waitUntil(
      async () => !(await $('[data-testid="artifact-dirty"]').isExisting()),
      { timeout: 10_000, timeoutMsg: "expected the artifact save to persist" },
    );

    // Dispatch the single task into a known branch.
    await editor.$("button*=Dispatch").click();
    const dialog = await $('[data-testid="dispatch-dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await browser.waitUntil(
      async () => (await $$('[data-testid="dispatch-task-row"]').length) === 1,
      { timeout: 10_000, timeoutMsg: "expected one seeded task row" },
    );
    await dialog.$('[aria-label="Task 1 branch"]').setValue("feat/seeded");
    await dialog.$('[aria-label="Dispatch project"]').selectByVisibleText("SdkProj");
    await dialog.$('[aria-label="Dispatch repository"]').selectByVisibleText("SdkFixture");
    const base = await dialog.$('[aria-label="Dispatch base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await dialog.$("button*=Dispatch").click();
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () =>
            document.querySelector('[data-testid="dispatch-dialog"] .results')?.textContent ?? "",
        )).includes("worktree created"),
      { timeout: 20_000, timeoutMsg: "expected the dispatch to create a worktree" },
    );
    await dialog.$("button*=Close").click();

    // Visit Coding so the worktree list reloads with the dispatched worktree.
    await (await $("button*=Coding")).click();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(
            () =>
              [...document.querySelectorAll('[data-testid="coding-row"]')]
                .map((r) => r.textContent ?? "")
                .join("\n"),
          )
        ).includes("feat/seeded"),
      { timeout: 15_000, timeoutMsg: "expected the dispatched worktree in Coding" },
    );

    // In Agents, select the dispatched worktree + SDK + account → goal prefills.
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/seeded");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");

    const goal = await $('[aria-label="Agent goal"]');
    await browser.waitUntil(
      async () => {
        const v = await goal.getValue();
        return v.includes("Wire the seeded goal") && v.includes("Seed context marker line");
      },
      { timeout: 15_000, timeoutMsg: "expected the goal to prefill from the dispatched task" },
    );
    // The seeded indicator is shown.
    expect(await (await $('[data-testid="goal-seeded-hint"]')).isExisting()).toBe(true);

    // Edit the goal, launch, and prove the EDITED goal (not the seed) reached the
    // sidecar — seed-not-binding.
    await browser.execute(() => {
      const ta = document.querySelector(
        '[aria-label="Agent goal"]',
      ) as HTMLTextAreaElement | null;
      if (ta) {
        ta.value = "EDITED seeded goal run";
        ta.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });
    await (await $("button*=New terminal")).click();
    await browser.waitUntil(
      async () => (await allFeedsText()).includes("Planning: EDITED seeded goal run"),
      { timeout: 15_000, timeoutMsg: "expected the edited goal to reach the sidecar" },
    );
  });
```

- [ ] **Step 2: Run the e2e suite**

Run: `pnpm e2e:docker`
Expected: PASS — the full `agent-sdk` spec (including the new test) and all other specs are green. (This is the authoritative gate; it builds the Docker image with the fake sidecars. If iterating, `pnpm e2e` runs the suite locally, but `e2e:docker` matches CI.)

- [ ] **Step 3: Commit**

```bash
git add e2e/specs/agent-sdk.e2e.ts
git commit -m "test(m10b-2b): e2e dispatch→goal prefill + seed-not-binding"
```

---

## Self-Review

**Spec coverage**
- Goal = task title + full artifact content → Task 1 (`format_dispatched_goal`). ✓
- On-demand resolver, strict degrade ladder, no migration → Task 2 (all branches tested). ✓
- Thin command, standard error mapping, no scoping (documented), argv note → Task 3. ✓
- Direct api call (no store passthrough) → Task 4 Step 1–2. ✓
- `adapterKind(val)` over lazy `selectedIsSdk`; monotonic token; empty-id guard; error swallow; dirty-check → Task 4 Step 3–6. ✓
- Seeded hint, reviewable rows, large-goal warning → Task 4 Step 3, 8. ✓
- `seededValue` cleared on adapter + workspace switch → Task 4 Step 5, 7. ✓
- Unit tests (formatter exact + multibyte; resolver incl. dispatched-vs-plain boundary, no-artifact, empty-content, full) → Tasks 1–2. ✓
- e2e proves prefill (polled) + seed-not-binding → Task 5. ✓
- No Vitest → frontend verified by typecheck + e2e. ✓

**Placeholder scan:** none — every code step shows complete code; every run step shows the command + expected result.

**Type consistency:** `format_dispatched_goal(&str, &str) -> String`, `resolve_dispatched_goal(&Connection, &str) -> rusqlite::Result<Option<String>>`, command `get_dispatched_goal(State, String) -> Result<Option<String>, String>`, api `getDispatchedGoal(string) -> Promise<string | null>`, helper `prefillGoal(string)` — names and signatures match across all tasks and the registration. The exact goal string is identical in Task 1, Task 2, and the format helper.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-23-dispatch-task-sdk-goal.md`.
