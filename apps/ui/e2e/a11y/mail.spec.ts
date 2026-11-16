import { expect, openApp, test } from "../support/app";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// The page of the channels with a linked conversation and drafts in every state, with its forms and dialogs open, at phone
// and desktop width, in both themes: axe-core, no leaked placeholder, no horizontal scroll, touch targets on the phone.
const SIZES = [
  { name: "390", viewport: { width: 390, height: 844 } },
  { name: "1440", viewport: { width: 1440, height: 900 } },
] as const;

for (const size of SIZES) {
  for (const theme of ["light", "dark"] as const) {
    test.describe(`${size.name} · ${theme}`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });

      test("channels with linked conversations and drafts", async ({ page }, info) => {
        await openApp(page, "/channels", { theme, scenario: "mail", mode: "developer" });
        await expect(page.getByRole("heading", { name: "Bozze email", level: 2 })).toBeVisible();
        await page.getByRole("switch", { name: /Mostra anche le bozze chiuse/ }).click();
        await expect(page.getByRole("article", { name: /Grazie/ })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
      });

      test("the editor, the refusal and the confirmation", async ({ page }, info) => {
        await openApp(page, "/channels", { theme, scenario: "mail", mode: "developer" });
        const d = page.getByRole("article", { name: /Preventivo per il cantiere/ });
        await d.getByRole("button", { name: "Modifica" }).click();
        await expect(d.getByRole("form", { name: "Modifica" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await d.getByRole("button", { name: "Chiudi senza salvare" }).click();
        await d.getByRole("button", { name: "Rifiuta" }).click();
        await expect(d.getByLabel("Motivo (facoltativo)")).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await d.getByRole("button", { name: "Non ancora" }).click();
        await d.getByRole("button", { name: "Approva e invia" }).click();
        await expect(page.getByRole("dialog", { name: "Invio questa email?" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("the form that links accounts", async ({ page }, info) => {
        await openApp(page, "/channels", { theme, scenario: "mail", mode: "developer" });
        await page.getByRole("button", { name: "Collega degli account" }).click();
        await expect(page.getByRole("form", { name: "Collega degli account" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
}
