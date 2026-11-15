import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { whereIs } from "../support/ask";
import { browserButton, clickFrame, dots, frameImage, frameSvg, handoverCard, header, log, openBrowserSession, openHandover, panel, signInThroughViewer, statusLine, viewer, withApi } from "../support/browser";

// The live view of the controlled browser, from the chat (also in the simple interface): the «Browser» button, the panel (a column at 1440, a
// full-screen sheet at 390), pausing the agent, taking the browser to log in, giving it back, the card of a request for help. The demo's browser
// is src/shared/api/mock/browser.ts: a fake page, one dot per character typed, the rules of the engine.

const SIZES = [
  { name: "1440", viewport: { width: 1440, height: 900 } },
  { name: "390", viewport: { width: 390, height: 844 } },
] as const;

/** The typed text must be nowhere: not in the page, the fields, the storage, the caches, the messages, the events. */
const nowhere = async (page: Page, secret: string) => expect(await whereIs(page, [secret])).toEqual([]);

for (const size of SIZES) {
  test.describe(`${size.name} px`, () => {
    test.use({ viewport: size.viewport });

    test("the button is there only while a browser runs; it opens the panel with the page, the state and the focus inside; Esc closes it and the focus goes back", async ({ page }) => {
      await openBrowserSession(page);
      // Before the agent opened a page there was no button (the session was opened without one): now it says who has the browser.
      await expect(browserButton(page)).toHaveAccessibleName("Browser: l'agente lo sta usando");
      await expect(browserButton(page)).toHaveAttribute("aria-expanded", "false");
      await browserButton(page).click();

      const view = panel(page);
      await expect(view).toBeVisible();
      await expect(browserButton(page)).toHaveAttribute("aria-expanded", "true");
      // The focus moved into the panel, to its title (nothing that sends a key to a page).
      await expect(view.getByRole("heading", { name: "Vista del browser" })).toBeFocused();
      await expect(statusLine(view, "L'agente sta usando il browser")).toBeVisible();
      await expect(view.getByText("fornitori.example.com · Preventivi fornitori")).toBeVisible();
      await expect(view.getByText("Connessione sicura (https)")).toBeAttached();
      // The frame has a text alternative with the title and the host; the page is an image, not a control.
      await expect(frameImage(view)).toHaveAttribute("alt", "Pagina «Preventivi fornitori» su fornitori.example.com");
      await expect(view.getByRole("img", { name: "Pagina «Preventivi fornitori» su fornitori.example.com" })).toBeVisible();
      await expect(viewer(view)).toHaveCount(0);
      await expect(view.getByText("Stai solo guardando. Per cliccare o scrivere nella pagina prendi il controllo.")).toBeVisible();
      await expect(view.getByText("Puoi anche usare la finestra del browser sul computer.")).toBeVisible();

      await page.keyboard.press("Escape");
      await expect(view).toBeHidden();
      await expect(browserButton(page)).toBeFocused();
      await expect(browserButton(page)).toHaveAttribute("aria-expanded", "false");
      // And the same button opens it again (a person who only has the keyboard).
      await page.keyboard.press("Enter");
      await expect(panel(page)).toBeVisible();
      await panel(page).getByRole("button", { name: "Chiudi la vista del browser" }).click();
      await expect(panel(page)).toBeHidden();
      await expect(browserButton(page)).toBeFocused();
    });

    test("Pausa agente → «Agente in pausa»; Riprendi agente → the agent again", async ({ page }) => {
      await openBrowserSession(page);
      await browserButton(page).click();
      const view = panel(page);
      await view.getByRole("button", { name: "Pausa agente" }).click();
      await expect(statusLine(view, "Agente in pausa")).toBeVisible();
      await expect(browserButton(page)).toHaveAccessibleName("Browser: agente in pausa");
      await expect(view.getByRole("button", { name: "Pausa agente" })).toHaveCount(0);
      // The same button is now «Riprendi agente», and still has the focus (a screen reader hears the new status).
      await expect(view.getByRole("button", { name: "Riprendi agente" })).toBeFocused();
      await view.getByRole("button", { name: "Riprendi agente" }).click();
      await expect(statusLine(view, "L'agente sta usando il browser")).toBeVisible();
      await expect(view.getByRole("button", { name: "Pausa agente" })).toBeVisible();
    });

    test("Prendi il controllo → the agent waits and «Scrivi nella pagina» appears; Restituisci all'agente → the agent again, and it will read the page again", async ({ page }) => {
      await openBrowserSession(page);
      await browserButton(page).click();
      const view = panel(page);
      await expect(view.getByLabel("Scrivi nella pagina")).toHaveCount(0);
      await view.getByRole("button", { name: "Prendi il controllo" }).click();
      await expect(statusLine(view, "Stai guidando il browser: l'agente aspetta")).toBeVisible();
      await expect(browserButton(page)).toHaveAccessibleName("Browser: lo stai guidando tu");
      await expect(view.getByLabel("Scrivi nella pagina")).toBeVisible();
      await expect(view.getByRole("button", { name: "Pausa agente" })).toBeDisabled();
      await expect(viewer(view)).toBeVisible();
      // The same button is now the way back, and still has the focus.
      await expect(view.getByRole("button", { name: "Restituisci all'agente" })).toBeFocused();
      await view.getByRole("button", { name: "Restituisci all'agente" }).click();
      await expect(statusLine(view, "L'agente sta usando il browser")).toBeVisible();
      await expect(view.getByLabel("Scrivi nella pagina")).toHaveCount(0);
      await expect(view.getByText("L'agente rilegge la pagina prima di agire")).toBeVisible();
    });

    test("a request for help, answered from the panel: what is typed goes to the page and nowhere else; giving the browser back closes the card", async ({ page }) => {
      await openHandover(page);
      // The panel, from the button. The card then points to it instead of showing a second view.
      await browserButton(page).click();
      const view = panel(page);
      await expect(statusLine(view, "Stai guidando il browser: l'agente aspetta")).toBeVisible();
      await expect(view.getByText("L'agente ti chiede di intervenire:")).toBeVisible();
      await expect(view.getByRole("button", { name: "Pausa agente" })).toBeDisabled();
      await expect(view.getByText("va solo alla pagina del sito: l'assistente non lo vede e non viene salvato")).toBeVisible();

      // A virtual keyboard or a password manager: the field and «Invia». It is masked until shown, and cleared when sent.
      const field = view.getByLabel("Scrivi nella pagina");
      await expect(field).toHaveAttribute("type", "password");
      await view.getByRole("button", { name: "Mostra il testo" }).click();
      await expect(field).toHaveAttribute("type", "text");
      await view.getByRole("button", { name: "Nascondi il testo" }).click();
      await expect(field).toHaveAttribute("type", "password");
      await view.getByRole("button", { name: "Tab", exact: true }).click();
      // The control that has the focus in the page is said in a live region (the kind and the label, never a value).
      await expect(view.getByRole("status").filter({ hasText: "Nella pagina è selezionato: campo di testo «Email»" })).toBeVisible();
      const secret = "TEST-ONLY-anna-9f3k2";
      await field.fill(secret);
      await view.getByRole("button", { name: "Invia" }).click();
      await expect(field).toHaveValue("");
      await expect.poll(() => dots(view)).toEqual([secret.length]);
      expect(await frameSvg(view)).not.toContain(secret);
      await nowhere(page, secret);

      // Enter in the field sends too; a key button sends a key (Backspace takes one character away).
      await view.getByRole("button", { name: "Tab", exact: true }).click();
      await expect(view.getByRole("status").filter({ hasText: "campo password «Password»" })).toBeVisible();
      const password = "TEST-ONLY-pw-77";
      await field.fill(password);
      await field.press("Enter");
      await expect(field).toHaveValue("");
      await expect.poll(() => dots(view)).toEqual([secret.length, password.length]);
      await view.getByRole("button", { name: "Cancella" }).click();
      await expect.poll(() => dots(view)).toEqual([secret.length, password.length - 1]);
      await nowhere(page, password);
      await view.getByRole("button", { name: "Invio" }).click();
      await expect.poll(async () => (await frameSvg(view)).includes("Area riservata")).toBe(true);
      await expect(view.getByText("fornitori.example.com · Area riservata · Fornitori")).toBeVisible();

      // Giving it back ends the request: the card closes with its own sentence, and the run goes on.
      await view.getByRole("button", { name: "Restituisci all'agente" }).click();
      await expect(statusLine(view, "L'agente sta usando il browser")).toBeVisible();
      await expect(view.getByLabel("Scrivi nella pagina")).toHaveCount(0);
      await expect(browserButton(page)).toHaveAccessibleName("Browser: l'agente lo sta usando");
      await page.keyboard.press("Escape");
      await expect(view).toBeHidden();
      await expect(log(page).getByText("Hai restituito il browser all'agente.")).toBeVisible();
      await expect(log(page).getByText("Grazie: adesso sono dentro.")).toBeVisible();
      await expect(handoverCard(page)).toHaveCount(0);
      await nowhere(page, secret);
      await nowhere(page, password);
    });

    test("the card of the request: it takes the focus on its text, shows the view with the controls, and «Ho finito» gives the browser back", async ({ page }) => {
      await openHandover(page);
      const card = handoverCard(page);
      // The focus is on the request, never on the viewer or a button (a person typing in the message box must not send keys to a page).
      await expect(card.locator("[data-ask-first]")).toBeFocused();
      await expect(card.getByText("Accedi a fornitori.example.com con la tua email e la tua password.", { exact: true })).toBeVisible();
      await expect(card.getByRole("status").filter({ hasText: "Serve il tuo intervento nel browser: Accedi a fornitori.example.com" })).toBeAttached();
      await expect(browserButton(page)).toHaveAccessibleName("Browser: serve il tuo intervento");

      await expect(statusLine(card, "Stai guidando il browser: l'agente aspetta")).toBeVisible();
      await expect(viewer(card)).toBeVisible();
      await expect(card.getByLabel("Scrivi nella pagina")).toBeVisible();
      // No second button that does what «Ho finito» does, and no generic «Non ora».
      await expect(card.getByRole("button", { name: "Restituisci all'agente" })).toHaveCount(0);
      await expect(card.getByRole("button", { name: "Non ora" })).toHaveCount(0);
      await expect(card.getByRole("button", { name: "Ho finito: restituisci il controllo" })).toBeVisible();
      await expect(card.getByRole("button", { name: "Non posso ora" })).toBeVisible();

      // The code of a second factor, typed with the field of the card: it goes to the page (a dot a character in the demo) and nowhere else.
      const secret = "TEST-ONLY-code-554433";
      await card.getByRole("button", { name: "Tab", exact: true }).click();
      await card.getByLabel("Scrivi nella pagina").fill(secret);
      await card.getByRole("button", { name: "Invia" }).click();
      await expect.poll(() => dots(card)).toEqual([secret.length]);
      await card.getByRole("button", { name: "Ho finito: restituisci il controllo" }).click();
      await expect(card).toBeHidden();
      await expect(log(page).getByText("Hai restituito il browser all'agente.")).toBeVisible();
      await expect(log(page).getByText("Grazie: adesso sono dentro.")).toBeVisible();
      await expect(browserButton(page)).toHaveAccessibleName("Browser: l'agente lo sta usando");
      await nowhere(page, secret);
    });

    test("«Non posso ora» closes the request, gives the browser back, and the agent says what it needs", async ({ page }) => {
      await openHandover(page);
      await handoverCard(page).getByRole("button", { name: "Non posso ora" }).click();
      await expect(handoverCard(page)).toBeHidden();
      await expect(log(page).getByText("Hai detto che non potevi intervenire ora.")).toBeVisible();
      await expect(log(page).getByText("senza l'accesso non posso leggere i preventivi riservati", { exact: false })).toBeVisible();
      await expect(browserButton(page)).toHaveAccessibleName("Browser: l'agente lo sta usando");
      // Another conversation, and back: the conversation is rebuilt from the stored messages, with the same closed row (not a tool card with raw JSON).
      await page.evaluate(() => { location.hash = "/chat/ses_pdf"; });
      await expect(log(page).getByText("Hai detto che non potevi intervenire ora.")).toBeHidden();
      await page.evaluate(() => { location.hash = "/chat/ses_cv"; });
      await expect(log(page).getByText("Hai detto che non potevi intervenire ora.")).toBeVisible();
      await expect(log(page).getByRole("button", { name: /browser_handover/ })).toHaveCount(0);
    });
  });
}

