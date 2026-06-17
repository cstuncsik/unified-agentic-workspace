import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-agent";

/**
 * Milestone 10a end-to-end: open an interactive agent terminal (a fake CLI
 * injected via UAW_AGENT_BIN that prints a banner then echoes stdin) against a
 * worktree, and verify the PTY/xterm round-trip — banner renders, typed input is
 * echoed back, and the session can be stopped. Uses its own fixture repo.
 */
describe("agent terminals", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (args: string[]) => execFileSync("git", ["-C", REPO, ...args], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "agent@uaw.local"]);
    git(["config", "user.name", "UAW Agent"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# agent fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project + repo + worktree", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AgentProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AgentFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AgentProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AgentFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/agent");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });
  });

  it("opens a terminal, renders the agent banner, echoes input, and stops", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/agent");
    // The CLI defaults to the first adapter (Claude Code); UAW_AGENT_BIN makes it
    // run our fake regardless.
    await (await $("button*=New terminal")).click();

    const term = await $('[data-testid="agent-terminal"]');
    await term.waitForExist({ timeout: 10_000 });

    // The fake agent prints AGENT-READY into the PTY → xterm renders it.
    await browser.waitUntil(
      async () => (await textOf('[data-testid="agent-terminal"]')).includes("AGENT-READY"),
      { timeout: 15_000, timeoutMsg: "expected the agent banner to render in the terminal" },
    );

    // Type into the terminal; the fake echoes it back through the PTY.
    await term.click();
    await browser.keys("ping-uaw");
    await browser.keys("Enter");
    await browser.waitUntil(
      async () => (await textOf('[data-testid="agent-terminal"]')).includes("ping-uaw"),
      { timeout: 15_000, timeoutMsg: "expected typed input to be echoed in the terminal" },
    );

    // Stop the session; the tab status reflects a terminal state.
    await (await $("button*=Stop")).click();
    await browser.waitUntil(
      async () => {
        const t = (await textOf('[data-testid="agent-tab"]')).toLowerCase();
        return t.includes("stopped") || t.includes("exited") || t.includes("failed");
      },
      { timeout: 15_000, timeoutMsg: "expected the session to reach a terminal status" },
    );
  });
});
