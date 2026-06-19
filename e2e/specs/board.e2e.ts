import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-board";

const colText = (stage: string) =>
  browser.execute((s) => {
    const col = document.querySelector(`[data-testid="board-column"][data-stage="${s}"]`);
    return col ? (col.textContent ?? "") : "";
  }, stage);

/**
 * Milestone 12 end-to-end: build two worktrees, review one, and verify the board
 * groups them into the right stage columns, that deciding a review moves a card,
 * and that two reviews compare side-by-side.
 */
describe("parallel agent board", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a project, repo, and two worktrees (one reviewed)", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("BoardProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("BoardFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    // Worktree A (will get a review), worktree B (stays in progress).
    for (const branch of ["board/a", "board/b"]) {
      await (await $("button*=Coding")).click();
      await (await $('[aria-label="Coding project"]')).selectByVisibleText("BoardProj");
      await (await $('[aria-label="Coding repository"]')).selectByVisibleText("BoardFixture");
      const base = await $('[aria-label="Base branch"]');
      await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
      await base.selectByVisibleText("main");
      await (await $('[aria-label="New branch name"]')).setValue(branch);
      await (await $("button*=Create worktree")).click();
      await browser.waitUntil(
        async () => (await textOf('[data-testid="coding-row"] .coding__branch')).includes(branch),
        { timeout: 15_000, timeoutMsg: `expected worktree ${branch}` },
      );
    }

    // Create a review on the first worktree (board/a → status pending → Needs review).
    const firstRow = (await $$('[data-testid="coding-row"]'))[
      (await $$('[data-testid="coding-row"]').length) - 1
    ];
    await firstRow.$("button*=Create review").click();
  });

  it("groups worktrees into stage columns and moves a card when a review is decided", async () => {
    await (await $("button*=Board")).click();
    await (await $('[data-testid="board"]')).waitForExist({ timeout: 10_000 });

    // board/b has no review → In progress; board/a has a pending review → Needs review.
    await browser.waitUntil(async () => (await colText("in-progress")).includes("board/b"), {
      timeout: 10_000,
      timeoutMsg: "expected board/b in In progress",
    });
    // All three stage columns always render (even the empty one).
    expect(await $$('[data-testid="board-column"]').length).toBe(3);
    expect(await colText("needs-review")).toContain("board/a");

    // Decide the review (Approve) → board/a moves to the Reviewed column.
    await (await $("button*=Reviews")).click();
    await (await $('[data-testid="review-row"]')).waitForExist({ timeout: 10_000 });
    await (await $('[data-testid="review-row"]')).click();
    // Scope the action lookup to the detail element — a combined `[attr] button*=Text`
    // string is not a valid wdio selector.
    const detail = await $('[data-testid="review-detail"]');
    await detail.$("button*=Approve").click();

    await (await $("button*=Board")).click();
    await (await $("button*=Refresh")).click();
    await browser.waitUntil(async () => (await colText("reviewed")).includes("board/a"), {
      timeout: 10_000,
      timeoutMsg: "expected board/a to move to Reviewed after approval",
    });
  });

  it("compares two reviews side-by-side", async () => {
    // A second review so there are two to compare (review the second worktree).
    await (await $("button*=Coding")).click();
    const rows = await $$('[data-testid="coding-row"]');
    await rows[0].$("button*=Create review").click();

    await (await $("button*=Board")).click();
    await (await $('[data-testid="board-compare"]')).click();
    const dialog = await $('[data-testid="compare-dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });

    const picks = await $$('[data-testid="compare-pick"] input');
    await picks[0].click();
    await picks[1].click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="compare-grid"] .compare__col').length) === 2,
      { timeout: 10_000, timeoutMsg: "expected two review columns side-by-side" },
    );
    await dialog.$("button*=Close").click();
  });
});
