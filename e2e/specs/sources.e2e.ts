import { browser, $, $$, expect } from "@wdio/globals";

/**
 * Milestone 6 smoke: the Sources view drives the real validate_repository_path
 * command (which shells out to git) end to end in the WebKit app.
 */
describe("sources", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("opens the Sources view with the git repositories section", async () => {
    await (await $("button*=Sources")).click();
    await expect($("h3*=Git Repositories")).toBeDisplayed();
    await expect($$('[data-testid="repository-row"]')).toBeElementsArrayOfSize(0);
  });

  it("validates a path and reports a non-git folder", async () => {
    await (await $('[aria-label="Repository path"]')).setValue("/definitely/not/a/repo");
    await (await $("button*=Validate")).click();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() => document.querySelector(".preview")?.textContent ?? "")
        ).includes("✗"),
      { timeout: 10_000, timeoutMsg: "expected a validation error in the preview" },
    );
  });
});
