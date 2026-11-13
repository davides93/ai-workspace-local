import { expect, openApp, test } from "../support/app";

test("devices: create an app token, copy it once, revoke it", async ({ page }) => {
  await openApp(page, "/devices", { mode: "developer" });
  const tokens = page.getByRole("list", { name: "Token per le app" });
  await expect(tokens.getByText(/^Nessun token per app\./)).toBeVisible();

  await page.getByRole("button", { name: "Nuovo token per un'app" }).click();
  const form = page.getByRole("dialog", { name: "Nuovo token per un'app" });
  const create = form.getByRole("button", { name: "Crea", exact: true });
  await expect(create).toBeDisabled();
  await form.getByRole("textbox", { name: "Nome dell'app" }).fill("Editor di prova");
  await create.click();

  // The secret is shown once, in a dialog of its own.
  const secret = page.getByRole("dialog", { name: "Copia il token adesso" });
  await expect(secret.getByText(/aiw_[A-Za-z0-9_-]{20,}/).first()).toBeVisible();
  await secret.getByRole("button", { name: "Fatto" }).click();
  await expect(secret).toBeHidden();

  const row = tokens.getByRole("listitem").filter({ hasText: "Editor di prova" });
  await expect(row).toHaveCount(1);
  await row.getByRole("button", { name: "Revoca" }).click();
  const confirm = page.getByRole("dialog", { name: "Revoca il token" });
  await confirm.getByRole("button", { name: "Revoca", exact: true }).click();
  await expect(confirm).toBeHidden();
  await expect(row.getByText("(revocato)")).toBeVisible();
  await expect(row.getByRole("button", { name: "Revoca" })).toHaveCount(0);
});
