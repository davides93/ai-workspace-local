import { expect, openApp, test } from "../support/app";

test.describe("settings", () => {
  test("language: Italian to English updates <html lang> and the texts", async ({ page }) => {
    await openApp(page, "/settings", { mode: "developer" });
    await expect(page.locator("html")).toHaveAttribute("lang", "it");
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();

    await page.getByRole("group", { name: "Lingua" }).getByRole("button", { name: "English" }).click();

    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    await expect(page.getByRole("group", { name: "Language" }).getByRole("button", { name: "English" })).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByRole("navigation", { name: "Sections" }).getByRole("link", { name: "Home" })).toBeVisible();
  });

  test("interface mode: from the chat, the menu switches to the developer view and back; the settings address redirects in simple mode", async ({ page }) => {
    // Simple mode: the chat is the only section, and the other addresses lead to it.
    await openApp(page, "/chat");
    const nav = page.getByRole("navigation", { name: "Sezioni" });
    await expect(nav.getByRole("link")).toHaveCount(0);
    await page.evaluate(() => { location.hash = "/settings"; });
    await expect(page).toHaveURL(/#\/chat(\/ses_\w+)?$/);
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeHidden();

    // The menu has the mode selector.
    await nav.getByRole("button", { name: "Altro" }).click();
    const menu = page.getByRole("dialog", { name: "Altro" });
    const mode = menu.getByRole("group", { name: "Modalità dell'interfaccia" });
    await expect(mode.getByRole("button", { name: "Semplice" })).toHaveAttribute("aria-pressed", "true");
    await mode.getByRole("button", { name: "Sviluppatore" }).click();
    // The sheet is the same one, now with the sections (it is called «Altre sezioni» then).
    const sheet = page.getByRole("dialog", { name: "Altre sezioni" });
    await expect(sheet.getByRole("group", { name: "Modalità dell'interfaccia" }).getByRole("button", { name: "Sviluppatore" })).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();

    // Developer mode: every section is there and the settings page opens.
    await expect(nav.getByRole("link", { name: "Runtime" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Sicurezza" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Attività" })).toBeVisible();
    await nav.getByRole("link", { name: "Impostazioni" }).click();
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();

    // Back to simple from the hidden view: it goes to the chat.
    const settingsMode = page.getByRole("group", { name: "Modalità dell'interfaccia" });
    await settingsMode.getByRole("button", { name: "Semplice" }).click();
    await expect(page).toHaveURL(/#\/chat(\/ses_\w+)?$/);
    await expect(nav.getByRole("link")).toHaveCount(0);
    await expect(page.getByRole("textbox", { name: "Messaggio" })).toBeVisible();
  });

  test("backup: create it and download the archive", async ({ page }) => {
    await openApp(page, "/settings", { mode: "developer" });
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Crea e scarica il backup" }).click();
    const file = await download;
    expect(file.suggestedFilename()).toBe("aiw-backup-demo.tar.gz");
    await expect(page.getByText(/Backup creato/)).toBeVisible();
  });
});
