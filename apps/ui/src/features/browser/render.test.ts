import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ApiProvider } from "../../shared/api/context";
import { MockTransport } from "../../shared/api/mock";
import type { BrowserState, Question } from "../../shared/api/types";
import { qk } from "../../shared/query/keys";
import { BrowserLive } from "./BrowserLive";
import { HandoverCard } from "./HandoverCard";
import { BrowserUiContext } from "./context";

// The markup of the live view (the project has no DOM test environment: this checks the structure and the attributes the keyboard and
// screen-reader behaviour stand on; the behaviour itself — keys, clicks, frames — is in the end-to-end and accessibility tests).
const state = (s: Partial<BrowserState> = {}): BrowserState => ({
  running: true, headless: false, mode: "agent", agent_busy: false, stale: false, handover: null,
  page: { title: "Accedi · Fornitori", host: "fornitori.example.com", secure: true }, dialog: null, focus: null, ...s,
});

const render = (el: ReactElement, seed?: BrowserState | null) => {
  const client = new QueryClient();
  if (seed !== undefined) client.setQueryData(qk.browser(), seed);
  return renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(ApiProvider, { api: new MockTransport(), children: el })));
};
const live = (s: Partial<BrowserState> = {}, variant: "panel" | "card" = "panel", hideGiveBack = false) => render(createElement(BrowserLive, { state: state(s), variant, hideGiveBack }));

