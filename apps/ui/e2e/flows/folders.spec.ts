import { expect, openApp, test } from "../support/app";

// «Cartelle consentite»: only in the sandboxed Mac App Store build (the demo scenario `mas`). The macOS panel and the
// permission that lasts are the shell's; here the demo stands in for them.
test.describe("allowed folders", () => {
  test("the default demo has no such panel", async ({ page }) => {
    await openApp(page, "/settings");
    await expect(page.getByRole("heading", { name: "Impostazioni", level: 1 })).toBeVisible();
    await expect(page.getByTestId("folders-panel")).toHaveCount(0);
  });

  test("the person adds and removes folders; the workspace setting is gone", async ({ page }) => {
    await openApp(page, "/settings", { scenario: "mas" });
    const panel = page.getByTestId("folders-panel");
    await expect(panel).toBeVisible();
    await expect(page.getByRole("heading", { name: "Cartelle consentite" })).toBeVisible();
    // The sandbox keeps its own folder; the setting that changes it is not offered.
    await expect(panel).toContainText("AIW-Workspace");
    await expect(page.getByText("Cartella su cui lavorano gli agenti")).toHaveCount(0);

    const list = panel.getByRole("list", { name: "Cartelle consentite" });
    await expect(list.getByRole("listitem")).toHaveCount(1);
    await expect(list).toContainText("Download");
    await expect(list).toContainText("/Users/demo/Downloads");

    await panel.getByRole("button", { name: "Aggiungi cartelle…" }).click();
    await expect(list.getByRole("listitem")).toHaveCount(2);
    await expect(list).toContainText("Documenti");

    await panel.getByRole("button", { name: "Togli «Download»" }).click();
    await expect(list.getByRole("listitem")).toHaveCount(1);
    await expect(list).not.toContainText("Download");
    await panel.getByRole("button", { name: "Togli «Documenti»" }).click();
    await expect(panel.getByTestId("folders-empty")).toContainText("Nessuna cartella consentita");
    await expect(panel.getByRole("button", { name: "Aggiungi cartelle…" })).toBeEnabled();
  });

  test("a refusal «fuori dal workspace» in a chat says where the person can allow the folder", async ({ page }) => {
    await openApp(page, "/chat", { scenario: "mas" });
    await page.getByRole("button", { name: /Guarda la cartella Download/ }).first().click();
    const hint = page.getByRole("note").filter({ hasText: "fuori dal workspace" });
    await expect(hint).toBeVisible();
    await expect(hint.getByRole("link", { name: "Impostazioni → Cartelle consentite" })).toHaveAttribute("href", "#/settings");
  });
});

test.describe("allowed folders in English", () => {
  test.use({ locale: "en-US" });
  test("the panel and its buttons follow the language", async ({ page }) => {
    await openApp(page, "/settings", { scenario: "mas" });
    const panel = page.getByTestId("folders-panel");
    await expect(page.getByRole("heading", { name: "Allowed folders" })).toBeVisible();
    await expect(panel.getByRole("button", { name: "Add folders…" })).toBeVisible();
    await expect(panel.getByRole("button", { name: "Remove “Download”" })).toBeVisible();
  });
});
