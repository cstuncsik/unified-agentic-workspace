import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 7 end-to-end: from a code project + attached repo, create a git
 * worktree, inspect its (clean) diff, mark it ready for review, and discard it.
 * Self-contained — runs against this spec's own fresh database (see wdio.conf).
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

  it("shows a clean diff, marks ready, then discards", async () => {
    const row = await $('[data-testid="coding-row"]');

    await row.$("button*=View diff").click();
    await browser.waitUntil(async () => (await textOf(".coding__diff")).includes("No changes"), {
      timeout: 10_000,
      timeoutMsg: "expected a clean-worktree diff",
    });

    await row.$("button*=Mark ready").click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="coding-row"] .re-badge')).includes("needs-review"),
      { timeout: 10_000, timeoutMsg: "expected status to become needs-review" },
    );

    await row.$("button*=Discard").click();
    await $(".re-dialog").$("button*=Delete").click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]').length) === 0, {
      timeout: 10_000,
      timeoutMsg: "expected the worktree row to be removed after discard",
    });
  });
});
