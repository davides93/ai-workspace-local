import { expect, openApp, test } from "../support/app";

// First start of the simple interface: pick the recommended model, watch it download (simulated),
// then land in the first chat and get a streamed answer.
test("first run: recommended model, simulated download, first streamed chat", async ({ page }) => {
  await openApp(page, "/", { scenario: "first-run" });

  const panel = page.getByRole("heading", { name: "Per iniziare" });
  await expect(panel).toBeVisible();
  await expect(page.getByText(/Ti consiglio .* il modello più adatto per Studio-PC/)).toBeVisible();

  await page.getByRole("button", { name: "Prepara tutto" }).click();

  // The progress is shown as a meter that moves, and the button is locked while the download runs.
  const meter = page.getByRole("meter", { name: /^Avanzamento di / });
  await expect(meter).toBeVisible();
  await expect(page.getByRole("button", { name: "Prepara tutto" })).toBeDisabled();
  const now = async () => Number(await meter.getAttribute("aria-valuenow"));
  const first = await now();
  await expect.poll(now).toBeGreaterThan(first);

  // When the model is on disk the app opens the first conversation by itself.
  await expect(page).toHaveURL(/#\/chat\/ses_/, { timeout: 40_000 });
  await expect(page.getByRole("heading", { name: "Cosa facciamo, in locale?" })).toBeVisible();

  const box = page.getByRole("textbox", { name: "Messaggio" });
  await box.fill("Ciao! Cosa sai fare?");
  await page.getByRole("button", { name: "Invia" }).click();

  // The answer streams in: the stop button is there while it is written, the text shows up in the log.
  const log = page.getByRole("log", { name: "Conversazione" });
  await expect(page.getByRole("button", { name: "Ferma" })).toBeVisible();
  await expect(log.getByText("Ciao! Cosa sai fare?")).toBeVisible();
  await expect(log.getByText("Inizio guardando cosa c'è nella cartella Downloads.")).toBeVisible();
});
