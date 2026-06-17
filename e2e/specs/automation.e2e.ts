import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-auto";

/**
 * Milestone 9 end-to-end: configure a project test command, create a worktree,
 * make a change, click "Complete and review", and verify the workspace lands in
 * Needs Review with a persisted review showing the captured check output and a
 * "Checks failed" risk flag. Uses its own fixture repo to stay isolated.
 */
describe("completion automation", () => {
  before(async () => {
    // Fresh, isolated fixture repo so adding a worktree never races coding.e2e.
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (args: string[]) => execFileSync("git", ["-C", REPO, ...args], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "auto@uaw.local"]);
    git(["config", "user.name", "UAW Auto"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# auto fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project with a test command and an attached repo", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AutoProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    // Configure the check command (prints a marker, then fails).
    const cmd = await $('[aria-label="Test command for AutoProj"]');
    await cmd.setValue("echo myCheck; exit 1");
    await browser.keys("Enter");

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AutoFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("creates a worktree from the repo", async () => {
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AutoProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AutoFixture");

    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), {
      timeout: 10_000,
      timeoutMsg: "base branch select never populated",
    });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/auto");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });
  });

  it("completes the worktree: runs checks, lands in Needs Review with output + failure flag", async () => {
    const row = await $('[data-testid="coding-row"]');

    // Make a change so the review has content.
    const worktreePath = (await textOf('[data-testid="coding-row"] .coding__path')).trim();
    fs.writeFileSync(path.join(worktreePath, "change.txt"), "work\n");

    await row.$("button*=Complete and review").click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="coding-row"] .re-badge')).includes("needs-review"),
      { timeout: 30_000, timeoutMsg: "expected the worktree to move to needs-review" },
    );

    // The resulting review shows the captured check output and the failure flag.
    await (await $("button*=Reviews")).click();
    const reviewRow = await $('[data-testid="review-row"]');
    await reviewRow.waitForExist({ timeout: 10_000 });
    await reviewRow.click();

    await browser.waitUntil(
      async () => (await textOf('[data-testid="review-detail"]')).includes("myCheck"),
      { timeout: 10_000, timeoutMsg: "expected the check output (myCheck) in the review" },
    );
    expect(await textOf('[data-testid="review-detail"]')).toContain("Checks failed");
  });
});
