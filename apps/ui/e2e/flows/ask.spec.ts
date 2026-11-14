import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { chatWith, FAKE_TOKEN, WRONG_TOKEN, whereIs } from "../support/ask";

// The questions and the requests for a secret of the agent, in the chat of the demo (scripts of
// src/shared/api/mock/assistant.ts). The cards are used with the keyboard alone wherever a person can.

const log = (page: Page) => page.getByRole("log", { name: "Conversazione" });
const sessionList = (page: Page) => page.getByRole("navigation", { name: "Elenco delle sessioni" });

test.describe("questions of the agent", () => {
  test("single choice: answered from the keyboard, the run goes on, the card stays as a closed row, and it is still there after reopening the conversation", async ({ page }) => {
    await chatWith(page, "Scarica un modello");
    const card = log(page).getByRole("form", { name: "Quale modello?" });
    await expect(card).toBeVisible();
    await expect(card.getByText("Ho trovato tre varianti adatte a questo computer.", { exact: true })).toBeVisible();
    // The screen reader is told, and the focus left the message box for the first option.
    await expect(card.getByRole("status")).toHaveText(/L'assistente ha una domanda: Ho trovato tre varianti/);
    const q4 = card.getByRole("radio", { name: "Q4_K_M · 4,7 GB" });
    const q5 = card.getByRole("radio", { name: "Q5_K_M · 5,6 GB" });
    const q8 = card.getByRole("radio", { name: "Q8_0 · 8,5 GB" });
    await expect(q4).toBeFocused();
    await expect(card.getByRole("radiogroup")).toBeVisible();

    // Arrows move and choose, the digits choose too, Enter sends.
    await page.keyboard.press("ArrowDown");
    await expect(q5).toBeChecked();
    await expect(q5).toBeFocused();
    await page.keyboard.press("3");
    await expect(q8).toBeChecked();
    await page.keyboard.press("1");
    await expect(q4).toBeChecked();
    await expect(q5).not.toBeChecked();
    await page.keyboard.press("Enter");

    // The question is now a line with the choice made; the run goes on to the confirmation of a management tool.
    await expect(card).toBeHidden();
    await expect(log(page).getByText("Hai risposto: Q4_K_M · 4,7 GB")).toBeVisible();
    const confirm = log(page).getByRole("article", { name: "aiw_model_download" });
    await expect(confirm).toBeVisible();
    // It came after a web search: say so. And «Consenti per la sessione» is not offered for a management tool.
    await expect(confirm.getByText("Richiesta arrivata dopo la lettura di contenuti esterni")).toBeVisible();
    await expect(confirm.getByRole("button", { name: "Consenti per la sessione" })).toHaveCount(0);
    await expect(confirm.getByRole("button", { name: "Consenti", exact: true })).toBeVisible();
    await expect(confirm.getByRole("button", { name: "Nega", exact: true })).toBeVisible();
    await confirm.getByRole("button", { name: "Consenti", exact: true }).click();
    await expect(log(page).getByText("Fatto: il download è partito")).toBeVisible();
    await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();
    // The answered question is still a closed row, and there is exactly one of them.
    await expect(log(page).getByText("Hai risposto: Q4_K_M · 4,7 GB")).toHaveCount(1);

    // Another conversation, and back: the conversation is rebuilt from the stored messages, with the same closed row.
    await sessionList(page).getByRole("button", { name: /^Riordino PDF in Downloads/ }).click();
    await expect(log(page).getByText("Hai risposto: Q4_K_M · 4,7 GB")).toBeHidden();
    await sessionList(page).getByRole("button", { name: /^Scarica un modello/ }).click();
    await expect(log(page).getByText("Hai risposto: Q4_K_M · 4,7 GB")).toBeVisible();
    await expect(log(page).getByRole("form")).toHaveCount(0);
    await expect(log(page).getByText("Fatto: il download è partito")).toBeVisible();
  });

  test("«Non ora» closes the question, and a question left open when the run is stopped ends as without an answer", async ({ page }) => {
    await chatWith(page, "Scarica un modello");
    const card = log(page).getByRole("form", { name: "Quale modello?" });
    await expect(card).toBeVisible();
    await card.getByRole("button", { name: "Non ora" }).click();
    await expect(card).toBeHidden();
    await expect(log(page).getByText("Hai scelto di non rispondere ora.")).toBeVisible();
    await expect(log(page).getByText("Va bene, non scarico nulla.")).toBeVisible();

    // Stopping the run while a question waits: nobody can answer it any more.
    await page.locator("#composer").fill("Scarica un modello");
    await page.locator("#composer").press("Enter");
    const second = log(page).getByRole("form", { name: "Quale modello?" });
    await expect(second).toBeVisible();
    await page.getByRole("button", { name: "Ferma" }).click();
    await expect(second).toBeHidden();
    await expect(log(page).getByText("Senza risposta: il run è finito prima.")).toBeVisible();
  });

  test("an answer that the card cannot send is not lost: nothing is chosen, it says so, and the question stays", async ({ page }) => {
    await chatWith(page, "Quali modelli confrontiamo?");
    const card = log(page).getByRole("form", { name: "Quali modelli?" });
    await expect(card).toBeVisible();
    await card.getByRole("button", { name: "Invia" }).click();
    await expect(card.getByRole("alert")).toHaveText("Scegli un'opzione.");
    // «Altro» picked but empty is not an answer either.
    await card.getByRole("checkbox", { name: "Altro" }).check();
    await card.getByRole("button", { name: "Invia" }).click();
    await expect(card.getByRole("alert")).toHaveText("Scrivi la tua risposta in «Altro».");
    await expect(card).toBeVisible();
  });

  test("several answers and «Altro», from the keyboard: digits choose, but not while typing in the field", async ({ page }) => {
    await chatWith(page, "Quali modelli confrontiamo?");
    const card = log(page).getByRole("form", { name: "Quali modelli?" });
    await expect(card.getByRole("group", { name: "Scegli i modelli da confrontare (puoi sceglierne più di uno)." })).toBeVisible();
    const qwen = card.getByRole("checkbox", { name: "Qwen3 4B" });
    const gemma = card.getByRole("checkbox", { name: "Gemma 4 E4B" });
    const llama = card.getByRole("checkbox", { name: "Llama 3.2 3B" });
    const other = card.getByRole("checkbox", { name: "Altro" });
    await expect(qwen).toBeFocused();

    await page.keyboard.press("1");
    await expect(qwen).toBeChecked();
    await page.keyboard.press("3");
    await expect(llama).toBeChecked();
    await expect(gemma).not.toBeChecked();
    // «Altro» is the next digit; Tab goes to its field, where the digits are text.
    await page.keyboard.press("4");
    await expect(other).toBeChecked();
    await page.keyboard.press("Tab");
    const text = card.getByRole("textbox", { name: "Scrivi la tua risposta" });
    await expect(text).toBeFocused();
    await page.keyboard.type("Phi4");
    await expect(text).toHaveValue("Phi4");
    await expect(other).toBeChecked();
    await expect(qwen).toBeChecked();
    await page.keyboard.press("Enter");

    await expect(card).toBeHidden();
    await expect(log(page).getByText("Hai risposto: Qwen3 4B, Llama 3.2 3B, «Phi4»")).toBeVisible();
    await expect(log(page).getByText("Confronto Qwen3 4B, Llama 3.2 3B, Phi4:")).toBeVisible();
  });

  test("confirmation: the two buttons and «Non ora», reached and used with the keyboard", async ({ page }) => {
    await chatWith(page, "Cancella i download vecchi");
    const card = log(page).getByRole("form", { name: "Cancello i vecchi?" });
    await expect(card).toBeVisible();
    await expect(card.getByText("Ci sono 3 download interrotti che occupano 2,1 GB. Li cancello?")).toBeVisible();
    await expect(card.getByRole("button", { name: "Cancella", exact: true })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(card.getByRole("button", { name: "Tienili", exact: true })).toBeFocused();
    await expect(card.getByRole("button", { name: "Non ora", exact: true })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(card).toBeHidden();
    await expect(log(page).getByText("Hai risposto: Tienili")).toBeVisible();
    await expect(log(page).getByText("Va bene, li tengo.")).toBeVisible();
  });

  test("form: errors in a live alert, fields reached with Tab, Enter sends", async ({ page }) => {
    await chatWith(page, "Crea un assistente per le email");
    const card = log(page).getByRole("form", { name: "Il nuovo assistente" });
    const name = card.getByRole("textbox", { name: "Nome *" });
    await expect(name).toBeFocused();

    // Nothing in the required field.
    await page.keyboard.press("Enter");
    await expect(card.getByRole("alert")).toHaveText("Compila «Nome»");
    await page.keyboard.type("Posta");
    await page.keyboard.press("Tab");
    await expect(card.getByRole("combobox", { name: "Lingua *" })).toBeFocused();
    await page.keyboard.type("ing");
    await page.keyboard.press("Tab");
    const drafts = card.getByRole("textbox", { name: "Quante bozze alla volta" });
    await expect(drafts).toBeFocused();
    await page.keyboard.type("3.5");
    await page.keyboard.press("Enter");
    await expect(card.getByRole("alert")).toContainText("intero");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Backspace");
    await expect(drafts).toHaveValue("3");
    await page.keyboard.press("Tab");
    const auto = card.getByRole("checkbox", { name: "Risponde da solo" });
    await expect(auto).toBeFocused();
    await page.keyboard.press("Space");
    await expect(auto).toBeChecked();
    await page.keyboard.press("Enter");

    await expect(card).toBeHidden();
    await expect(log(page).getByText("Hai risposto: Nome: Posta · Lingua: inglese · Quante bozze alla volta: 3 · Risponde da solo: Sì")).toBeVisible();
    await expect(log(page).getByText("Preparo «Posta» in inglese, con risposta automatica.")).toBeVisible();
  });

  test("a question that waits in another conversation shows a banner that leads to it", async ({ page }) => {
    await chatWith(page, "Scarica un modello");
    const card = log(page).getByRole("form", { name: "Quale modello?" });
    await expect(card).toBeVisible();
    const banner = page.getByRole("status").filter({ hasText: "L'assistente ha una domanda in attesa" });
    // In its own conversation the card is enough: no banner.
    await expect(banner).toHaveCount(0);

    await sessionList(page).getByRole("button", { name: /^Riordino PDF in Downloads/ }).click();
    await expect(banner).toBeVisible();
    await expect(banner.getByText("Rispondi nella sessione «Scarica un modello».")).toBeVisible();
    await banner.getByRole("link", { name: "Apri la sessione" }).click();

    // Back in the conversation the card is there again (the run was still waiting), and the banner is gone.
    await expect(card).toBeVisible();
    await expect(banner).toHaveCount(0);
    await card.getByRole("radio", { name: "Q8_0 · 8,5 GB" }).check();
    await card.getByRole("button", { name: "Invia" }).click();
    await expect(log(page).getByText("Hai risposto: Q8_0 · 8,5 GB")).toBeVisible();
    await log(page).getByRole("article", { name: "aiw_model_download" }).getByRole("button", { name: "Nega", exact: true }).click();
    await expect(log(page).getByText("Ok, non scarico nulla.")).toBeVisible();
  });
});

test.describe("request for a secret", () => {
  test("the value goes from the field to the destination and is nowhere else: not in the page, the storage, the caches, the messages", async ({ page }) => {
    await chatWith(page, "Aggiungi il token di Hugging Face");
    const card = log(page).getByRole("form", { name: "Token di Hugging Face" });
    await expect(card).toBeVisible();
    await expect(card.getByText("Serve per scaricare modelli riservati")).toBeVisible();
    await expect(card.getByText("Sarà salvato in: Hugging Face (huggingface.co)")).toBeVisible();
    await expect(card.getByRole("status")).toHaveText("L'assistente chiede: Token di Hugging Face");

    // The field: protected, no autofill, no spell check, and the focus is in it.
    const field = card.getByLabel("Valore di «Token di Hugging Face»");
    await expect(field).toBeFocused();
    await expect(field).toHaveAttribute("type", "password");
    await expect(field).toHaveAttribute("autocomplete", "off");
    await expect(field).toHaveAttribute("spellcheck", "false");
    await expect(field).toHaveAttribute("autocapitalize", "off");
    await expect(card.getByRole("button", { name: "Salva" })).toBeDisabled();

    // Show and hide, with a button that keeps its name and says whether it is on.
    const toggle = card.getByRole("button", { name: "Mostra il valore" });
    await expect(toggle).toHaveAttribute("aria-pressed", "false");
    await field.fill(WRONG_TOKEN);
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", "true");
    await expect(field).toHaveAttribute("type", "text");
    await toggle.click();
    await expect(field).toHaveAttribute("type", "password");

    // The detector sees what it should: a typed value is in the field, and the demo data does hold other text.
    expect(await whereIs(page, [WRONG_TOKEN])).toEqual(expect.arrayContaining(["field value"]));
    expect(await whereIs(page, ["Riordino PDF in Downloads"])).toEqual(expect.arrayContaining(["query cache", "demo data (messages, events, audit, requests)"]));

    // A value the destination refuses: the card stays, says so (without repeating the value), the field is empty again and focused.
    await field.press("Enter");
    await expect(card.getByRole("alert")).toContainText("Il valore non è stato accettato");
    await expect(card.getByRole("alert")).not.toContainText(WRONG_TOKEN);
    await expect(card).toBeVisible();
    await expect(field).toHaveValue("");
    await expect(field).toBeFocused();
    expect(await whereIs(page, [WRONG_TOKEN])).toEqual([]);

    // A good one: stored, the card closes, the run goes on.
    await field.fill(FAKE_TOKEN);
    await field.press("Enter");
    await expect(card).toBeHidden();
    await expect(log(page).getByText("Salvato: Token di Hugging Face")).toBeVisible();
    await expect(log(page).getByText("Il token è salvato")).toBeVisible();
    await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();
    expect(await whereIs(page, [FAKE_TOKEN, WRONG_TOKEN])).toEqual([]);

    // Reopened from the stored messages: the same line, and still no value anywhere.
    await sessionList(page).getByRole("button", { name: /^Riordino PDF in Downloads/ }).click();
    await sessionList(page).getByRole("button", { name: /^Aggiungi il token di Hugging Face/ }).click();
    await expect(log(page).getByText("Salvato: Token di Hugging Face")).toBeVisible();
    await expect(log(page).getByRole("form")).toHaveCount(0);
    expect(await whereIs(page, [FAKE_TOKEN, WRONG_TOKEN])).toEqual([]);

  });

  test("«Non ora» closes the request: not provided, and the run goes on without it", async ({ page }) => {
    await chatWith(page, "Aggiungi il token di Hugging Face");
    const card = log(page).getByRole("form", { name: "Token di Hugging Face" });
    await expect(card).toBeVisible();
    await card.getByRole("button", { name: "Non ora" }).click();
    await expect(card).toBeHidden();
    await expect(log(page).getByText("Non fornito: Token di Hugging Face")).toBeVisible();
    await expect(log(page).getByText("Va bene, non aggiungo il token.")).toBeVisible();
  });

  test("a device that may not provide secrets gets no field, only where to do it, and the typed value is gone", async ({ page }) => {
    await chatWith(page, "Aggiungi il token di Hugging Face", { query: "scope=device" });
    const card = log(page).getByRole("form", { name: "Token di Hugging Face" });
    const field = card.getByLabel("Valore di «Token di Hugging Face»");
    await expect(field).toBeVisible();
    await field.fill(FAKE_TOKEN);
    await field.press("Enter");
    await expect(card.getByRole("alert")).toContainText("Questo dispositivo non può inserire valori riservati.");
    await expect(card.getByRole("alert")).toContainText("Inseriscilo dall'app su questo computer.");
    await expect(field).toHaveCount(0);
    await expect(card.getByRole("button", { name: "Salva" })).toHaveCount(0);
    expect(await whereIs(page, [FAKE_TOKEN])).toEqual([]);
    // Nothing was stored: the run still waits, and stopping it ends the request.
    await page.getByRole("button", { name: "Ferma" }).click();
    await expect(log(page).getByText("Richiesta annullata: Token di Hugging Face non è stato fornito.")).toBeVisible();
  });
});
