import { expect, openApp, test } from "../support/app";
import { chatWith, FAKE_TOKEN, whereIs } from "../support/ask";
import { appLinks, conversation, FAKE_KEY, goSimpleWithPalette, linksOutsideChat, messageBox, startDownloadThenGoSimple } from "../support/simple";

// The simple interface is one chat: every other address leads to it, nothing in it leads to a hidden section, and what used to
// be done in those sections is done from the chat (the assistant) or from its menu. Developer mode stays as it was.

const CHAT_URL = /#\/chat(\/ses_\w+)?$/;

test.describe("addresses", () => {
  // Every kind of address: sections, ones with parameters, old hashes (`#tools`, `#preferences`) and one that does not exist.
  const addresses = ["/", "/settings", "/models/discover", "/runtimes/llama-server", "/security/policy", "/teach", "/agents/coder", "/documents", "/inbox", "/devices", "tools", "preferences", "runtimes", "/questa-pagina-non-esiste"];
  for (const address of addresses) {
    test(`${address} leads to the chat`, async ({ page }) => {
      await openApp(page, address);
      await expect(page).toHaveURL(CHAT_URL);
      await expect(messageBox(page)).toBeVisible();
    });
  }

  test("a session of the chat is kept, also after a reload", async ({ page }) => {
    await openApp(page, "/chat/ses_pdf");
    await expect(page).toHaveURL(/#\/chat\/ses_pdf$/);
    await page.reload();
    await expect(messageBox(page)).toBeVisible();
    await expect(page).toHaveURL(/#\/chat\/ses_pdf$/);
  });
});

test.describe("the mode is the configuration's, not the one kept on the device", () => {
  /** Records every change of the address, so that a redirect that lasts a moment is seen too. */
  const recordHashes = (page: import("@playwright/test").Page, stored?: "simple" | "developer") =>
    page.addInitScript((value) => {
      const w = window as unknown as { __hashes: string[]; __stored: string | null };
      w.__hashes = [];
      if (value) localStorage.setItem("aiw.ui.mode", value);
      w.__stored = localStorage.getItem("aiw.ui.mode");
      window.addEventListener("hashchange", () => w.__hashes.push(location.hash));
    }, stored);
  const hashes = (page: import("@playwright/test").Page) => page.evaluate(() => (window as unknown as { __hashes: string[] }).__hashes);

  test("Developer in the configuration and no mode kept on the device: #/runtimes opens and is never redirected", async ({ page }) => {
    await recordHashes(page);
    await openApp(page, "/runtimes", { mode: "developer" });
    await expect(page.getByRole("heading", { name: "Runtime", level: 1 })).toBeVisible();
    expect(await page.evaluate(() => (window as unknown as { __stored: string | null }).__stored), "nothing was kept on the device").toBeNull();
    expect(await hashes(page), "the address never changed").toEqual([]);
    await expect(page).toHaveURL(/#\/runtimes$/);
    // The shell follows the configuration: the device now keeps the mode.
    await expect.poll(() => page.evaluate(() => localStorage.getItem("aiw.ui.mode"))).toBe("developer");
  });

  test("Developer in the configuration and `simple` kept on the device (stale): still opens, never redirected", async ({ page }) => {
    await recordHashes(page, "simple");
    await openApp(page, "/security/hooks", { mode: "developer" });
    await expect(page.getByRole("heading", { name: "Sicurezza", level: 1 })).toBeVisible();
    expect(await hashes(page)).toEqual([]);
    await expect(page).toHaveURL(/#\/security\/hooks$/);
  });

  test("Simple in the configuration and `developer` kept on the device (changed elsewhere): the hidden address leads to the chat", async ({ page }) => {
    await recordHashes(page, "developer");
    await openApp(page, "/runtimes");
    await expect(page).toHaveURL(CHAT_URL);
    await expect(messageBox(page)).toBeVisible();
    await expect.poll(() => page.evaluate(() => localStorage.getItem("aiw.ui.mode"))).toBe("simple");
  });

  test("all 19 routes of the developer interface still open as they did", async ({ page }) => {
    await openApp(page, "/", { mode: "developer" });
    const nav = page.getByRole("navigation", { name: "Sezioni" });
    // Every section of the rail, by its name in developer mode.
    for (const name of ["Panoramica", "Workspace", "Proposte", "Agenti", "Insegna", "Documenti", "Canali", "Team", "Workflow", "Valutazioni", "Tool, MCP e browser", "Integrazioni", "Modelli", "Runtime", "Dispositivi", "Addestramento", "Sicurezza", "Attività", "Impostazioni"]) {
      // A badge (the count of proposals) can follow the name.
      await expect(nav.getByRole("link", { name: new RegExp(`^${name}`) }), name).toBeVisible();
    }
    await expect(nav.locator("a.nav-item")).toHaveCount(19);
    // The device is a link to its page, as it always was in this mode.
    await expect(nav.getByRole("link", { name: /^Dispositivo corrente/ })).toBeVisible();
  });
});

test.describe("nothing leads to a hidden section", () => {
  test("the conversation, the menu, the palette and the notes of a failed session link to the chat only", async ({ page }) => {
    await openApp(page, "/chat/ses_bug");
    await expect(conversation(page).getByText("Errore del runtime", { exact: true })).toBeVisible();
    expect(linksOutsideChat(await appLinks(page))).toEqual([]);

    await page.getByRole("navigation", { name: "Sezioni" }).getByRole("button", { name: "Altro" }).click();
    await expect(page.getByRole("dialog", { name: "Altro" })).toBeVisible();
    expect(linksOutsideChat(await appLinks(page)), "menu").toEqual([]);
    await page.keyboard.press("Escape");

    await page.keyboard.press("Control+k");
    await expect(page.getByRole("dialog", { name: "Cerca e comandi" })).toBeVisible();
    expect(linksOutsideChat(await appLinks(page)), "palette").toEqual([]);
  });

  test("the device in the rail is text, not a link, and there is no section list", async ({ page }) => {
    await openApp(page, "/chat");
    const rail = page.getByRole("navigation", { name: "Sezioni" });
    await expect(rail.getByRole("link")).toHaveCount(0);
    // The rail is calm: no device card, no kill switch; everything else is behind «Altro».
    await expect(rail.locator(".device-chip")).toHaveCount(0);
    await expect(rail.getByRole("button", { name: "Kill switch" })).toHaveCount(0);
    await expect(rail.getByRole("button", { name: "Altro" })).toBeVisible();
    await rail.getByRole("button", { name: "Altro" }).click();
    const menu = page.getByRole("dialog", { name: "Altro" });
    await expect(menu.locator(".device-chip")).toContainText("Studio-PC");
    await expect(menu.getByRole("button", { name: /Kill switch/ })).toBeVisible();
  });

  test("the first-run panel is in the chat and leads nowhere else", async ({ page }) => {
    await openApp(page, "/chat", { scenario: "first-run" });
    await expect(page.getByRole("heading", { name: "Per iniziare" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Prepara tutto" })).toBeVisible();
    expect(linksOutsideChat(await appLinks(page))).toEqual([]);
    // Without a session the message box is off, and the sessions list says so without sending anyone to an agent chooser.
    await expect(messageBox(page)).toBeDisabled();
  });
});

test.describe("notes of the conversation", () => {
  test("a failed run: «Chiedi all'assistente» types a sentence in the message box, «Mostra tutte le sezioni» opens the runtimes", async ({ page }) => {
    await openApp(page, "/chat/ses_bug");
    const log = conversation(page);
    // The session is the Coder's: its name is in the header (the Assistente's is not, it is the one chat).
    await expect(page.getByRole("list", { name: "Come lavora questa sessione" }).getByText("Coder")).toBeVisible();
    const ask = log.getByRole("button", { name: /^Chiedi all'assistente/ });
    await expect(ask).toBeVisible();
    await ask.click();
    await expect(messageBox(page)).toHaveValue(/controlla perché il motore non funziona/);
    await expect(messageBox(page)).toBeFocused();

    await log.getByRole("button", { name: /^Mostra tutte le sezioni/ }).click();
    await expect(page).toHaveURL(/#\/runtimes$/);
    await expect(page.getByRole("heading", { name: "Runtime", level: 1 })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Sezioni" }).getByRole("link", { name: "Sicurezza" })).toBeVisible();
  });

  test("a lesson to review opens in a side panel, not in the Panoramica", async ({ page }) => {
    await openApp(page, "/chat/ses_bug");
    const open = page.getByRole("button", { name: /^Rivedi la lezione/ });
    await expect(open).toBeVisible();
    await expect(page.getByRole("link", { name: /Rivedila nella Panoramica/ })).toHaveCount(0);
    await open.click();
    const panel = page.getByRole("dialog", { name: "Lezioni da rivedere" });
    await expect(panel.getByRole("heading", { name: "Correggere un test del parser GGUF" })).toBeVisible();
    expect(linksOutsideChat(await appLinks(page))).toEqual([]);
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden();
    await expect(open).toBeFocused();
  });

  test("«Rifletti» from the list: the toast sends to the conversation, not to the Panoramica that is not there", async ({ page }) => {
    await openApp(page, "/chat/ses_pdf");
    const sessions = page.getByRole("navigation", { name: "Elenco delle sessioni" });
    await sessions.getByRole("listitem").filter({ hasText: "Riordino PDF in Downloads" }).getByRole("button", { name: "Rifletti su questa conversazione" }).click();
    const toast = page.getByRole("status").filter({ hasText: "Ho proposto una lezione" });
    await expect(toast).toHaveText("Ho proposto una lezione: la trovi nella conversazione, con «Vuoi che lo ricordi?»");
    await expect(toast).not.toContainText("Panoramica");
    // …and the conversation does have it: the card of the memory.
    await expect(page.getByRole("group", { name: "Vuoi che lo ricordi?" })).toBeVisible();
  });

  test("«Rifletti» in Sviluppatore: the toast still points to the Panoramica", async ({ page }) => {
    await openApp(page, "/chat/ses_pdf", { mode: "developer" });
    const sessions = page.getByRole("navigation", { name: "Elenco delle sessioni" });
    await sessions.getByRole("listitem").filter({ hasText: "Riordino PDF in Downloads" }).getByRole("button", { name: "Rifletti su questa conversazione" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Ho proposto una lezione" })).toHaveText("Ho proposto una lezione: la trovi nella Panoramica");
  });

  test("the session is the Assistente's: no agent in the header, the model as text", async ({ page }) => {
    await openApp(page, "/chat");
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    const meta = page.getByRole("list", { name: "Come lavora questa sessione" });
    await expect(meta.getByText("Approvazione")).toBeVisible();
    await expect(meta.getByText("Assistente")).toHaveCount(0);
    await expect(meta.getByRole("link")).toHaveCount(0);
  });
});

test.describe("the assistant from the chat", () => {
  test("«Scarica un modello» from the palette types the request, and sending it makes the assistant ask which one", async ({ page }) => {
    await openApp(page, "/chat");
    await page.keyboard.press("Control+k");
    await page.getByRole("combobox", { name: "Cerca una sezione o un comando" }).fill("scarica");
    await page.keyboard.press("Enter");
    await expect(messageBox(page)).toHaveValue("Scarica un modello adatto a questo computer");
    await expect(messageBox(page)).toBeFocused();
    await messageBox(page).press("Enter");
    await expect(conversation(page).getByRole("form", { name: "Quale modello?" })).toBeVisible();
  });

  test("a new session is the Assistente's and the scripted requests work from it: the question, and the protected field for a token", async ({ page }) => {
    // «Nuova» in the sessions list: no agent is chosen, the session is the Assistente's.
    await chatWith(page, "Scarica un modello");
    const list = page.getByRole("navigation", { name: "Elenco delle sessioni" });
    // (Its title is the first message once the list is refreshed; the agent's name is in the row either way.)
    await expect(list.getByRole("button", { name: /^(Nuova sessione|Scarica un modello).*Assistente/ })).toBeVisible();
    const card = conversation(page).getByRole("form", { name: "Quale modello?" });
    await expect(card).toBeVisible();
    // The cards of the assistant (and the run panel) lead to the chat only, like everything else here.
    expect(linksOutsideChat(await appLinks(page)), "question").toEqual([]);
    await card.getByRole("button", { name: "Non ora" }).click();
    await expect(conversation(page).getByText("Va bene, non scarico nulla.")).toBeVisible();
    // The closing text arrives before the end of the run, and a message sent while the run is still going is dropped without
    // a word (`ChatView.send`): wait until the run is over (no «Ferma», «Invia» is back) before writing the next one.
    await expect(page.getByRole("button", { name: "Ferma" })).toBeHidden();
    await expect(page.getByRole("button", { name: "Invia" })).toBeVisible();

    await messageBox(page).fill("Aggiungi il token di Hugging Face");
    await messageBox(page).press("Enter");
    const secret = conversation(page).getByRole("form", { name: "Token di Hugging Face" });
    await expect(secret).toBeVisible();
    expect(linksOutsideChat(await appLinks(page)), "secret request").toEqual([]);
    const field = secret.getByLabel("Valore di «Token di Hugging Face»");
    await expect(field).toHaveAttribute("type", "password");
    await field.fill(FAKE_TOKEN);
    await field.press("Enter");
    await expect(conversation(page).getByText("Salvato: Token di Hugging Face")).toBeVisible();
    await expect(secret).toBeHidden();
    // The value is nowhere: not in the page, the storage, the caches, the messages of the demo (the detector is proved in ask.spec).
    expect(await whereIs(page, [FAKE_TOKEN])).toEqual([]);
  });

  for (const size of [{ name: "1440", width: 1440, height: 900 }, { name: "390", width: 390, height: 844 }]) {
    test(`${size.name}: the run panel, open, leads to the chat only`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await openApp(page, "/chat/ses_pdf");
      const toggle = page.getByRole("button", { name: "Pannello del run" });
      // Open on a wide screen to begin with, a sheet on a phone that has to be asked for.
      if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
      await expect(toggle).toHaveAttribute("aria-pressed", "true");
      await expect(page.getByRole("complementary", { name: "Pannello del run" }).or(page.getByRole("dialog", { name: "Pannello del run" }))).toBeVisible();
      expect(linksOutsideChat(await appLinks(page))).toEqual([]);
    });
  }

  test("a suggestion for looking after the app types its sentence in the message box", async ({ page }) => {
    await openApp(page, "/chat");
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    const ideas = page.getByRole("list", { name: "Idee per iniziare" });
    await expect(ideas.getByRole("button")).toHaveCount(6);
    await ideas.getByRole("button", { name: "Che cosa sai di me?" }).click();
    await expect(messageBox(page)).toHaveValue("Che cosa sai di me?");
  });
});

test.describe("email drafts of the assistant", () => {
  test("a strip above the message box opens the drafts to review, which are the same cards as in Canali, and sends nothing by itself", async ({ page }) => {
    await openApp(page, "/chat", { scenario: "mail" });
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    const strip = page.getByRole("region", { name: "Bozze di posta da rivedere" });
    // Two drafts wait for the owner and one failed to send: three need a decision, the sent one does not.
    await expect(strip.getByRole("button", { name: /3 bozze di posta da rivedere/ })).toBeVisible();
    await strip.getByRole("button", { name: /bozze di posta da rivedere/ }).click();
    const sheet = page.getByRole("dialog", { name: "Bozze di posta da rivedere" });
    await expect(sheet).toBeVisible();
    await expect(sheet.getByText("Nulla è partito").or(sheet.getByText("non è partito nulla")).first()).toBeVisible();
    await expect(sheet.getByRole("article", { name: /Preventivo per il cantiere/ })).toBeVisible();
    await expect(sheet.getByRole("button", { name: "Approva e invia" }).first()).toBeVisible();
    expect(linksOutsideChat(await appLinks(page)), "drafts").toEqual([]);
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
  });

  test("with no draft waiting there is no strip", async ({ page }) => {
    await openApp(page, "/chat");
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    await expect(messageBox(page)).toBeVisible();
    await expect(page.getByRole("region", { name: "Bozze di posta da rivedere" })).toHaveCount(0);
  });
});

test.describe("a secret typed in the message box", () => {
  test("raises a warning that is announced, offers the protected field and never blocks sending", async ({ page }) => {
    // The protected field is a tool of the Assistente: in its session (a new one) the button is there.
    await openApp(page, "/chat");
    await page.getByRole("button", { name: "Nuova", exact: true }).click();
    // The new session is on screen (its empty state): typing before that goes into the box of the session that was open,
    // and the draft is reset when the view switches to the new one.
    await expect(page.getByRole("list", { name: "Idee per iniziare" })).toBeVisible();
    const box = messageBox(page);
    await expect(box).toBeEnabled();
    const warning = page.getByRole("status").filter({ hasText: "Sembra una chiave o un token" });
    await expect(warning).toHaveCount(0);

    await box.fill(`Usa questa chiave ${FAKE_KEY} per il servizio`);
    await expect(warning).toBeVisible();
    await expect(warning).toContainText("Chiedi all'assistente di usare il campo protetto");
    await expect(page.getByRole("button", { name: "Invia" })).toBeEnabled();

    // It follows the text: gone as soon as the key is.
    await box.fill("Usa la chiave del servizio");
    await expect(warning).toHaveCount(0);

    // The button takes the secret out and puts a request for the protected field in its place.
    await box.fill(FAKE_KEY);
    await page.getByRole("button", { name: "Togli il testo e chiedi il campo protetto" }).click();
    await expect(box).toHaveValue(/chiedimelo con il campo protetto/);
    await expect(warning).toHaveCount(0);
    await expect(box).not.toHaveValue(new RegExp(FAKE_KEY));

    // Not blocked: the same text goes out when the person decides so.
    await box.fill(`Ecco la chiave ${FAKE_KEY}`);
    await expect(warning).toBeVisible();
    await box.press("Enter");
    await expect(conversation(page).getByText(`Ecco la chiave ${FAKE_KEY}`)).toBeVisible();
  });

  for (const session of [{ id: "ses_pdf", agent: "Operatore" }, { id: "ses_bug", agent: "Coder" }]) {
    test(`in a session of the ${session.agent}, which cannot ask for the protected field: the warning stays, the button does not come`, async ({ page }) => {
      await openApp(page, `/chat/${session.id}`);
      await expect(page.getByRole("list", { name: "Come lavora questa sessione" }).getByText(session.agent)).toBeVisible();
      const box = messageBox(page);
      const warning = page.getByRole("status").filter({ hasText: "Sembra una chiave o un token" });
      await box.fill(`Usa questa chiave ${FAKE_KEY} per il servizio`);
      await expect(warning).toBeVisible();
      await expect(warning).toContainText("Chiedi all'assistente di usare il campo protetto");
      await expect(page.getByRole("button", { name: "Togli il testo e chiedi il campo protetto" })).toHaveCount(0);
      // Not blocked either.
      await expect(page.getByRole("button", { name: "Invia" })).toBeEnabled();
    });
  }

  test("developer mode stays as it was: no warning and no extra space, for a person who types a key there", async ({ page }) => {
    await openApp(page, "/chat/ses_pdf", { mode: "developer" });
    await messageBox(page).fill(`Usa questa chiave ${FAKE_KEY} per il servizio`);
    await expect(page.getByRole("status").filter({ hasText: "Sembra una chiave o un token" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Togli il testo e chiedi il campo protetto" })).toHaveCount(0);
    await expect(page.locator(".ws-secret")).toHaveCount(0);
    // Nothing blocks sending either.
    await expect(page.getByRole("button", { name: "Invia" })).toBeEnabled();
  });

  test("an ordinary message raises nothing", async ({ page }) => {
    await openApp(page, "/chat/ses_pdf");
    await messageBox(page).fill("Riordina i PDF in Downloads, ho pulito il disk-usage-analyzer-tool-vecchio");
    await expect(page.getByRole("status").filter({ hasText: "Sembra una chiave o un token" })).toHaveCount(0);
  });
});

test.describe("downloads", () => {
  test("a download going on shows in a strip above the message box, with its progress, and the strip goes away when it is done", async ({ page }) => {
    await openApp(page, "/models/catalog", { mode: "developer" });
    await startDownloadThenGoSimple(page);
    const strip = page.getByRole("region", { name: "Download in corso" });
    await expect(strip).toBeVisible();
    await expect(strip.getByText(/^Scarico /)).toBeVisible();
    const meter = strip.getByRole("meter", { name: /^Download di / });
    const now = async () => Number(await meter.getAttribute("aria-valuenow"));
    const first = await now();
    await expect.poll(now).toBeGreaterThan(first);
    // Nothing in the strip is a live region: a percentage read out at every step would drown the chat.
    await expect(strip.locator("[aria-live], [role=status], [role=alert]")).toHaveCount(0);
    await expect(strip).toBeHidden({ timeout: 30_000 });
    // The toast that says it is done is plain text: nothing to follow to the Modelli page.
    await expect(page.getByText(/Download completato/)).toBeVisible();
    expect(linksOutsideChat(await appLinks(page))).toEqual([]);
  });

  test("with no download there is no strip", async ({ page }) => {
    await openApp(page, "/chat");
    await expect(page.getByRole("region", { name: "Download in corso" })).toHaveCount(0);
  });
});

test.describe("update notice", () => {
  test("in the rail it opens the updater in a dialog (the settings page is not there); in developer mode it is a link to the settings", async ({ page }) => {
    // The demo finds a new version when asked to check; the notice is in the rail as soon as there is one.
    await openApp(page, "/settings", { mode: "developer" });
    await page.getByRole("button", { name: "Controlla ora" }).click();
    const rail = page.getByRole("navigation", { name: "Sezioni" });
    await expect(rail.getByRole("link", { name: /^Aggiornamento disponibile/ })).toHaveAttribute("href", "#/settings");

    await goSimpleWithPalette(page);
    const notice = rail.getByRole("button", { name: /^Aggiornamento disponibile/ });
    await expect(rail.getByRole("link", { name: /^Aggiornamento disponibile/ })).toHaveCount(0);
    await notice.click();
    const dialog = page.getByRole("dialog", { name: "Aggiornamento disponibile" });
    await expect(dialog.getByText("Versione 0.5.0-beta.3")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Controlla ora" })).toBeVisible();
    // Still the chat behind it, and Esc gives the focus back to the notice.
    await expect(page).toHaveURL(CHAT_URL);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(notice).toBeFocused();
  });
});
