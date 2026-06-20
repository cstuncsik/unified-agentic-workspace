import { browser, $, $$, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const KEY_VALUE = "sk-ant-e2e-SDK-SECRET";
const REPO = "/tmp/fixture-repo-sdk";

const feedText = () =>
  browser.execute(() => document.querySelector('[data-testid="agent-sdk-feed"]')?.textContent ?? "");

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
  });

  it("requires an account for the SDK adapter", async () => {
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("Default (no key)");
    await (await $('[aria-label="Agent goal"]')).setValue("x");
    // canStart is false without an account → the launch button is disabled.
    expect(await (await $("button*=New terminal")).isEnabled()).toBe(false);
  });
});
