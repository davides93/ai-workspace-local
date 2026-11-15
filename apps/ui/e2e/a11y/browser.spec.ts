import type { Locator } from "@playwright/test";
import { expect, test } from "../support/app";
import { browserButton, clickFrame, frameImage, handoverCard, log, openBrowserSession, openHandover, panel, signInThroughViewer, statusLine, viewer } from "../support/browser";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// The live view of the browser, scanned with axe-core in every state it has (the panel with the agent driving, with the person driving, with
// a window of the site; the card of a request for help, alone and next to the open panel; a device that cannot see the browser), in both themes
// at phone and desktop width; the touch targets and the width on the phone; the same screens in English.
const SIZES = [
  { name: "390", viewport: { width: 390, height: 844 } },
  { name: "1440", viewport: { width: 1440, height: 900 } },
] as const;

/** The picture is there (axe reads the page around it, but a half-loaded frame would change the layout under the scan). */
async function frameLoaded(scope: Locator) {
  await expect(frameImage(scope)).toBeVisible();
  await expect.poll(() => frameImage(scope).evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
}

for (const size of SIZES) {
  for (const theme of ["light", "dark"] as const) {
    test.describe(`${size.name} · ${theme}`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });

      test("the «Browser» button, and the panel while the agent drives, paused and giving the picture a text alternative", async ({ page }, info) => {
        await openBrowserSession(page, { theme });
        await expectNoBlockingViolations(page, info);
        await browserButton(page).click();
        const view = panel(page);
        await frameLoaded(view);
        await expect(statusLine(view, "L'agente sta usando il browser")).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await view.getByRole("button", { name: "Pausa agente" }).click();
        await expect(statusLine(view, "Agente in pausa")).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("the panel while the person drives: the viewer, the keys, the field, the live region of the focus, the notice", async ({ page }, info) => {
        // The request for help puts the sign-in page in front, which is a page with fields (the quotes page has none to put the focus on).
        await openHandover(page, { theme });
        await browserButton(page).click();
        const view = panel(page);
        await expect(statusLine(view, "Stai guidando il browser: l'agente aspetta")).toBeVisible();
        await expect(viewer(view)).toBeVisible();
        await frameLoaded(view);
        await view.getByRole("button", { name: "Tab", exact: true }).click();
        await expect(view.getByRole("status").filter({ hasText: "campo di testo «Email»" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        // The text shown instead of masked, and the viewer with the focus ring.
        await view.getByLabel("Scrivi nella pagina").fill("TEST-ONLY-1234");
        await view.getByRole("button", { name: "Mostra il testo" }).click();
        await viewer(view).focus();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("the panel with a window of the site waiting for an answer (a confirmation), and then a closed request in the conversation", async ({ page }, info) => {
        await openHandover(page, { theme });
        await browserButton(page).click();
        const view = panel(page);
        await frameLoaded(view);
        await signInThroughViewer(page, view);
        await clickFrame(view, 0.9, 0.9);
        await expect(view.getByRole("alertdialog", { name: "Il sito chiede una conferma" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await view.getByRole("alertdialog").getByRole("button", { name: "Annulla" }).click();
        await view.getByRole("button", { name: "Restituisci all'agente" }).click();
        await page.keyboard.press("Escape");
        await expect(view).toBeHidden();
        await expect(log(page).getByText("Hai restituito il browser all'agente.")).toBeVisible();
        await expect(log(page).getByText("Grazie: adesso sono dentro.")).toBeVisible();
        await expect(page.getByRole("button", { name: /^(Ferma|Stop)$/ })).toBeHidden();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("the card of a request for help, with the view of the browser in it", async ({ page }, info) => {
        await openHandover(page, { theme });
        const card = handoverCard(page);
        await frameLoaded(card);
        await expect(viewer(card)).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        // The same card with the text of the field shown, an error of an answer that could not go, and the pause of the agent does not apply.
        await card.getByLabel("Scrivi nella pagina").fill("TEST-ONLY-code");
        await card.getByRole("button", { name: "Mostra il testo" }).click();
        await expectNoBlockingViolations(page, info);
      });

      test("the card next to the open panel (it points to it instead of showing a second view)", async ({ page }, info) => {
        await openHandover(page, { theme });
        await browserButton(page).click();
        await expect(panel(page)).toBeVisible();
        await frameLoaded(panel(page));
        if (size.name === "1440") {
          await expect(handoverCard(page).getByText("La vista del browser è aperta nel pannello a lato")).toBeVisible();
        }
        await expectNoBlockingViolations(page, info);
      });

      test("the card on a device that may not see the browser", async ({ page }, info) => {
        await openHandover(page, { theme, query: "scope=device" });
        await expect(handoverCard(page).getByText("Da questo dispositivo non posso mostrarti il browser.", { exact: false })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
}

test.describe("390 · the phone", () => {
  test.use({ viewport: SIZES[0].viewport });

  test("the sheet fills the screen, nothing scrolls sideways, every control of the view is at least 44 px high, and the button keeps its name", async ({ page }) => {
    await openHandover(page);
    // The header has no word on the button (it is tight) but the name is whole.
    await expect(browserButton(page)).toHaveAccessibleName("Browser: serve il tuo intervento");
    await browserButton(page).click();
    const view = panel(page);
    await frameLoaded(view);
    const box = await view.boundingBox();
    expect(box?.width ?? 0).toBeGreaterThanOrEqual(389);
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(843);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    expect(await view.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    const controls = view.locator(".bl .btn, .bl .input");
    const n = await controls.count();
    expect(n).toBeGreaterThan(5);
    for (let i = 0; i < n; i++) {
      const b = await controls.nth(i).boundingBox();
      expect(b?.height ?? 0, `control #${i}`).toBeGreaterThanOrEqual(43.5);
    }
  });

  test("the card: the viewer and its controls fit the width, and its buttons are at least 44 px high", async ({ page }) => {
    await openHandover(page);
    const card = handoverCard(page);
    await frameLoaded(card);
    expect(await card.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    const controls = card.locator(".btn, .input");
    const n = await controls.count();
    expect(n).toBeGreaterThan(5);
    for (let i = 0; i < n; i++) {
      const b = await controls.nth(i).boundingBox();
      expect(b?.height ?? 0, `control #${i}`).toBeGreaterThanOrEqual(43.5);
    }
  });
});

test.describe("English", () => {
  test.use({ locale: "en-US" });
  for (const size of SIZES) {
    test.describe(size.name, () => {
      test.use({ viewport: size.viewport });

      test("the card and the panel", async ({ page }, info) => {
        await openHandover(page);
        await frameLoaded(handoverCard(page));
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await page.locator("header.ws-head").getByRole("button", { name: /^Browser: your help is needed$/ }).click();
        const view = panel(page);
        await frameLoaded(view);
        await expect(view.getByRole("button", { name: "Give back to the agent" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
});
