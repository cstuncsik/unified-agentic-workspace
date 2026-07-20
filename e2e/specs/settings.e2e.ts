import { browser, $, expect } from "@wdio/globals";
import * as fs from "node:fs";

const CONFIG = process.env.UAW_CONFIG_PATH as string;

/**
 * Config Settings Page (Slice ②) end-to-end: saving the form must MERGE into
 * config.json (services/config.rs::merge_edits) — a hand-edited theme and any
 * unknown top-level key survive untouched, and the args textarea's newline
 * split/blank-drop/space-in-arg-intact contract holds at the file level.
 * SettingsView is app-global (rendered whenever activeView === 'settings',
 * independent of any workspace), so this needs no project/repo fixture.
 */
describe("settings", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("saves agent args + fontSize, preserving hand-edited theme + unknown keys", async () => {
    fs.writeFileSync(
      CONFIG,
      JSON.stringify({ keepMe: "yes", terminal: { theme: { background: "#0a0a12" } } }),
    );

    await (await $("button*=Settings")).click();
    await (await $('[data-testid="settings-view"]')).waitForExist({ timeout: 10_000 });
    // The form renders only once getForEdit() resolves (the `loaded` gate).
    await (await $('[data-testid="args-claude-code"]')).waitForExist({ timeout: 10_000 });

    // Set the textarea via the DOM + a real "input" event: wdio's setValue treats
    // "\n" as Enter and can collapse blank lines, which would hide the very
    // blank-line-drop behaviour this test needs to prove (see dispatch.e2e.ts).
    await browser.execute((val) => {
      const ta = document.querySelector(
        '[data-testid="args-claude-code"]',
      ) as HTMLTextAreaElement | null;
      if (ta) {
        ta.value = val;
        ta.dispatchEvent(new Event("input", { bubbles: true }));
      }
    }, "--uaw-set\n\n--msg hello there");
    await (await $('[data-testid="font-size"]')).setValue("16");

    // A lingering toast can sit over the Save button and intercept a native
    // click; dispatch straight to the element's handler instead.
    await browser.execute(() =>
      (document.querySelector('[data-testid="settings-save"]') as HTMLButtonElement)?.click(),
    );

    await browser.waitUntil(
      () => {
        try {
          const c = JSON.parse(fs.readFileSync(CONFIG, "utf8"));
          return (
            c.keepMe === "yes" &&
            c.terminal?.theme?.background === "#0a0a12" &&
            c.terminal?.fontSize === 16 &&
            Array.isArray(c.agents?.["claude-code"]?.args)
          );
        } catch {
          return false;
        }
      },
      { timeout: 10_000, timeoutMsg: "merged config not written with preserved keys" },
    );

    const cfg = JSON.parse(fs.readFileSync(CONFIG, "utf8"));
    expect(cfg.agents["claude-code"].args).toEqual(["--uaw-set", "--msg hello there"]);
    expect(cfg.keepMe).toBe("yes"); // unknown top-level key preserved
    expect(cfg.terminal.theme.background).toBe("#0a0a12"); // hand-edited theme preserved
  });
});
