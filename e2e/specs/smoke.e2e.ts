import { browser, $, $$, expect } from "@wdio/globals";

/**
 * Full-stack smoke test: drives the real built UAW app through tauri-driver,
 * exercising the real Rust commands and SQLite. Covers the Milestone 2 core
 * loop (workspace -> project -> session -> status) end to end.
 *
 * Tests share one app instance and one database, so they run in order and build
 * on each other's state.
 */

/**
 * Read an element's text via textContent. WebKit's WebDriver getElementText
 * (what `toHaveText` uses) returns "" for elements styled with
 * overflow:hidden + text-overflow:ellipsis, even when the text is fully
 * rendered — so for those we read textContent directly and poll until it
 * settles (rows render asynchronously after a create).
 */
async function expectText(selector: string, expected: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      (await browser.execute(
        (sel) => document.querySelector(sel)?.textContent?.trim() ?? "",
        selector,
      )) === expected,
    { timeout: 10_000, timeoutMsg: `expected "${selector}" to have text "${expected}"` },
  );
}

describe("UAW core loop", () => {
  before(async () => {
    // Give the headless window a known size so layout is deterministic.
    await browser.setWindowSize(1280, 900);
  });

  it("boots with the auto-created default workspace", async () => {
    const heading = await $("h1");
    await heading.waitForExist({ timeout: 30_000 });
    await expect(heading).toHaveText("Default");
  });

  it("creates a project", async () => {
    await (await $("button*=Projects")).click();

    await (await $('[aria-label="New project name"]')).setValue("Alpha");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();

    await expect($$('[data-testid="project-row"]')).toBeElementsArrayOfSize(1);
    await expectText('[data-testid="project-row"] .row__title', "Alpha");
  });

  it("creates a session attached to that project", async () => {
    await (await $("button*=Inbox")).click();

    await (await $('[aria-label="New session title"]')).setValue("First task");
    await (await $('[aria-label="Session mode"]')).selectByAttribute("value", "code");
    await (await $('[aria-label="Session project"]')).selectByVisibleText("Alpha");
    await (await $("button*=Create")).click();

    await (await $('[data-testid="session-row"]')).waitForExist({ timeout: 10_000 });
    await expectText('[data-testid="session-row"] .row__title', "First task");
    // The originating project name is rendered on the session row.
    await expectText('[data-testid="session-row"] .row__project', "Alpha");
  });

  it("moves the session through the status workflow", async () => {
    const status = await $('[aria-label="Session status"]');
    await status.selectByAttribute("value", "needs-review");
    await expect(status).toHaveValue("needs-review");

    await status.selectByAttribute("value", "done");
    await expect(status).toHaveValue("done");

    // The row regroups under the Done heading.
    await expect($("h3*=Done")).toBeDisplayed();
  });

  it("isolates data per workspace", async () => {
    await (await $('[aria-label="New workspace"]')).click();
    await (await $('[aria-label="New workspace name"]')).setValue("Client");
    await (await $('button[title="Create"]')).click();

    // The new workspace becomes current and starts empty.
    await expect($("h1")).toHaveText("Client");
    await (await $("button*=Inbox")).click();
    await expect($$('[data-testid="session-row"]')).toBeElementsArrayOfSize(0);

    // Switching back to Default still shows the original session.
    await (await $('[aria-label="Select workspace"]')).selectByVisibleText("Default");
    await expect($("h1")).toHaveText("Default");
    await expectText('[data-testid="session-row"] .row__title', "First task");
  });
});
