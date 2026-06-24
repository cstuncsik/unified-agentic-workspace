import { browser, $, $$, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const KEY_VALUE = "sk-ant-e2e-SDK-SECRET";
const REPO = "/tmp/fixture-repo-sdk";

const feedText = () =>
  browser.execute(
    () => document.querySelector('[data-testid="agent-sdk-feed"]')?.textContent ?? "",
  );

const allFeedsText = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="agent-sdk-feed"]')]
      .map((f) => f.textContent ?? "")
      .join("\n"),
  );

/**
 * Milestone 10b-2b slice 1: a plan-only Claude Agent SDK run via a fake Node
 * sidecar (UAW_AGENT_SDK_SIDECAR). Proves the structured feed renders, the
 * injected key is present in the sidecar env (KEY:set) but never rendered (the
 * fake deliberately echoes it; the backend masks it), and the SDK adapter
 * requires a bound account.
 */
describe("claude agent sdk (plan-only)", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (a: string[]) => execFileSync("git", ["-C", REPO, ...a], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "a@uaw.local"]);
    git(["config", "user.name", "UAW"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# sdk fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a project, repo, worktree, account", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("SdkProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("SdkFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("SdkProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("SdkFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/sdk");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });

    await (await $("button*=Providers")).click();
    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "anthropic");
    await (await $('[aria-label="Account display name"]')).setValue("SDK Acct");
    await (await $('[aria-label="API key"]')).setValue(KEY_VALUE);
    await (await $("button*=Add account")).click();
    await (await $('[data-testid="provider-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("runs a plan-only SDK session, streams the feed, never exposes the key", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");
    await (await $('[aria-label="Agent goal"]')).setValue("summarize the readme");
    await (await $("button*=New terminal")).click();

    await (await $('[data-testid="agent-sdk-feed"]')).waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await feedText()).includes("Planning:"), {
      timeout: 15_000,
      timeoutMsg: "expected the assistant row",
    });
    await browser.waitUntil(async () => (await feedText()).includes("Done"), {
      timeout: 15_000,
      timeoutMsg: "expected the result row",
    });
    // A tool row rendered.
    expect((await $$('[data-testid="sdk-event"][data-kind="tool"]')).length).toBeGreaterThan(0);
    // Injection proven (KEY:set), value redacted (the fake echoed the raw key).
    const text = await feedText();
    expect(text).toContain("KEY:set");
    expect(text).not.toContain(KEY_VALUE);
    // Plan mode never edits, so the worktree stays clean → no review affordance.
    expect((await $$('[data-testid="sdk-review-cta"]')).length).toBe(0);
    // Plan mode never edits, so completion must NOT auto-create a review. The "Done"
    // wait earlier in this test already gated on a positive completion signal, so this
    // is not racy. `sdk-review-done` renders only when a review was created (reviewed
    // flips true), so its absence is a sufficient negative — and we assert it WITHOUT
    // navigating to Reviews, because leaving + returning to Agents would remount the
    // view and reset the CLI selection the next test relies on. The edit-mode test
    // covers the positive Reviews-count assertion.
    expect((await $$('[data-testid="sdk-review-done"]')).length).toBe(0);
  });

  it("requires an account for the SDK adapter", async () => {
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("Default (no key)");
    await (await $('[aria-label="Agent goal"]')).setValue("x");
    // canStart is false without an account → the launch button is disabled.
    expect(await (await $("button*=New terminal")).isEnabled()).toBe(false);
  });

  it("creates a second worktree for an edit-mode run", async () => {
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("SdkProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("SdkFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/sdk-edit");
    await (await $("button*=Create worktree")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="coding-row"]').length) >= 2, {
      timeout: 15_000,
      timeoutMsg: "expected a second worktree row",
    });
  });

  it("edit mode changes the worktree and offers a review that persists", async () => {
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
  });

  it("lists the account's models and runs the chosen one", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");

    // The model select lazy-loads the account's models (async Node spawn).
    const modelSelect = await $('[aria-label="Agent model"]');
    await browser.waitUntil(async () => (await modelSelect.$$("option").length) > 1, {
      timeout: 15_000,
      timeoutMsg: "model select never populated",
    });
    // The default option is selected (value "") before the user picks a model.
    expect(await modelSelect.getValue()).toBe("");
    await modelSelect.selectByVisibleText("Claude Sonnet 4.5");

    await (await $('[aria-label="Agent goal"]')).setValue("summarize with sonnet");
    await (await $("button*=New terminal")).click();

    // The chosen model id reached the sidecar (argv[4]) → the fake echoed it to the feed.
    await browser.waitUntil(
      async () => (await allFeedsText()).includes("MODEL:claude-sonnet-4-5"),
      {
        timeout: 15_000,
        timeoutMsg: "expected the chosen model to reach the sidecar",
      },
    );
  });

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
});
