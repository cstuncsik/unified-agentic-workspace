import { browser, $, $$, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 7 end-to-end: from a code project + attached repo, create a git
 * worktree, review its diff (clean, then with an untracked change), mark it
 * ready, and discard it. Self-contained — runs against this spec's own fresh
 * database and worktrees dir (see wdio.conf), using the run-e2e.sh fixture repo.
 */
describe("coding workspaces", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("creates the prerequisites: a code project and an attached repo", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("CodeProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("Fixture");
    await (await $('[aria-label="Repository path"]')).setValue("/tmp/fixture-repo");
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("creates a worktree from the repo", async () => {
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("CodeProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("Fixture");

    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), {
      timeout: 10_000,
      timeoutMsg: "base branch select never populated",
    });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/e2e");
    await (await $("button*=Create worktree")).click();

    const row = await $('[data-testid="coding-row"]');
    await row.waitForExist({ timeout: 15_000 });
    await browser.waitUntil(
      async () => (await textOf('[data-testid="coding-row"] .coding__branch')).includes("feat/e2e"),
      { timeout: 10_000, timeoutMsg: "expected the new worktree branch to appear" },
    );
  });

  it("reviews a clean diff, then surfaces an untracked file", async () => {
    const row = await $('[data-testid="coding-row"]');

    await row.$("button*=View diff").click();
    await browser.waitUntil(async () => (await textOf(".coding__diff")).includes("No changes"), {
      timeout: 10_000,
      timeoutMsg: "expected a clean-worktree diff",
    });
    await row.$("button*=Hide diff").click();

    // Write an untracked file straight into the worktree on disk (the wdio worker
    // shares the filesystem with the app), then re-open the diff.
    const worktreePath = (await textOf('[data-testid="coding-row"] .coding__path')).trim();
    fs.writeFileSync(path.join(worktreePath, "untracked.txt"), "new file\n");

    await row.$("button*=View diff").click();
    await browser.waitUntil(async () => (await textOf(".coding__diff")).includes("untracked.txt"), {
      timeout: 10_000,
      timeoutMsg: "expected the untracked file to be listed in the diff",
    });
  });

  it("creates a review for the worktree and approves it", async () => {
    const row = await $('[data-testid="coding-row"]');

    // Delete the committed README in the worktree so the review has a deterministic
    // risk flag ("Files deleted") to assert on (the worktree already has an
    // untracked file from the previous test).
    const worktreePath = (await textOf('[data-testid="coding-row"] .coding__path')).trim();
    fs.rmSync(path.join(worktreePath, "README.md"));

    await row.$("button*=Create review").click();

    // Go to the Reviews view; a pending review should be listed.
    await (await $("button*=Reviews")).click();
    const reviewRow = await $('[data-testid="review-row"]');
    await reviewRow.waitForExist({ timeout: 10_000 });
    await reviewRow.click();

    // The detail panel shows the deleted-file risk flag and the untracked file.
    await browser.waitUntil(
      async () => (await textOf('[data-testid="review-detail"]')).includes("Files deleted"),
      { timeout: 10_000, timeoutMsg: "expected a 'Files deleted' risk note" },
    );
    expect(await textOf('[data-testid="review-detail"]')).toContain("untracked.txt");

    // Approve it; the status badge updates. Scope the button lookup to the detail
    // element — a combined `[attr] button*=Text` string isn't a valid wdio selector.
    const detail = await $('[data-testid="review-detail"]');
    await detail.$("button*=Approve").click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="review-row"] .re-badge')).includes("approved"),
      { timeout: 10_000, timeoutMsg: "expected the review status to become approved" },
    );

    // Return to Coding for the discard test that follows.
    await (await $("button*=Coding")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("marks ready, then discards the dirty worktree", async () => {
    const row = await $('[data-testid="coding-row"]');

    await row.$("button*=Mark ready").click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="coding-row"] .re-badge')).includes("needs-review"),
      { timeout: 10_000, timeoutMsg: "expected status to become needs-review" },
    );

    await row.$("button*=Discard").click();
    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Discard").click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]').length) === 0, {
      timeout: 10_000,
      timeoutMsg: "expected the worktree row to be removed after discard",
    });
  });
});
