import { expect, openApp, test } from "../support/app";

// The guided check of Docker, from the simple interface's menu. The demo answers «Docker installed, not running».

test("the menu has «Un computer per i tuoi agenti» and it says what to do, and installs nothing", async ({ page }) => {
  await openApp(page, "/chat");
  await page.getByRole("navigation", { name: "Sezioni" }).getByRole("button", { name: "Altro" }).click();
  await page.getByRole("button", { name: "Un computer per i tuoi agenti" }).click();

  const dialog = page.getByRole("dialog", { name: "Un computer per i tuoi agenti" });
  await expect(dialog.getByRole("heading", { name: "Docker è installato ma spento" })).toBeVisible();
  await expect(dialog.getByText("Apri l'app Docker Desktop")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Controlla di nuovo" })).toBeEnabled();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});