describe("BrowserLive markup", () => {
  it("says who drives the browser in a polite status line, and where the person is", () => {
    const html = live();
    expect(html).toMatch(/<p[^>]*class="bl-status is-agent"[^>]*role="status"[^>]*>L&#x27;agente sta usando il browser<\/p>/);
    expect(html).toContain("fornitori.example.com · Accedi · Fornitori");
    expect(html).toContain("Connessione sicura (https)");
    expect(live({ mode: "paused" })).toContain(">Agente in pausa<");
    expect(live({ mode: "human" })).toContain(">Stai guidando il browser: l&#x27;agente aspetta<");
  });

  it("while the agent drives: Pausa and Prendi il controllo; the page is not a control", () => {
    const html = live();
    expect(html).toContain('role="group" aria-label="Comandi del browser"');
    expect(html).toContain("Pausa agente");
    expect(html).toContain("Prendi il controllo");
    expect(html).not.toContain("Restituisci all&#x27;agente");
    expect(html).not.toContain('role="application"');
    expect(html).not.toContain("Scrivi nella pagina");
    expect(html).toContain("Stai solo guardando");
  });

  it("when paused the same button says «Riprendi agente»", () => {
    const html = live({ mode: "paused" });
    expect(html).toContain("Riprendi agente");
    expect(html).not.toContain("Pausa agente");
  });

  it("while the person drives: the viewer is a named application that can be focused, with the keys, the field and the notice that nothing goes to the assistant", () => {
    const html = live({ mode: "human", focus: "campo password «Chiave»" });
    expect(html).toMatch(/<div[^>]*role="application"[^>]*tabindex="0"[^>]*>/);
    expect(html).toContain('data-own-keys=""');
    expect(html).toContain('aria-label="Pagina del browser, fornitori.example.com · Accedi · Fornitori"');
    expect(html).toMatch(/aria-describedby="[^"]+-h"/);
    expect(html).toContain("Tab e Maiusc+Tab escono dalla vista");
    for (const name of [">Tab<", ">Maiusc+Tab<", ">Invio<", ">Cancella<", ">Invia<", "Restituisci all&#x27;agente"]) expect(html).toContain(name);
    expect(html).toMatch(/<label[^>]*for="[^"]+-t"[^>]*>Scrivi nella pagina<\/label>/);
    expect(html).toContain("va solo alla pagina del sito: l&#x27;assistente non lo vede e non viene salvato");
    // The text is masked until shown, and no password manager is invited.
    expect(html).toMatch(/<input[^>]*type="password"[^>]*autoComplete="off"|<input[^>]*autoComplete="off"[^>]*type="password"|<input[^>]*type="password"/);
    expect(html).toContain("Mostra il testo");
    // The toggle says its state in its name; a button that also changed `aria-pressed` would say it twice, and wrongly.
    expect(html).not.toContain("aria-pressed");
    // The control that has the focus in the page is said in a live region.
    expect(html).toMatch(/<p[^>]*class="bl-focus"[^>]*role="status"[^>]*aria-live="polite"[^>]*>Nella pagina è selezionato: campo password «Chiave»<\/p>/);
  });

  it("an action of the agent still running turns the typing off and says why", () => {
    const html = live({ mode: "human", agent_busy: true });
    expect(html).not.toContain('role="application"');
    expect(html).toContain("L&#x27;agente sta finendo un&#x27;azione");
    expect(html.match(/<button[^>]*disabled=""[^>]*>(?:<svg.*?<\/svg>)?(?:Tab|Maiusc\+Tab|Invio|Cancella)<\/button>/g)?.length).toBe(4);
  });

  it("in the card the button that gives the browser back is not repeated: the card has its own", () => {
    expect(live({ mode: "human" }, "card", true)).not.toContain("Restituisci all&#x27;agente");
    expect(live({ mode: "human" }, "panel", false)).toContain("Restituisci all&#x27;agente");
  });

  it("the panel shows what the agent asked, the card does not (its own text has it)", () => {
    const handover = { question_id: "ask_1", run_id: "run_1", reason: "Accedi a example.com" };
    expect(live({ mode: "human", handover }, "panel")).toContain("L&#x27;agente ti chiede di intervenire:");
    expect(live({ mode: "human", handover }, "card", true)).not.toContain("L&#x27;agente ti chiede di intervenire:");
  });

  it("a window of the site is an alert dialog with its answers; a prompt has a field", () => {
    const html = live({ mode: "human", dialog: { kind: "prompt", message: "Come ti chiami?", default_text: "Anna" } });
    expect(html).toMatch(/role="alertdialog"[^>]*aria-labelledby="[^"]+"[^>]*aria-describedby="[^"]+"/);
    expect(html).toContain("Il sito chiede un valore");
    expect(html).toContain("Come ti chiami?");
    expect(html).toContain('aria-label="Risposta alla finestra del sito"');
    expect(html).toContain('value="Anna"');
    expect(html).toContain(">OK<");
    expect(html).toContain(">Annulla<");
    const confirm = live({ dialog: { kind: "confirm", message: "Uscire?", default_text: "" } });
    expect(confirm).not.toContain('aria-label="Risposta alla finestra del sito"');
    expect(confirm).toContain("Prendi il controllo per rispondere tu");
  });

  it("says where else the browser can be seen, or that this is the only view", () => {
    expect(live()).toContain("Puoi anche usare la finestra del browser sul computer.");
    expect(live({ headless: true })).toContain("questa vista è l&#x27;unico modo per vederlo");
  });

  it("a browser that is not open says so and has no controls", () => {
    const html = live({ running: false, page: null });
    expect(html).toContain("Il browser non è aperto");
    expect(html).not.toContain("Comandi del browser");
  });

  it("an address that is not https is said (the connection pill reads «http»)", () => {
    const html = live({ page: { title: "x", host: "example.org", secure: false } });
    expect(html).toContain("Connessione non sicura");
    expect(html).toContain("is-insecure");
  });
});

describe("HandoverCard markup", () => {
  const q: Question = {
    id: "ask_h", run_id: "run_1", kind: "handover", title: "Serve il tuo intervento nel browser", message: "Accedi a fornitori.example.com", options: [], multiple: false, allow_other: false,
    fields: [], confirm_label: "Ho finito: restituisci il controllo", deny_label: "Non posso ora", expires_at: null,
  };
  const card = (seed?: BrowserState | null, panelOpen = false) =>
    render(createElement(BrowserUiContext.Provider, { value: { panelOpen, openPanel: () => {} } }, createElement(HandoverCard, { q })), seed);

  it("is a group named by its title, with the request, the view and the two buttons; the focus goes to the text, not to a control", () => {
    const html = card(state({ mode: "human" }));
    expect(html).toMatch(/<div[^>]*role="group"[^>]*aria-labelledby="[^"]+"/);
    expect(html).toContain("Serve il tuo intervento nel browser");
    expect(html).toMatch(/<p[^>]*class="ask-message"[^>]*tabindex="-1"[^>]*data-ask-first[^>]*>Accedi a fornitori.example.com<\/p>/);
    expect(html).toContain("Ho finito: restituisci il controllo");
    expect(html).toContain("Non posso ora");
    expect(html).toContain('role="application"');
    // Only the text carries the marker that moves the focus.
    expect(html.match(/data-ask-first/g)).toHaveLength(1);
    // No generic «Non ora». And no <form> at all: the field that types into the page is a password field, and a form that goes away after a
    // request is what a browser takes for a sign-in to remember. Every button is a plain button.
    expect(html).not.toContain(">Non ora<");
    expect(html).not.toContain("<form");
    expect(html).not.toMatch(/<button(?![^>]*type="button")/);
  });

  it("the field that types into the page is not controlled: nothing typed can be in the `value` attribute of the markup", () => {
    const html = card(state({ mode: "human" }));
    const field = /<input[^>]*type="password"[^>]*>/.exec(html)?.[0] ?? "";
    expect(field).toContain('id="');
    expect(field).not.toContain("value=");
    const panel = live({ mode: "human" });
    expect(/<input[^>]*type="password"[^>]*>/.exec(panel)?.[0]).not.toContain("value=");
  });

  it("points to the panel while it is open, instead of showing a second view", () => {
    const html = card(state({ mode: "human" }), true);
    expect(html).toContain("La vista del browser è aperta nel pannello a lato");
    expect(html).not.toContain('role="application"');
    expect(html).toContain("Ho finito: restituisci il controllo");
  });

  it("without the browser for this device (403, 404) it says how to go on and keeps both buttons", () => {
    const html = card(null);
    expect(html).toContain("Da questo dispositivo non posso mostrarti il browser");
    expect(html).toContain("Ho finito: restituisci il controllo");
    expect(html).not.toContain('role="application"');
  });

  it("uses the interface's own words for the title and the buttons, not the engine's (always Italian), whatever the engine sends", () => {
    const html = render(createElement(HandoverCard, { q: { ...q, title: "Titolo del motore", confirm_label: "Fatto", deny_label: "Niente" } }), state());
    expect(html).toContain("Serve il tuo intervento nel browser");
    expect(html).toContain("Ho finito: restituisci il controllo");
    expect(html).toContain("Non posso ora");
    for (const engine of ["Titolo del motore", ">Fatto<", ">Niente<"]) expect(html).not.toContain(engine);
  });
});
