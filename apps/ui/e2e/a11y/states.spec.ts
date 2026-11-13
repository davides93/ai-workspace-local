import { expect, openApp, test } from "../support/app";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// Screens that only exist after an interaction (overlays, a running chat, the first run) and the
// English interface, scanned with axe-core in both themes at phone and desktop width.
const SIZES = [
  { name: "390", viewport: { width: 390, height: 844 } },
  { name: "1440", viewport: { width: 1440, height: 900 } },
] as const;

for (const size of SIZES) {
  for (const theme of ["light", "dark"] as const) {
    test.describe(`${size.name} · ${theme}`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });

      test("first run of the simple interface", async ({ page }, info) => {
        await openApp(page, "/", { theme, scenario: "first-run" });
        await expect(page.getByRole("button", { name: "Prepara tutto" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("first run while the model downloads", async ({ page }, info) => {
        await openApp(page, "/", { theme, scenario: "first-run" });
        await page.getByRole("button", { name: "Prepara tutto" }).click();
        await expect(page.getByRole("meter", { name: /^Avanzamento di / })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("command palette", async ({ page }, info) => {
        await openApp(page, "/", { theme });
        await page.keyboard.press("Control+k");
        await expect(page.getByRole("dialog", { name: "Cerca e comandi" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("chat waiting for an approval, and the approvals drawer", async ({ page }, info) => {
        await openApp(page, "/chat/ses_pdf", { theme });
        await page.getByRole("textbox", { name: "Messaggio" }).fill("Ordina i PDF in Downloads");
        await page.getByRole("button", { name: "Invia" }).click();
        const banner = page.getByRole("button", { name: /1 conferma in attesa/ });
        await expect(banner).toBeVisible();
        await expect(page.getByRole("log", { name: "Conversazione" }).getByRole("article", { name: "shell" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await banner.click();
        await expect(page.getByRole("dialog", { name: "Conferme in attesa (1)" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("new token dialog and the token shown once", async ({ page }, info) => {
        await openApp(page, "/devices", { theme, mode: "developer" });
        await page.getByRole("button", { name: "Nuovo token per un'app" }).click();
        const form = page.getByRole("dialog", { name: "Nuovo token per un'app" });
        await expectNoBlockingViolations(page, info);
        await form.getByRole("textbox", { name: "Nome dell'app" }).fill("Editor");
        await form.getByRole("button", { name: "Crea", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Copia il token adesso" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("documents with a search result", async ({ page }, info) => {
        await openApp(page, "/documents", { theme });
        const search = page.getByRole("region", { name: "Cerca nei documenti" });
        await search.getByRole("searchbox", { name: "Cosa cerchi?" }).fill("canone");
        await search.getByRole("button", { name: "Cerca", exact: true }).click();
        await expect(search.getByText("contratto-affitto.txt").first()).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

    });

    test.describe(`${size.name} · ${theme} · English`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme, locale: "en-US" });
      for (const route of ["/", "/chat/ses_pdf", "/models", "/settings", "/documents", "/devices"]) {
        test(route, async ({ page }, info) => {
          await openApp(page, route, { theme });
          await expect(page.locator("html")).toHaveAttribute("lang", "en");
          await expectNoBlockingViolations(page, info);
          await expectNoPlaceholderLeaks(page);
        });
      }
    });
  }
}
