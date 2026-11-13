import { expect, openApp, test } from "../support/app";

test.describe("settings", () => {
  test("language: Italian to English updates <html lang> and the texts", async ({ page }) => {
    await openApp(page, "/settings");
    await expect(page.locator("html")).toHaveAttribute("lang", "it");
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();

    await page.getByRole("group", { name: "Lingua" }).getByRole("button", { name: "English" }).click();

    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
    await expect(page.getByRole("group", { name: "Language" }).getByRole("button", { name: "English" })).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByRole("navigation", { name: "Sections" }).getByRole("link", { name: "Home" })).toBeVisible();
  });

  test("interface mode: the developer view shows the sections the simple one hides", async ({ page }) => {
    await openApp(page, "/settings");
    const nav = page.getByRole("navigation", { name: "Sezioni" });
    await expect(nav.getByRole("link", { name: "Runtime" })).toBeHidden();
    await expect(nav.getByRole("link", { name: "Sicurezza" })).toBeHidden();

    const mode = page.getByRole("group", { name: "Modalità dell'interfaccia" });
    await expect(mode.getByRole("button", { name: "Semplice" })).toHaveAttribute("aria-pressed", "true");
    await mode.getByRole("button", { name: "Sviluppatore" }).click();

    await expect(mode.getByRole("button", { name: "Sviluppatore" })).toHaveAttribute("aria-pressed", "true");
    await expect(nav.getByRole("link", { name: "Runtime" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Sicurezza" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Attività" })).toBeVisible();
    // Back to simple: the sections go away again.
    await mode.getByRole("button", { name: "Semplice" }).click();
    await expect(nav.getByRole("link", { name: "Runtime" })).toBeHidden();
  });

  test("backup: create it and download the archive", async ({ page }) => {
    await openApp(page, "/settings");
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Crea e scarica il backup" }).click();
    const file = await download;
    expect(file.suggestedFilename()).toBe("aiw-backup-demo.tar.gz");
    await expect(page.getByText(/Backup creato/)).toBeVisible();
  });
});