// The panel is a column at 1440, a side drawer at 1000 and a sheet at 390: in each of them the viewer keeps Esc for the page, and Esc elsewhere closes it.
const LAYOUTS = [...SIZES, { name: "1000", viewport: { width: 1000, height: 800 } }] as const;
for (const layout of LAYOUTS) {
  test.describe(`keyboard and pointer in the viewer (${layout.name})`, () => {
    test.use({ viewport: layout.viewport });

    test("the viewer takes clicks, keys and paste; Tab leaves it, Esc is the page's; Esc elsewhere closes the panel", async ({ page }) => {
      await openHandover(page);
      await browserButton(page).click();
      const view = panel(page);
      const page_ = viewer(view);
      await expect(page_).toBeVisible();
      await expect(page_).toHaveAttribute("tabindex", "0");
      await expect(page_).toHaveAttribute("aria-describedby", /.+/);

      // A click on the frame is a click on the page: it lands on the «Email» field (said in the live region) and the keys that follow are typed there.
      await clickFrame(view, 0.5, 0.35);
      await expect(page_).toBeFocused();
      await expect(view.getByRole("status").filter({ hasText: "campo di testo «Email»" })).toBeVisible();
      await page.keyboard.type("abc");
      await expect.poll(() => dots(view)).toEqual([3]);
      await page.keyboard.press("Backspace");
      await expect.poll(() => dots(view)).toEqual([2]);
      // A paste is text for the page.
      await page_.evaluate((el) => {
        const data = new DataTransfer();
        data.setData("text/plain", "1234567890");
        el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
      });
      await expect.poll(() => dots(view)).toEqual([12]);
      // A shortcut stays with the browser: Ctrl+A is not a key of the page.
      await page.keyboard.press("Control+a");
      expect(await dots(view)).toEqual([12]);

      // Esc is for the page while the viewer has the focus: the panel stays.
      await page.keyboard.press("Escape");
      await expect(view).toBeVisible();
      await expect(page_).toBeFocused();
      // Tab leaves the viewer (no keyboard trap) and goes to the keys of the panel; Shift+Tab comes back.
      await page.keyboard.press("Tab");
      await expect(page_).not.toBeFocused();
      await expect(view.getByRole("button", { name: "Tab", exact: true })).toBeFocused();
      await page.keyboard.press("Shift+Tab");
      await expect(page_).toBeFocused();
      await page.keyboard.press("Tab");
      // From there Esc closes the panel, and the focus goes back to the button.
      await page.keyboard.press("Escape");
      await expect(view).toBeHidden();
      await expect(browserButton(page)).toBeFocused();
    });
  });
}

