import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-dispatch";

/**
 * Milestone 11 end-to-end: from a markdown artifact with checklist tasks, dispatch
 * to coding sessions + worktrees, verify the inline results, the artifact back-link,
 * and that the worktrees appear in Coding.
 */
describe("dispatch artifact to coding tasks", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project + attached repo", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("DispProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("DispFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("dispatches an artifact's tasks into worktrees", async () => {
    await (await $("button*=Artifacts")).click();
    await (await $('[aria-label="New artifact title"]')).setValue("Plan");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="artifact-editor"]')).waitForExist({ timeout: 10_000 });

    // Two checklist tasks. Wait for the edit to register (dirty appears), Save,
    // then wait for it to PERSIST before dispatching — extract_artifact_tasks reads
    // the artifact from the DB, so dispatching before the update lands seeds empty.
    // Set the value via the DOM (with real newlines + an input event for v-model):
    // wdio setValue treats "\n" as Enter and can collapse the lines, which would
    // leave the checklist items mid-line and unparseable by extract_tasks.
    await browser.execute((val) => {
      const ta = document.querySelector(
        '[aria-label="Markdown source"]',
      ) as HTMLTextAreaElement | null;
      if (ta) {
        ta.value = val;
        ta.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }, "# Plan\n\n- [ ] Dispatch one\n- [ ] Dispatch two\n");
    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    const editor = await $('[data-testid="artifact-editor"]');
    await editor.$("button*=Save").click();
    await browser.waitUntil(async () => !(await $('[data-testid="artifact-dirty"]').isExisting()), {
      timeout: 10_000,
      timeoutMsg: "expected the artifact save to persist",
    });

    // Open the dispatch dialog (tasks are seeded from the checklist).
    await editor.$("button*=Dispatch").click();
    const dialog = await $('[data-testid="dispatch-dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await browser
      .waitUntil(async () => (await $$('[data-testid="dispatch-task-row"]').length) === 2, {
        timeout: 10_000,
        timeoutMsg: "two seeded task rows",
      })
      .catch(async () => {
        const body = await textOf('[data-testid="dispatch-dialog"]');
        const n = await $$('[data-testid="dispatch-task-row"]').length;
        throw new Error(`expected 2 task rows, got ${n}. dialog body: ${body.slice(0, 400)}`);
      });

    // Pick project/repo/base.
    await dialog.$('[aria-label="Dispatch project"]').selectByVisibleText("DispProj");
    await dialog.$('[aria-label="Dispatch repository"]').selectByVisibleText("DispFixture");
    const base = await dialog.$('[aria-label="Dispatch base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");

    await dialog.$("button*=Dispatch").click();

    // Inline results: both tasks created a worktree.
    await browser.waitUntil(
      async () =>
        (await textOf('[data-testid="dispatch-dialog"] .results')).includes("worktree created"),
      { timeout: 20_000, timeoutMsg: "expected dispatch results" },
    );
    await dialog.$("button*=Close").click();

    // Back-link on the artifact.
    await browser.waitUntil(
      async () => (await textOf('[data-testid="dispatched-sessions"]')).includes("Dispatched: 2"),
      { timeout: 10_000, timeoutMsg: "expected the artifact to show 2 dispatched sessions" },
    );

    // Worktrees show up in Coding.
    await (await $("button*=Coding")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]').length) === 2, {
      timeout: 10_000,
      timeoutMsg: "expected two dispatched worktrees in Coding",
    });
  });

  it("is resilient: re-dispatch creates sessions but reports per-task worktree failures", async () => {
    // Re-open the same artifact and dispatch again. The two branches already exist
    // from the first dispatch, so each worktree fails — but the sessions are still
    // created and linked, proving 'session always, worktree best-effort'.
    await (await $("button*=Artifacts")).click();
    await (await $('[data-testid="artifact-row"]')).waitForExist({ timeout: 10_000 });
    await (await $$('[data-testid="artifact-row"]'))[0].click();

    const editor = await $('[data-testid="artifact-editor"]');
    await editor.$("button*=Dispatch").click();
    const dialog = await $('[data-testid="dispatch-dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await browser.waitUntil(
      async () => (await $$('[data-testid="dispatch-task-row"]').length) === 2,
      { timeout: 10_000, timeoutMsg: "expected the two tasks re-seeded" },
    );

    await dialog.$('[aria-label="Dispatch project"]').selectByVisibleText("DispProj");
    await dialog.$('[aria-label="Dispatch repository"]').selectByVisibleText("DispFixture");
    const base = await dialog.$('[aria-label="Dispatch base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");

    await dialog.$("button*=Dispatch").click();

    // Both worktrees fail (branch already exists) → two error result rows.
    await browser.waitUntil(
      async () => (await $$('[data-testid="dispatch-dialog"] .result--err').length) === 2,
      { timeout: 20_000, timeoutMsg: "expected both re-dispatched worktrees to error" },
    );
    await dialog.$("button*=Close").click();

    // The sessions were still created and linked: the count grew from 2 to 4.
    await browser.waitUntil(
      async () => (await textOf('[data-testid="dispatched-sessions"]')).includes("Dispatched: 4"),
      {
        timeout: 10_000,
        timeoutMsg: "expected 4 dispatched sessions (2 + 2) despite worktree failures",
      },
    );

    // No new worktrees were created (still 2).
    await (await $("button*=Coding")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]').length) === 2, {
      timeout: 10_000,
      timeoutMsg: "expected still only two worktrees after the failed re-dispatch",
    });
  });
});
