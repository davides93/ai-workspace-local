import type { Locator, Page } from "@playwright/test";
import { expect, openApp, test } from "../support/app";

// «Conversazioni collegate» e «Bozze email» (demo `mail`): the owner links accounts on purpose, reads a draft whole
// (hidden copies and attachments included), and an email leaves only after «Approva e invia» + a second confirmation,
// never from a refusal or a cancel. All of it with the keyboard too. The engine's own proof is in the Rust tests.

const draft = (page: Page, name: RegExp | string): Locator => page.getByRole("article", { name });

test.describe("linked conversations and email drafts", () => {
  test.beforeEach(async ({ page }) => {
    await openApp(page, "/channels", { scenario: "mail", mode: "developer" });
    await expect(page.getByRole("heading", { name: "Bozze email", level: 2 })).toBeVisible();
  });

  test("the linked conversation lists its accounts and opens the shared chat", async ({ page }) => {
    const ctx = page.getByRole("article", { name: "Mario, ovunque" });
    await expect(ctx).toBeVisible();
    const accounts = ctx.getByRole("list", { name: "Account di Mario, ovunque" });
    await expect(accounts.getByRole("listitem")).toHaveCount(2);
    await expect(accounts).toContainText("Telegram");
    await expect(accounts).toContainText("mario@example.it");
    await expect(ctx).toContainText("Chiede conferma");
    await ctx.getByRole("link", { name: "Apri la conversazione" }).click();
    await expect(page).toHaveURL(/#\/chat\/ses_/);
    // The chat list marks it as shared.
    await expect(page.getByText("Condivisa").first()).toBeVisible();
  });

  test("accounts are linked on purpose, one per channel, and each account in one conversation only", async ({ page }) => {
    // Both accounts are already in «Mario, ovunque»: the form offers them greyed out and refuses a second link.
    await page.getByRole("button", { name: "Collega degli account" }).click();
    const form = page.getByRole("form", { name: "Collega degli account" });
    await form.getByLabel("Nome della conversazione").fill("Doppione");
    await form.getByRole("button", { name: "Collega gli account" }).click();
    await expect(form.getByRole("alert")).toContainText("Scegli almeno un account");
    await expect(form.getByLabel("Account su Telegram").locator("option", { hasText: "111" })).toBeDisabled();
    // Unlink the first one (asked twice), then link again.
    await form.getByRole("button", { name: "Annulla" }).click();
    await page.getByRole("article", { name: "Mario, ovunque" }).getByRole("button", { name: "Scollega" }).click();
    const dialog = page.getByRole("dialog", { name: "Scollega" });
    await expect(dialog).toContainText("La cronologia resta nella chat");
    await dialog.getByRole("button", { name: "Sì, scollega" }).click();
    await expect(page.getByText("Nessun account collegato")).toBeVisible();
    await page.getByRole("button", { name: "Collega degli account" }).click();
    const again = page.getByRole("form", { name: "Collega degli account" });
    await again.getByLabel("Nome della conversazione").fill("Io, ovunque");
    await again.getByLabel("Account su Telegram").selectOption("111");
    await again.getByLabel("Account su Posta").selectOption("mario@example.it");
    await again.getByRole("button", { name: "Collega gli account" }).click();
    await expect(page.getByRole("article", { name: "Io, ovunque" })).toBeVisible();
  });

  test("the draft is shown whole: sender, recipients with the hidden copy, text, attachment and fingerprint", async ({ page }) => {
    const d = draft(page, /Preventivo per il cantiere/);
    await expect(d).toContainText("In attesa di revisione");
    await expect(d).toContainText("Scritta dall'assistente");
    await expect(d).toContainText("mail:<richiesta@cliente.it>");
    await expect(d.getByRole("note")).toHaveCount(0);
    await expect(d).toContainText("assistente@casa.example");
    await expect(d).toContainText("anna@cliente.it");
    await expect(d).toContainText("capo@esempio.it");
    await expect(d).toContainText("archivio@esempio.it");
    await expect(d.getByText("Ccn (nascosti)")).toBeVisible();
    await expect(d).toContainText("in allegato il preventivo richiesto");
    await expect(d).toContainText("preventivo-cantiere.pdf");
    await expect(d).toContainText("Impronta del contenuto");
    await expect(d).toContainText("Scritta dall'assistente: controlla che destinatari e testo siano quelli che volevi");
    // Nothing has gone: the sent draft is closed and hidden until asked for.
    await expect(draft(page, /Grazie/)).toHaveCount(0);
    await page.getByRole("switch", { name: /Mostra anche le bozze chiuse/ }).click();
    await expect(draft(page, /Grazie/)).toContainText("Inviata");
  });

  test("approving asks again with the recipients, and only «Sì, invia questo contenuto» sends", async ({ page }) => {
    const d = draft(page, /Preventivo per il cantiere/);
    await d.getByRole("button", { name: "Approva e invia" }).click();
    const dialog = page.getByRole("dialog", { name: "Invio questa email?" });
    await expect(dialog).toContainText("anna@cliente.it, in copia a capo@esempio.it, in copia nascosta a archivio@esempio.it");
    await expect(dialog).toContainText("1 allegato");
    // «Non ancora» closes it and nothing went.
    await dialog.getByRole("button", { name: "Non ancora" }).click();
    await expect(dialog).toBeHidden();
    await expect(d).toContainText("In attesa di revisione");
    await d.getByRole("button", { name: "Approva e invia" }).click();
    await page.getByRole("dialog", { name: "Invio questa email?" }).getByRole("button", { name: "Sì, invia questo contenuto" }).click();
    await expect(draft(page, /Preventivo per il cantiere/)).toHaveCount(0);
    await page.getByRole("switch", { name: /Mostra anche le bozze chiuse/ }).click();
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("Inviata");
  });

  test("a draft that changes under the open confirmation closes it: the old approval does not cover the new text", async ({ page }) => {
    const d = draft(page, /Preventivo per il cantiere/);
    await d.getByRole("button", { name: "Approva e invia" }).click();
    const dialog = page.getByRole("dialog", { name: "Invio questa email?" });
    await expect(dialog).toBeVisible();
    // The assistant revises the draft while the owner is deciding (the list is asked again every few seconds).
    await page.evaluate(() => (globalThis as unknown as { __aiwDemo: { reviseDraft(s: string, b: string): void } }).__aiwDemo.reviseDraft("Preventivo per il cantiere", "Testo cambiato dall'assistente"));
    await expect(dialog).toBeHidden({ timeout: 15_000 });
    const after = draft(page, /Preventivo per il cantiere/);
    await expect(after.getByRole("alert")).toContainText("La bozza è cambiata mentre decidevi");
    await expect(after).toContainText("Testo cambiato dall'assistente");
    await expect(after).toContainText("Revisione 2");
    await expect(after).toContainText("In attesa di revisione");
    // Nothing went; approving again sends the content that is on the card now.
    await after.getByRole("button", { name: "Approva e invia" }).click();
    await page.getByRole("dialog", { name: "Invio questa email?" }).getByRole("button", { name: "Sì, invia questo contenuto" }).click();
    await expect(draft(page, /Preventivo per il cantiere/)).toHaveCount(0);
    await page.getByRole("switch", { name: /Mostra anche le bozze chiuse/ }).click();
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("Testo cambiato dall'assistente");
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("Inviata");
  });

  test("refusing sends nothing and keeps the reason; cancelling withdraws the draft", async ({ page }) => {
    const luca = draft(page, /Conferma riunione/);
    await luca.getByRole("button", { name: "Rifiuta" }).click();
    await luca.getByLabel("Motivo (facoltativo)").fill("Lo scrivo io a voce");
    await luca.getByRole("button", { name: "Rifiuta senza inviare" }).click();
    await expect(draft(page, /Conferma riunione/)).toHaveCount(0);
    const rotto = draft(page, /Sollecito fattura/);
    await expect(rotto).toContainText("Non partita");
    await expect(rotto).toContainText("non è partito nulla");
    await rotto.getByRole("button", { name: "Annulla la bozza" }).click();
    await expect(draft(page, /Sollecito fattura/)).toHaveCount(0);
    await page.getByRole("switch", { name: /Mostra anche le bozze chiuse/ }).click();
    await expect(draft(page, /Conferma riunione/)).toContainText("Rifiutata");
    await expect(draft(page, /Conferma riunione/)).toContainText("Lo scrivo io a voce");
    await expect(draft(page, /Sollecito fattura/)).toContainText("Annullata");
    // The first draft is still waiting: nothing was sent.
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("In attesa di revisione");
  });

  test("editing makes a new revision: the fingerprint changes, and an attachment is added from the workspace only", async ({ page }) => {
    const d = draft(page, /Preventivo per il cantiere/);
    const before = await d.getByText(/Impronta del contenuto/).innerText();
    await d.getByRole("button", { name: "Modifica" }).click();
    const form = d.getByRole("form", { name: "Modifica" });
    await form.getByLabel("Testo").fill("Testo corretto.");
    await form.getByLabel("Aggiungi un allegato dal workspace").fill("/etc/passwd");
    await form.getByRole("button", { name: "Aggiungi" }).click();
    await form.getByRole("button", { name: "Salva le modifiche" }).click();
    await expect(form.getByRole("alert")).toContainText("fuori dal workspace");
    await form.getByRole("button", { name: "Rimuovi l'allegato /etc/passwd" }).click();
    await form.getByRole("button", { name: "Salva le modifiche" }).click();
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("Testo corretto.");
    await expect(draft(page, /Preventivo per il cantiere/)).toContainText("Revisione 2");
    expect(await draft(page, /Preventivo per il cantiere/).getByText(/Impronta del contenuto/).innerText()).not.toBe(before);
  });

  test("the owner writes a draft by hand and finds it waiting for review", async ({ page }) => {
    await page.getByRole("button", { name: "Scrivi una bozza" }).click();
    const form = page.getByRole("form", { name: "Nuova bozza" });
    await form.getByRole("textbox", { name: "A", exact: true }).fill("giulia@esempio.it");
    await form.getByRole("textbox", { name: "A", exact: true }).press("Enter");
    await form.getByLabel("Oggetto").fill("Una prova");
    await form.getByLabel("Testo").fill("Ciao Giulia.");
    await form.getByRole("button", { name: "Crea la bozza" }).click();
    const d = draft(page, /Una prova/);
    await expect(d).toContainText("Scritta da te");
    await expect(d).toContainText("giulia@esempio.it");
    await expect(d).toContainText("In attesa di revisione");
  });

  test("everything can be done with the keyboard: the confirmation takes the focus and Esc gives it back", async ({ page }) => {
    const d = draft(page, /Preventivo per il cantiere/);
    const approve = d.getByRole("button", { name: "Approva e invia" });
    await approve.focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "Invio questa email?" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Non ancora" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(approve).toBeFocused();
  });
});

test.describe("English", () => {
  test.use({ locale: "en-US" });
  test("the review is there in English too", async ({ page }) => {
    await openApp(page, "/channels", { scenario: "mail", mode: "developer" });
    await expect(page.getByRole("heading", { name: "Email drafts", level: 2 })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Linked conversations", level: 2 })).toBeVisible();
    await expect(page.getByRole("button", { name: "Approve and send" }).first()).toBeVisible();
  });
});
