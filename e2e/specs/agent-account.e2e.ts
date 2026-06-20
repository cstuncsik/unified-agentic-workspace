import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const KEY_VALUE = "sk-ant-e2e-SECRET-do-not-print";
const REPO = "/tmp/fixture-repo-acct";

// Text of the currently-visible terminal (multiple stay mounted via v-show).
const visibleTermText = () =>
  browser.execute(() => {
    const terms = Array.from(
      document.querySelectorAll('[data-testid="agent-terminal"]'),
    ) as HTMLElement[];
    const vis = terms.find((t) => t.offsetParent !== null) ?? terms[terms.length - 1];
    return vis ? (vis.textContent ?? "") : "";
  });

const accountOptionTexts = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll('[aria-label="Provider account"] option')).map((o) =>
      (o.textContent ?? "").trim(),
    ),
  );

/**
 * Milestone 10b-2a: bind a provider account to an agent terminal and inject its
 * key into the PTY env. The fake agent prints KEY:set / KEY:unset (boolean, never
 * the value), so injection is proven without ever exposing the key.
 */
describe("agent account injection", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (a: string[]) => execFileSync("git", ["-C", REPO, ...a], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "a@uaw.local"]);
    git(["config", "user.name", "UAW"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# acct fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project, repo, worktree, and an Anthropic account", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AcctProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AcctFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AcctProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AcctFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/acct");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });

    await (await $("button*=Providers")).click();
    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "anthropic");
    await (await $('[aria-label="Account display name"]')).setValue("My Anthropic");
    await (await $('[aria-label="API key"]')).setValue(KEY_VALUE);
    await (await $("button*=Add account")).click();
    await (await $('[data-testid="provider-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("injects the bound account key into the terminal env (never the value)", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/acct");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("My Anthropic");
    await (await $("button*=New terminal")).click();

    await (await $('[data-testid="agent-terminal"]')).waitForExist({ timeout: 10_000 });
    await browser.waitUntil(async () => (await visibleTermText()).includes("KEY:set"), {
      timeout: 15_000,
      timeoutMsg: "expected KEY:set (the injected account key reached the agent env)",
    });
    // The raw key value must NEVER appear in the terminal/transcript.
    expect(await visibleTermText()).not.toContain(KEY_VALUE);
  });

  it("filters accounts by adapter and omits the key when none is selected", async () => {
    // Codex (openai) must NOT offer the anthropic account.
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Codex");
    expect(await accountOptionTexts()).not.toContain("My Anthropic");

    // Claude Code offers it; pick Default (no key) and launch -> KEY:unset.
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Code");
    expect(await accountOptionTexts()).toContain("My Anthropic");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("Default (no key)");
    await (await $("button*=New terminal")).click();

    await browser.waitUntil(async () => (await visibleTermText()).includes("KEY:unset"), {
      timeout: 15_000,
      timeoutMsg: "expected KEY:unset for a Default (no account) session",
    });
  });
});