test.describe("keyboard and pointer in the viewer (1440)", () => {
  test("the focus of the viewer is shown", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const page_ = viewer(panel(page));
    await page_.focus();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Tab");
    await expect(page_).toBeFocused();
    expect(await page_.evaluate((el) => getComputedStyle(el).boxShadow)).not.toBe("none");
  });

  test("a window of the site that asks something is answered in the panel, from the keyboard too; the frame keeps the last page meanwhile", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    // Sign in (the demo accepts anything in both fields), then «Esci» in the page asks first.
    await clickFrame(view, 0.5, 0.35);
    await page.keyboard.type("anna@example.com");
    await page.keyboard.press("Tab");
    await view.getByRole("button", { name: "Tab", exact: true }).click();
    await view.getByLabel("Scrivi nella pagina").fill("TEST-ONLY-pw");
    await view.getByRole("button", { name: "Invia" }).click();
    await view.getByRole("button", { name: "Invio" }).click();
    await expect.poll(async () => (await frameSvg(view)).includes("Area riservata")).toBe(true);

    await clickFrame(view, 0.9, 0.9);
    const dialog = view.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Vuoi davvero uscire dall'area riservata?")).toBeVisible();
    // The person is taken to the answer; the page behind it cannot be typed into meanwhile.
    await expect(dialog.getByRole("button", { name: "OK" })).toBeFocused();
    await expect(view.getByText("Una finestra del sito aspetta una risposta: rispondi qui sopra prima di scrivere nella pagina.")).toBeVisible();
    await expect(viewer(view)).toHaveCount(0);
    await expect(frameImage(view)).toBeVisible();
    // Esc is «Annulla» (and does not close the panel).
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(view).toBeVisible();
    expect(await frameSvg(view)).toContain("Area riservata");

    await clickFrame(view, 0.9, 0.9);
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "OK" }).click();
    await expect(dialog).toBeHidden();
    await expect.poll(async () => (await frameSvg(view)).includes("Accedi")).toBe(true);
    await expect(view.getByText("fornitori.example.com · Accedi · Fornitori")).toBeVisible();
  });
});

