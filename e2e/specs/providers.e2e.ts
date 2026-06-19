import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 10b-1 end-to-end: add a provider account, see it listed (with no key
 * rendered), and remove it. The key is stored in the file-backed keystore the
 * debug binary selects via UAW_KEYSTORE_DIR — the UI only ever shows metadata.
 */
describe("provider accounts", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("adds an account and lists it without exposing the key", async () => {
    await (await $("button*=Providers")).click();

    // Select OpenAI (not the default) so the select interaction + providerLabel
    // mapping are actually exercised.
    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "openai");
    await (await $('[aria-label="Account display name"]')).setValue("My OpenAI");
    await (await $('[aria-label="API key"]')).setValue("sk-openai-e2e-SECRET-key");
    await (await $("button*=Add account")).click();

    const row = await $('[data-testid="provider-row"]');
    await row.waitForExist({ timeout: 10_000 });
    await browser.waitUntil(
      async () => (await textOf('[data-testid="provider-row"]')).includes("My OpenAI"),
      { timeout: 10_000, timeoutMsg: "expected the account row to show its name" },
    );

    // The raw key must never be rendered anywhere in the list.
    const rowText = await textOf('[data-testid="provider-row"]');
    expect(rowText).not.toContain("sk-openai-e2e-SECRET-key");
    expect(rowText).toContain("OpenAI");
  });

  it("removes the account", async () => {
    // Scope the action lookup to the row — a combined `[attr] button*=Text`
    // string is not a valid wdio selector.
    const row = await $('[data-testid="provider-row"]');
    await row.$("button*=Remove").click();

    const confirmDialog = await $('[data-testid="confirm-dialog"]');
    await confirmDialog.waitForDisplayed({ timeout: 5_000 });
    await confirmDialog.$("button*=Remove").click();

    await browser.waitUntil(async () => (await $$('[data-testid="provider-row"]').length) === 0, {
      timeout: 10_000,
      timeoutMsg: "expected the account row to be removed",
    });
  });
});
