import { expect, openApp, test } from "../support/app";
import { expectNoBlockingViolations, expectNoHorizontalScroll, expectNoPlaceholderLeaks } from "../support/scan";
import { FAKE_KEY, messageBox, startDownloadThenGoSimple } from "../support/simple";

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

      test("first run of the simple interface (the panel is in the chat)", async ({ page }, info) => {
        await openApp(page, "/chat", { theme, scenario: "first-run" });
        await expect(page.getByRole("button", { name: "Prepara tutto" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("settings of the Mac App Store build: allowed folders", async ({ page }, info) => {
        await openApp(page, "/settings", { theme, scenario: "mas", mode: "developer" });
        await expect(page.getByTestId("folders-panel")).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        // The empty state of the list too.
        await page.getByRole("button", { name: "Togli «Download»" }).click();
        await expect(page.getByTestId("folders-empty")).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("a refused folder in the chat of the Mac App Store build, with its hint", async ({ page }, info) => {
        await openApp(page, "/chat/ses_mas", { theme, scenario: "mas" });
        await expect(page.getByRole("note").filter({ hasText: "fuori dal workspace" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("first run while the model downloads", async ({ page }, info) => {
        await openApp(page, "/chat", { theme, scenario: "first-run" });
        await page.getByRole("button", { name: "Prepara tutto" }).click();
        await expect(page.getByRole("meter", { name: /^Avanzamento di / })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("simple chat: a failed session with its notes, and the lesson panel", async ({ page }, info) => {
        await openApp(page, "/chat/ses_bug", { theme });
        await expect(page.getByRole("log", { name: "Conversazione" }).getByRole("button", { name: /^Chiedi all'assistente/ })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await page.getByRole("button", { name: /^Rivedi la lezione/ }).click();
        await expect(page.getByRole("dialog", { name: "Lezioni da rivedere" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("simple chat: the warning about a secret in the message box", async ({ page }, info) => {
        await openApp(page, "/chat/ses_pdf", { theme });
        await messageBox(page).fill(`Usa questa chiave ${FAKE_KEY}`);
        await expect(page.getByRole("status").filter({ hasText: "Sembra una chiave o un token" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("simple chat: the strip of the downloads that go on", async ({ page }, info) => {
        await openApp(page, "/models/catalog", { theme, mode: "developer" });
        await startDownloadThenGoSimple(page);
        await expect(page.getByRole("region", { name: "Download in corso" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("simple chat: the menu", async ({ page }, info) => {
        await openApp(page, "/chat", { theme });
        await page.getByRole("button", { name: "Menu", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Menu" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("command palette", async ({ page }, info) => {
        await openApp(page, "/chat", { theme });
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

      test("proposals: the background tasks in every state, the refusal, the dialog, no sideways scroll", async ({ page }, info) => {
        await openApp(page, "/inbox", { theme, mode: "developer" });
        const tasks = page.getByRole("region", { name: "Lavori in background" });
        const run = tasks.getByRole("article", { name: "Archiviare le fatture di ottobre" });
        // The demo: one at work, paused for quiet hours, a spent budget and a restart, two finished.
        await expect(tasks.getByRole("article")).toHaveCount(4);
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await expectNoHorizontalScroll(page, ".tk");
        // The history of every finished task, with the box that is switched off.
        for (const details of await tasks.getByText("Dettagli", { exact: true }).all()) await details.click();
        await expectNoBlockingViolations(page, info);
        await expectNoHorizontalScroll(page, ".tk");
        // A pause asked (the step is being finished), then paused, then the confirmation it asks for after the resume.
        await run.getByRole("button", { name: "Pausa", exact: true }).click();
        await expect(run.getByText("Si sta fermando", { exact: true })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expect(run.getByText("In pausa", { exact: true })).toBeVisible();
        await run.getByRole("button", { name: "Riprendi", exact: true }).click();
        await expect(run.getByRole("group", { name: "Conferma in attesa" })).toBeVisible();
        await run.getByText(/^Cronologia \(\d+\)$/).click();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await expectNoHorizontalScroll(page, ".tk");
        // The engine's refusal to resume in quiet hours, and an instruction that is too long.
        await tasks.getByRole("article", { name: "Confrontare le offerte dei fornitori" }).getByRole("button", { name: "Riprendi", exact: true }).click();
        await expect(tasks.locator(".tk-refusal")).toBeVisible();
        await run.getByRole("textbox", { name: "Istruzione" }).fill("a".repeat(4001));
        await expect(run.getByRole("alert").filter({ hasText: "supera i 4000 caratteri" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoHorizontalScroll(page, ".tk");
        // The question before cancelling.
        await run.getByRole("button", { name: "Annulla", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Annullare il lavoro?" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

      test("the workspace does not answer: the connection banner and the error state of the chat", async ({ page }, info) => {
        await openApp(page, "/chat", { theme, scenario: "offline" });
        await expect(page.getByText("Connessione al workspace persa")).toBeVisible();
        await expect(page.getByRole("button", { name: "Riprova" }).first()).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("a browser that is not paired: the pairing screen, and a wrong code", async ({ page }, info) => {
        await openApp(page, "/", { theme, scenario: "unpaired" });
        await expect(page.getByRole("heading", { name: "Accoppia questo dispositivo" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await page.getByRole("textbox", { name: "Codice di pairing" }).fill("000000");
        await page.getByRole("button", { name: "Accoppia" }).click();
        await expect(page.getByText(/codice non valido o scaduto/)).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });

      test("documents with a search result", async ({ page }, info) => {
        await openApp(page, "/documents", { theme, mode: "developer" });
        const search = page.getByRole("region", { name: "Cerca nei documenti" });
        await search.getByRole("searchbox", { name: "Cosa cerchi?" }).fill("canone");
        await search.getByRole("button", { name: "Cerca", exact: true }).click();
        await expect(search.getByText("contratto-affitto.txt").first()).toBeVisible();
        await expectNoBlockingViolations(page, info);
      });

    });

    test.describe(`${size.name} · ${theme} · English`, () => {
      test.use({ viewport: size.viewport, colorScheme: theme, locale: "en-US" });
      for (const route of ["/", "/chat/ses_pdf", "/models", "/settings", "/documents", "/devices", "/inbox"]) {
        test(route, async ({ page }, info) => {
          await openApp(page, route, { theme, mode: "developer" });
          await expect(page.locator("html")).toHaveAttribute("lang", "en");
          await expectNoBlockingViolations(page, info);
          await expectNoPlaceholderLeaks(page);
        });
      }
      test("the chat of the simple interface, and its menu", async ({ page }, info) => {
        await openApp(page, "/chat/ses_pdf", { theme });
        await expect(page.locator("html")).toHaveAttribute("lang", "en");
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
        await page.getByRole("button", { name: "Menu", exact: true }).click();
        await expect(page.getByRole("dialog", { name: "Menu" })).toBeVisible();
        await expectNoBlockingViolations(page, info);
        await expectNoPlaceholderLeaks(page);
      });
    });
  }
}
