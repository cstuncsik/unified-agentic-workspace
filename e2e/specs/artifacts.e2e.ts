import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 4 end-to-end: create/edit/reopen markdown artifacts, verify the
 * sanitized preview (rendered heading + no script / no javascript: href), the
 * dirty-guard discard-on-switch, deletion, and project-detach (SET NULL).
 */
describe("markdown artifacts", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("creates an artifact (Create disabled until titled)", async () => {
    await (await $("button*=Artifacts")).click();
    const create = await $("button*=Create");
    expect(await create.isEnabled()).toBe(false);
    await (await $('[aria-label="New artifact title"]')).setValue("Research notes");
    expect(await create.isEnabled()).toBe(true);
    await create.click();
    await (await $('[data-testid="artifact-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("edits, shows the Unsaved indicator, saves, and renders a sanitized preview", async () => {
    const editor = await $('[data-testid="artifact-editor"]');
    await editor.waitForExist({ timeout: 10_000 });
    const source = await $('[aria-label="Markdown source"]');
    await source.setValue("# Hello\n\n[click](javascript:alert(1))\n\n<script>alert(2)</script>\n");

    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    const save = await editor.$("button*=Save");
    expect(await save.isEnabled()).toBe(true);
    await save.click();
    await browser.waitUntil(async () => !(await $('[data-testid="artifact-dirty"]').isExisting()), {
      timeout: 10_000,
      timeoutMsg: "expected the Unsaved indicator to clear after save",
    });

    // Preview: heading renders, and the XSS payloads are neutralized.
    await (await editor.$("span*=Preview")).click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="artifact-preview"] h1')).includes("Hello"),
      { timeout: 10_000, timeoutMsg: "expected the rendered <h1>Hello" },
    );
    const safe = await browser.execute(() => {
      const root = document.querySelector('[data-testid="artifact-preview"]');
      if (!root) return false;
      const noScript = root.querySelector("script") === null;
      const a = root.querySelector("a");
      const noJs = !a || !(a.getAttribute("href") ?? "").startsWith("javascript:");
      return noScript && noJs;
    });
    expect(safe).toBe(true);
  });

  it("guards unsaved edits when switching artifacts", async () => {
    // A second artifact to switch to.
    await (await $('[aria-label="New artifact title"]')).setValue("Second");
    await (await $("button*=Create")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="artifact-row"]').length) === 2, {
      timeout: 10_000,
    });

    // Make the (now-selected) Second artifact dirty, then try to switch away.
    await (await $('[aria-label="Markdown source"]')).setValue("dirty edit");
    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    await (await $$('[data-testid="artifact-row"]'))[1].click();

    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    // Cancel keeps us on the dirty artifact.
    await dialog.$("button*=Cancel").click();
    expect(await $('[data-testid="artifact-dirty"]').isExisting()).toBe(true);

    // Confirm discards and switches.
    await (await $$('[data-testid="artifact-row"]'))[1].click();
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Discard").click();
    await browser.waitUntil(async () => !(await $('[data-testid="artifact-dirty"]').isExisting()), {
      timeout: 10_000,
      timeoutMsg: "expected a clean editor after discarding",
    });
  });

  it("deletes an artifact", async () => {
    const before = await $$('[data-testid="artifact-row"]').length;
    // Scope the lookup to the editor element — a combined `[attr] button*=Text`
    // string is not a valid wdio selector.
    const editor = await $('[data-testid="artifact-editor"]');
    await editor.$("button*=Delete").click();
    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Delete").click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="artifact-row"]').length) === before - 1,
      {
        timeout: 10_000,
        timeoutMsg: "expected one fewer artifact row after delete",
      },
    );
  });

  it("detaches the project badge from artifacts when the project is deleted", async () => {
    // Create a project to scope an artifact to.
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("Detachable");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    // Create a project-scoped artifact; its row shows the project badge.
    await (await $("button*=Artifacts")).click();
    await (await $('[aria-label="New artifact title"]')).setValue("Scoped doc");
    await (await $('[aria-label="Artifact project"]')).selectByVisibleText("Detachable");
    await (await $("button*=Create")).click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="artifact-row"] .re-badge').length) >= 1,
      {
        timeout: 10_000,
        timeoutMsg: "expected the scoped artifact to show a project badge",
      },
    );

    // Delete the project (it is the only one in this spec's workspace).
    await (await $("button*=Projects")).click();
    const projectRow = await $('[data-testid="project-row"]');
    await projectRow.$("button*=Delete").click();
    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Delete").click();

    // Back in Artifacts the badge is gone (SET NULL surfaced live via detachProject).
    await (await $("button*=Artifacts")).click();
    await browser.waitUntil(
      async () => (await $$('[data-testid="artifact-row"] .re-badge').length) === 0,
      {
        timeout: 10_000,
        timeoutMsg: "expected the project badge to disappear after the project is deleted",
      },
    );
  });
});
