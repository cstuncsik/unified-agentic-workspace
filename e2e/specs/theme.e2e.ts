import { browser, $, expect } from "@wdio/globals";

const rootClass = () => browser.execute(() => document.documentElement.className.trim());
const stored = () => browser.execute(() => localStorage.getItem("uaw.theme"));

describe("theme toggle", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
  });

  it("switches Light / Dark / System and persists the choice", async () => {
    const select = await $('[aria-label="Theme"]');

    await select.selectByAttribute("value", "light");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent-light");
    expect(await stored()).toBe("light");

    await select.selectByAttribute("value", "dark");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent-dark");
    expect(await stored()).toBe("dark");

    await select.selectByAttribute("value", "system");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent");
    expect(await stored()).toBe("system");
  });
});
