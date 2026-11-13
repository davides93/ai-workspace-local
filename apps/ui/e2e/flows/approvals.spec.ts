import { expect, openApp, test } from "../support/app";

// The demo agent asks before it moves files: one request is approved from the banner's drawer,
// the next one is denied from the card in the conversation.
test("approvals: one request approved, one denied", async ({ page }) => {
  await openApp(page, "/chat");
  const box = page.getByRole("textbox", { name: "Messaggio" });
  const log = page.getByRole("log", { name: "Conversazione" });
  await page.getByRole("button", { name: "Nuova", exact: true }).click();
  // The new session resets the composer: wait for it before typing, or the text can be wiped.
  await expect(page.getByRole("heading", { name: "Cosa facciamo, in locale?" })).toBeVisible();

  // 1. Approved, from the global drawer.
  await box.fill("Ordina i PDF in Downloads");
  await page.getByRole("button", { name: "Invia" }).click();
  const banner = page.getByRole("button", { name: /1 conferma in attesa/ });
  await expect(banner).toBeVisible();
  await banner.click();
  const drawer = page.getByRole("dialog", { name: "Conferme in attesa (1)" });
  await expect(drawer.getByText("Il comando sposta file nella cartella Downloads.")).toBeVisible();
  await drawer.getByRole("button", { name: "Consenti", exact: true }).click();
  await expect(banner).toBeHidden();
  await expect(log.getByText("Fatto. I PDF sono ordinati per anno")).toBeVisible();
  await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();

  // 2. Denied, from the card of the conversation.
  await box.fill("Riordinali di nuovo");
  await page.getByRole("button", { name: "Invia" }).click();
  const card = log.getByRole("article", { name: "shell" });
  await expect(card.getByText("Serve la tua conferma")).toBeVisible();
  await card.getByRole("button", { name: "Nega", exact: true }).click();
  await expect(log.getByText("Ok, non sposto nulla.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();

  // Both decisions are in the activity log.
  await page.getByRole("button", { name: "Mostra tutte le sezioni" }).click();
  await page.getByRole("link", { name: "Attività" }).click();
  const audit = page.getByRole("table", { name: "Audit trail" });
  await expect(audit.getByRole("row", { name: /Utente approval:shell Approvato/ })).toHaveCount(1);
  await expect(audit.getByRole("row", { name: /Utente approval:shell Rifiutato/ })).toHaveCount(1);
});
