import type { Locator, Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { chatWith, FAKE_TOKEN, WRONG_TOKEN } from "../support/ask";
import { expectNoBlockingViolations, expectNoPlaceholderLeaks } from "../support/scan";

// The cards of the questions and of the requests for a secret, in every state they have, scanned with axe-core in both
// themes at phone and desktop width; the touch targets on the phone; the same cards in English.
const SIZES = [
  { name: "390", viewport: { width: 390, height: 844 } },
  { name: "1440", viewport: { width: 1440, height: 900 } },
] as const;

const SESSION = "ses_pdf";
const log = (page: Page) => page.getByRole("log", { name: "Conversazione" });

/** Sends another message once the run before it is over (a message sent while it goes on is not sent). */
async function sayNext(page: Page, text: string) {
  await expect(page.getByRole("button", { name: /^(Ferma|Stop)$/ })).toBeHidden();
  await page.locator("#composer").fill(text);
  await page.locator("#composer").press("Enter");
}

for (const size of SIZES) {
  for (const theme of ["light", "dark"] as const) {
    test.describe(`${size.name} · ${theme}`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme });

      test("question with a single choice", async ({ page }, info) => {
        await chatWith(page, "Scarica un modello", { theme, session: SESSION });
        await expect(log(page).getByRole("form", { name: "Quale modello?" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("question with several choices and «Altro» picked", async ({ page }, info) => {
        await chatWith(page, "Quali modelli confrontiamo?", { theme, session: SESSION });
        const card = log(page).getByRole("form", { name: "Quali modelli?" });
        await card.getByRole("checkbox", { name: "Gemma 4 E4B" }).check();
        await card.getByRole("checkbox", { name: "Altro" }).check();
        await card.getByRole("textbox", { name: "Scrivi la tua risposta" }).fill("Phi 4 mini");
        await expectNoBlockingViolations(page, info);
        // The error of an empty answer is in a live alert, and still readable.
        await card.getByRole("textbox", { name: "Scrivi la tua risposta" }).fill("");
        await card.getByRole("button", { name: "Invia" }).click();
        await expect(card.getByRole("alert")).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("confirmation question", async ({ page }, info) => {
        await chatWith(page, "Cancella i download vecchi", { theme, session: SESSION });
        await expect(log(page).getByRole("form", { name: "Cancello i vecchi?" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("form question, empty and with its error", async ({ page }, info) => {
        await chatWith(page, "Crea un assistente per le email", { theme, session: SESSION });
        const card = log(page).getByRole("form", { name: "Il nuovo assistente" });
        await expect(card).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await card.getByRole("button", { name: "Invia" }).click();
        await expect(card.getByRole("alert")).toHaveText("Compila «Nome»");
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("request for a secret: empty, with the value shown, and after a refused value", async ({ page }, info) => {
        await chatWith(page, "Aggiungi il token di Hugging Face", { theme, session: SESSION });
        const card = log(page).getByRole("form", { name: "Token di Hugging Face" });
        await expect(card).toBeVisible();
        await expectNoBlockingViolations(page, info);
        const field = card.getByLabel("Valore di «Token di Hugging Face»");
        await field.fill(WRONG_TOKEN);
        await card.getByRole("button", { name: "Mostra il valore" }).click();
        await expectNoBlockingViolations(page, info);
        await field.press("Enter");
        await expect(card.getByRole("alert")).toContainText("Il valore non è stato accettato");
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("request for a secret on a device that may not provide it", async ({ page }, info) => {
        await chatWith(page, "Aggiungi il token di Hugging Face", { theme, session: SESSION, query: "scope=device" });
        const card = log(page).getByRole("form", { name: "Token di Hugging Face" });
        await card.getByLabel("Valore di «Token di Hugging Face»").fill(FAKE_TOKEN);
        await card.getByRole("button", { name: "Salva" }).click();
        await expect(card.getByRole("alert")).toContainText("Inseriscilo dall'app su questo computer.");
        await expectNoBlockingViolations(page, info);
      });

      test("closed rows: an answered question, a request for a secret that was stored, and the confirmation of a management tool", async ({ page }, info) => {
        await chatWith(page, "Scarica un modello", { theme, session: SESSION });
        const card = log(page).getByRole("form", { name: "Quale modello?" });
        await card.getByRole("radio", { name: "Q5_K_M · 5,6 GB" }).check();
        await card.getByRole("button", { name: "Invia" }).click();
        await expect(log(page).getByText("Hai risposto: Q5_K_M · 5,6 GB")).toBeVisible();
        // The confirmation of `aiw_model_download`, with the warning about what was read from outside, and no «Consenti per la sessione».
        const confirm = log(page).getByRole("article", { name: "aiw_model_download" });
        await expect(confirm.getByText("Richiesta arrivata dopo la lettura di contenuti esterni")).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await confirm.getByRole("button", { name: "Consenti", exact: true }).click();
        await expect(log(page).getByText("Fatto: il download è partito")).toBeVisible();

        await sayNext(page, "Aggiungi il token di Hugging Face");
        const secret = log(page).getByRole("form", { name: "Token di Hugging Face" });
        await secret.getByLabel("Valore di «Token di Hugging Face»").fill(FAKE_TOKEN);
        await secret.getByRole("button", { name: "Salva" }).click();
        await expect(log(page).getByText("Salvato: Token di Hugging Face")).toBeVisible();
        await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("banner for a question that waits in another conversation", async ({ page }, info) => {
        await chatWith(page, "Scarica un modello", { theme, session: SESSION });
        await expect(log(page).getByRole("form", { name: "Quale modello?" })).toBeVisible();
        await page.evaluate(() => { location.hash = "/models"; });
        const banner = page.getByRole("status").filter({ hasText: "L'assistente ha una domanda in attesa" });
        await expect(banner).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
}

test.describe("390 · touch targets", () => {
  test.use({ viewport: SIZES[0].viewport });

  /** Every control of the card is at least 44 px high (and the rows of options too): the phone's rule, not the 40 px of the shared buttons. */
  async function expectTargets(controls: Locator, label: string) {
    const n = await controls.count();
    expect(n, `${label}: nothing to measure`).toBeGreaterThan(0);
    for (let i = 0; i < n; i++) {
      const box = await controls.nth(i).boundingBox();
      expect(box?.height ?? 0, `${label} #${i}`).toBeGreaterThanOrEqual(43.5);
    }
  }

  test("choice, confirmation, form and secret cards", async ({ page }) => {
    await chatWith(page, "Quali modelli confrontiamo?", { session: SESSION });
    const choice = log(page).getByRole("form", { name: "Quali modelli?" });
    await expect(choice).toBeVisible();
    await expectTargets(choice.locator(".ask-opt, .ask-opt-pick"), "option rows");
    await expectTargets(choice.locator(".btn"), "buttons of the choice");
    await expectTargets(choice.locator("input.input"), "field «Altro»");
    await choice.getByRole("button", { name: "Non ora" }).click();
    await expect(log(page).getByText("Hai scelto di non rispondere ora.")).toBeVisible();

    await sayNext(page, "Cancella i download vecchi");
    const confirm = log(page).getByRole("form", { name: "Cancello i vecchi?" });
    await expect(confirm).toBeVisible();
    await expectTargets(confirm.locator(".btn"), "buttons of the confirmation");
    await confirm.getByRole("button", { name: "Non ora" }).click();

    await sayNext(page, "Crea un assistente per le email");
    const form = log(page).getByRole("form", { name: "Il nuovo assistente" });
    await expect(form).toBeVisible();
    await expectTargets(form.locator(".input, .btn"), "form fields and buttons");
    await expectTargets(form.locator(".ask-check"), "form checkbox row");
    await form.getByRole("button", { name: "Non ora" }).click();

    await sayNext(page, "Aggiungi il token di Hugging Face");
    const secret = log(page).getByRole("form", { name: "Token di Hugging Face" });
    await expect(secret).toBeVisible();
    await expectTargets(secret.locator(".input, .btn"), "secret field and buttons");
  });
});

test.describe("English", () => {
  test.use({ locale: "en-US" });
  for (const size of SIZES) {
    test.describe(size.name, () => {
      test.use({ viewport: size.viewport });

      test("the question cards and the request for a secret", async ({ page }, info) => {
        await chatWith(page, "Scarica un modello", { session: SESSION });
        await expect(page.locator("html")).toHaveAttribute("lang", "en");
        const card = page.getByRole("form", { name: "Quale modello?" });
        await expect(card).toBeVisible();
        await expect(card.getByRole("button", { name: "Not now" })).toBeVisible();
        await expect(card.getByRole("button", { name: "Send" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await card.getByRole("button", { name: "Not now" }).click();
        await expect(page.getByText("You chose not to answer now.")).toBeVisible();

        await sayNext(page, "Aggiungi il token di Hugging Face");
        const secret = page.getByRole("form", { name: "Token di Hugging Face" });
        await expect(secret).toBeVisible();
        await expect(secret.getByText("It will be saved in: Hugging Face (huggingface.co)")).toBeVisible();
        await secret.getByLabel("Value of “Token di Hugging Face”").fill(WRONG_TOKEN);
        await secret.getByRole("button", { name: "Save" }).click();
        await expect(secret.getByRole("alert")).toContainText("The value was not accepted");
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
});