const nowhereInAttributes = (page: Page, secret: string) =>
  page.evaluate((value) => {
    const found: string[] = [];
    for (const el of document.querySelectorAll("*")) for (const a of el.attributes) if (a.value.includes(value)) found.push(`${el.tagName.toLowerCase()}[${a.name}]`);
    if (document.documentElement.outerHTML.includes(value)) found.push("outerHTML");
    return found;
  }, secret);

test.describe("what is typed, a window of the site, the queue of the keys (1440)", () => {
  test("what is typed in the field of the page is in the field and nowhere else: not in an attribute of the markup, also with the text shown", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    const field = view.getByLabel("Scrivi nella pagina");
    const secret = "TEST-ONLY-typed-77031";
    await field.fill(secret);
    await expect(field).toHaveValue(secret);
    expect(await nowhereInAttributes(page, secret)).toEqual([]);
    // Showing the text changes the field's type only: the text stays, and is still in no attribute.
    await view.getByRole("button", { name: "Mostra il testo" }).click();
    await expect(field).toHaveAttribute("type", "text");
    await expect(field).toHaveValue(secret);
    expect(await nowhereInAttributes(page, secret)).toEqual([]);
    await view.getByRole("button", { name: "Nascondi il testo" }).click();
    await expect(field).toHaveAttribute("type", "password");
    // The toggle says its state in its name, not also in aria-pressed.
    await expect(view.getByRole("button", { name: "Mostra il testo" })).not.toHaveAttribute("aria-pressed", /.*/);
    await view.getByRole("button", { name: "Tab", exact: true }).click();
    await view.getByRole("button", { name: "Invia" }).click();
    await expect(field).toHaveValue("");
    await nowhere(page, secret);
    expect(await nowhereInAttributes(page, secret)).toEqual([]);
  });

  test("a window answered twice: the second answer is refused with 409 and says so; the «Browser» button and the panel stay", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    await signInThroughViewer(page, view);
    await clickFrame(view, 0.9, 0.9);
    const dialog = view.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    // The window is answered elsewhere (the real window of the browser) just before the click on «OK»: the view still shows it.
    await withApi(page, async (api) => {
      await api.browserDialog({ action: "dismiss" } as never);
      for (const b of document.querySelectorAll('[role="alertdialog"] button')) if (b.textContent?.trim() === "OK") (b as HTMLButtonElement).click();
    });
    const alert = view.getByRole("alert").filter({ hasText: "nessuna finestra del sito aspetta una risposta" });
    await expect(alert).toBeVisible();
    await expect(dialog).toBeHidden();
    // Nothing is hidden: the panel, the button, the state of the browser.
    await expect(view).toBeVisible();
    await expect(browserButton(page)).toBeVisible();
    await expect(statusLine(view, "Stai guidando il browser: l'agente aspetta")).toBeVisible();
    await expect(viewer(view)).toBeVisible();
    // An error is read once: it goes by itself (after a few seconds).
    await expect(alert).toBeHidden({ timeout: 12_000 });
  });

  test("a 404 from the answer to a window does not hide the browser either: its message, the state asked again, the panel and the button stay", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    await signInThroughViewer(page, view);
    await clickFrame(view, 0.9, 0.9);
    const dialog = view.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    await withApi(page, (api) => {
      api.browserDialog = async () => { throw Object.assign(new Error("finestra non trovata"), { status: 404 }); };
    });
    await dialog.getByRole("button", { name: "OK" }).click();
    await expect(view.getByRole("alert").filter({ hasText: "finestra non trovata" })).toBeVisible();
    // Not for one moment, and not after the next poll of the state either.
    await expect(browserButton(page)).toBeVisible();
    await page.waitForTimeout(2_600);
    await expect(browserButton(page)).toBeVisible();
    await expect(view).toBeVisible();
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("button", { name: "OK" })).toBeEnabled();
  });

  test("while an answer to a window is on its way «OK» and «Annulla» are off, and a double click sends one answer", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    await signInThroughViewer(page, view);
    await clickFrame(view, 0.9, 0.9);
    const dialog = view.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    await withApi(page, (api) => {
      const w = window as unknown as { __answers: number };
      w.__answers = 0;
      const original = api.browserDialog.bind(api);
      api.browserDialog = async (reply: never) => {
        w.__answers++;
        await new Promise((r) => setTimeout(r, 1_200));
        return original(reply);
      };
    });
    await dialog.getByRole("button", { name: "OK" }).dblclick();
    await expect(dialog.getByRole("button", { name: "OK" })).toBeDisabled();
    await expect(dialog.getByRole("button", { name: "Annulla" })).toBeDisabled();
    await expect(dialog).toBeHidden();
    expect(await page.evaluate(() => (window as unknown as { __answers: number }).__answers)).toBe(1);
    await expect(browserButton(page)).toBeVisible();
    await expect(view.getByRole("alert")).toHaveCount(0);
  });

  test("the frames stop while a window of the site waits (the last one stays) and go on when it is gone", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    await signInThroughViewer(page, view);
    await withApi(page, (api) => {
      const w = window as unknown as { __frames: number };
      w.__frames = 0;
      const original = api.browserFrame.bind(api);
      api.browserFrame = async () => {
        w.__frames++;
        return original();
      };
    });
    const frames = () => page.evaluate(() => (window as unknown as { __frames: number }).__frames);
    await clickFrame(view, 0.9, 0.9);
    const dialog = view.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    await page.waitForTimeout(500);
    const during = await frames();
    await page.waitForTimeout(3_000);
    // At most the frame that found the window out (a 409); not one every half second.
    expect(await frames() - during).toBe(0);
    expect(during).toBeLessThanOrEqual(4);
    await expect(frameImage(view)).toBeVisible();
    expect(await frameSvg(view)).toContain("Area riservata");
    await dialog.getByRole("button", { name: "Annulla" }).click();
    await expect(dialog).toBeHidden();
    await expect.poll(frames, { timeout: 5_000 }).toBeGreaterThan(during + 2);
  });

  test("a window of the site does not take the focus from the message box: it is announced, and the focus stays where the person is typing", async ({ page }) => {
    await openHandover(page);
    const card = handoverCard(page);
    await signInThroughViewer(page, card);
    const composer = page.locator("#composer");
    await composer.click();
    await composer.fill("scrivo una cosa");
    await expect(composer).toBeFocused();
    // The window opens without a click of the person in the view (the real page did it, say).
    await withApi(page, (api) => api.browserInput({ type: "click", x: 0.9, y: 0.9, button: "left", count: 1 } as never));
    const dialog = card.getByRole("alertdialog", { name: "Il sito chiede una conferma" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("status")).toHaveText("Il sito chiede una conferma: Vuoi davvero uscire dall'area riservata?");
    await page.waitForTimeout(400);
    await expect(composer).toBeFocused();
    await expect(composer).toHaveValue("scrivo una cosa");
    await expect(dialog.getByRole("button", { name: "OK" })).not.toBeFocused();
    // Answered from the dialog, it works as ever.
    await dialog.getByRole("button", { name: "Annulla" }).click();
    await expect(dialog).toBeHidden();
  });

  test("«Ho finito» waits for the keys still on their way: the last ones are in the page before it goes back", async ({ page }) => {
    await openHandover(page);
    const card = handoverCard(page);
    await withApi(page, (api) => {
      const calls: string[] = [];
      (window as unknown as { __calls: string[] }).__calls = calls;
      const input = api.browserInput.bind(api);
      api.browserInput = async (i: { type: string }) => {
        await new Promise((r) => setTimeout(r, 300));
        calls.push(`input:${i.type}`);
        return input(i as never);
      };
      const answer = api.answerQuestion.bind(api);
      api.answerQuestion = (...a: never[]) => {
        calls.push("answer");
        return answer(...a);
      };
    });
    await clickFrame(card, 0.5, 0.35);
    await page.keyboard.type("abc");
    await card.getByRole("button", { name: "Ho finito: restituisci il controllo" }).click();
    await expect(card).toBeHidden();
    const calls = await page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);
    expect(calls.filter((c) => c === "answer")).toHaveLength(1);
    expect(calls[calls.length - 1]).toBe("answer");
    expect(calls.filter((c) => c === "input:key" || c === "input:click")).toHaveLength(4);
  });

  test("a finger dragging on the page is one request with the sum, not one for every move", async ({ page }) => {
    await openHandover(page);
    await browserButton(page).click();
    const view = panel(page);
    await expect.poll(() => frameImage(view).evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    await withApi(page, (api) => {
      const scrolls: number[] = [];
      (window as unknown as { __scrolls: number[] }).__scrolls = scrolls;
      const input = api.browserInput.bind(api);
      api.browserInput = (i: { type: string; dy?: number }) => {
        if (i.type === "scroll") scrolls.push(i.dy ?? 0);
        return input(i as never);
      };
    });
    const expected = await viewer(view).evaluate((el) => {
      const img = el.querySelector("img")!;
      const r = img.getBoundingClientRect();
      const touch = (type: string, y: number) =>
        el.dispatchEvent(new PointerEvent(type, { pointerType: "touch", pointerId: 7, clientX: r.left + r.width / 2, clientY: y, bubbles: true, cancelable: true }));
      touch("pointerdown", r.top + 120);
      // Twelve moves of 8 px, one after the other, without a pause: 96 px of the screen.
      for (let i = 1; i <= 12; i++) touch("pointermove", r.top + 120 - i * 8);
      touch("pointerup", r.top + 24);
      return { page: 96 * (img.naturalWidth || 1000) / r.width };
    });
    await expect.poll(() => page.evaluate(() => (window as unknown as { __scrolls: number[] }).__scrolls.length)).toBeGreaterThan(0);
    await page.waitForTimeout(400);
    const scrolls = await page.evaluate(() => (window as unknown as { __scrolls: number[] }).__scrolls);
    expect(scrolls.length, "a request for each move would be 12").toBeLessThanOrEqual(2);
    const sum = scrolls.reduce((a, b) => a + b, 0);
    expect(Math.abs(sum - expected.page)).toBeLessThanOrEqual(expected.page * 0.03 + 2);
  });
});

