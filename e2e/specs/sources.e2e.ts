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

  it("attaches a fixture repo, shows its status, and detaches it", async () => {
    await (await $('[aria-label="Repository name"]')).setValue("Fixture");
    await (await $('[aria-label="Repository path"]')).setValue("/tmp/fixture-repo");
    await (await $("button*=Attach")).click();

    const row = await $('[data-testid="repository-row"]');
    await row.waitForExist({ timeout: 10_000 });
    // Live status from get_repository_status — also asserts the snake_case
    // GitInspection fields deserialize correctly across the Tauri boundary.
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(
            () =>
              document.querySelector('[data-testid="repository-row"] .repo__status')?.textContent ??
              "",
          )
        ).includes("clean"),
      { timeout: 10_000, timeoutMsg: "expected the fixture repo to report a clean status" },
    );

    await row.$("button*=Detach").click();
    await $(".re-dialog").$("button*=Delete").click();
    await browser.waitUntil(async () => (await $$('[data-testid="repository-row"]').length) === 0, {
      timeout: 10_000,
      timeoutMsg: "expected the repository row to be removed after detach",
    });
  });
});