test.describe("a device that may not see the browser (1440)", () => {
  test("a paired device (`?scope=device`) does not see the button or the panel; the request still shows its card, with the way to go on", async ({ page }) => {
    await openHandover(page, { query: "scope=device" });
    await expect(browserButton(page)).toHaveCount(0);
    const card = handoverCard(page);
    await expect(card.getByText("Da questo dispositivo non posso mostrarti il browser.", { exact: false })).toBeVisible();
    await expect(viewer(card)).toHaveCount(0);
    await expect(header(page).getByRole("button", { name: /^Browser/ })).toHaveCount(0);
    await card.getByRole("button", { name: "Ho finito: restituisci il controllo" }).click();
    await expect(log(page).getByText("Hai restituito il browser all'agente.")).toBeVisible();
  });
});

test.describe("developer interface and English", () => {
  test("the button and the panel are in the developer interface too", async ({ page }) => {
    await openBrowserSession(page, { mode: "developer" });
    await browserButton(page).click();
    await expect(statusLine(panel(page), "L'agente sta usando il browser")).toBeVisible();
  });

  test.describe("English", () => {
    test.use({ locale: "en-US" });
    test("the texts follow the language", async ({ page }) => {
      await openHandover(page);
      await expect(page.locator("html")).toHaveAttribute("lang", "en");
      const card = handoverCard(page);
      // The title and the buttons are the interface's own, in its language (the engine's are always Italian); the request itself is the agent's words.
      await expect(card.getByText("Your help is needed in the browser", { exact: true })).toBeVisible();
      await expect(card.getByRole("button", { name: "I'm done: give back control" })).toBeVisible();
      await expect(card.getByRole("button", { name: "I can't right now" })).toBeVisible();
      await expect(card.getByRole("button", { name: "Ho finito: restituisci il controllo" })).toHaveCount(0);
      await expect(card.getByText("You are driving the browser: the agent waits")).toBeVisible();
      const button = header(page).getByRole("button", { name: /^Browser: your help is needed$/ });
      await expect(button).toBeVisible();
      await button.click();
      const view = panel(page).first();
      await expect(view.getByRole("heading", { name: "Browser view" })).toBeVisible();
      await expect(view.getByText("You are driving the browser: the agent waits")).toBeVisible();
      await expect(view.getByRole("button", { name: "Give back to the agent" })).toBeVisible();
      await expect(view.getByLabel("Type into the page")).toBeVisible();
      await expect(view.getByText("What you type here (passwords, codes) goes only to the website's page")).toBeVisible();
      // With the panel open the card points to it, and names its button as the button is named.
      if ((page.viewportSize()?.width ?? 0) >= 1320) {
        await expect(card.getByText("press “I'm done: give back control”", { exact: false })).toBeVisible();
        await expect(card.getByRole("button", { name: "I'm done: give back control" })).toBeVisible();
      }
    });
  });
});
