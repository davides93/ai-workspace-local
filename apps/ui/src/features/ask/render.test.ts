import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ApiProvider } from "../../shared/api/context";
import { MockTransport } from "../../shared/api/mock";
import type { Question, SecretRequest } from "../../shared/api/types";
import type { QuestionItem, SecretItem } from "../chat/model/items";
import { QuestionCard } from "./QuestionCard";
import { SecretPrompt } from "./SecretPrompt";

// The markup of the cards (the project has no DOM test environment: this checks the structure and the attributes the
// keyboard and screen-reader behaviour stand on; the behaviour itself is in the end-to-end and accessibility tests).
const render = (el: ReactElement) =>
  renderToStaticMarkup(createElement(QueryClientProvider, { client: new QueryClient() }, createElement(ApiProvider, { api: new MockTransport(), children: el })));

const base: Question = {
  id: "ask_1", run_id: "run_1", kind: "choice", title: "Quale modello?", message: "Ho trovato tre varianti.",
  options: [{ id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" }, { id: "q8", label: "Q8_0 · 8,5 GB", description: null }],
  multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null,
};
const question = (q: Partial<Question>, extra: Partial<QuestionItem> = {}): string => render(createElement(QuestionCard, { it: { kind: "question", id: "ask_1", q: { ...base, ...q }, ...extra } }));

describe("QuestionCard markup", () => {
  it("a single choice is a form named by its title, with a radio group of native radios sharing one name", () => {
    const html = question({});
    expect(html).toMatch(/<form[^>]*aria-labelledby="[^"]+"/);
    expect(html).toContain('role="radiogroup"');
    expect(html.match(/type="radio"/g)).toHaveLength(2);
    const names = [...html.matchAll(/<input[^>]*type="radio"[^>]*>/g)].map((m) => /name="([^"]+)"/.exec(m[0])?.[1]);
    expect(names).toHaveLength(2);
    expect(new Set(names).size).toBe(1);
    expect(names[0]).toBeTruthy();
    // The digits are shown for the keyboard, hidden from the screen reader; the description is read as a description.
    expect(html).toContain('<kbd aria-hidden="true">1</kbd>');
    expect(html).toContain('aria-keyshortcuts="1"');
    expect(html).toMatch(/aria-describedby="[^"]+-d"/);
    expect(html).toContain("Non ora");
    expect(html).toContain("Invia");
  });

  it("several choices are checkboxes in a group", () => {
    const html = question({ multiple: true });
    expect(html).toContain('role="group"');
    expect(html).not.toContain('role="radiogroup"');
    expect(html.match(/type="checkbox"/g)).toHaveLength(2);
  });

  it("«Altro» adds one more choice (the next digit) and a field to write in, named", () => {
    const html = question({ allow_other: true });
    expect(html.match(/type="radio"/g)).toHaveLength(3);
    expect(html).toContain('<kbd aria-hidden="true">3</kbd>');
    expect(html).toContain('aria-label="Scrivi la tua risposta"');
  });

  it("a confirmation is two buttons with the labels given, and «Non ora»", () => {
    const html = question({ kind: "confirm", options: [], confirm_label: "Scarica", deny_label: "Lascia stare" });
    expect(html).toContain("Scarica");
    expect(html).toContain("Lascia stare");
    expect(html).toContain("Non ora");
    expect(html).not.toContain('type="radio"');
    expect(html).not.toContain(">Invia<");
  });

  it("a form has a labelled control per field and does not let the browser fill it in", () => {
    const html = question({
      kind: "form", options: [],
      fields: [{ name: "nome", label: "Nome", type: "string", required: true }, { name: "lingua", label: "Lingua", type: "enum", options: ["it", "en"] }, { name: "ok", label: "Va bene", type: "boolean" }],
    });
    expect(html).toMatch(/<label for="[^"]+-f-nome">Nome \*<\/label>/);
    expect(html).toContain('autoComplete="off"');
    expect(html).toContain("<select");
    expect(html).toContain('type="checkbox"');
    expect(html).toContain('aria-required="true"');
  });

  it("the card has a polite status region for the screen reader, empty at first (it is filled after it appears)", () => {
    expect(question({})).toContain('<p class="sr-only" role="status"></p>');
  });

  it("a closed question is one line with the answer, with no form", () => {
    const html = question({}, { outcome: "answered", answer: { selected: ["q4"], other: null, confirmed: null, values: null } });
    expect(html).not.toContain("<form");
    expect(html).toContain("Quale modello?");
    expect(html).toContain("Hai risposto: Q4_K_M · 4,7 GB");
    expect(question({}, { outcome: "declined", answer: null })).toContain("Hai scelto di non rispondere ora.");
    expect(question({}, { outcome: "cancelled", answer: null })).toContain("Senza risposta");
  });
});

const secretReq: SecretRequest = {
  id: "sec_1", run_id: "run_1", target: "provider_token:hf", label: "Token di Hugging Face", reason: "Serve per scaricare modelli riservati",
  destination: "Hugging Face (huggingface.co)",
};
const secret = (extra: Partial<SecretItem> = {}): string => render(createElement(SecretPrompt, { it: { kind: "secret", id: "sec_1", req: secretReq, ...extra } }));

describe("SecretPrompt markup", () => {
  it("shows what is asked, why, and where the value will be written", () => {
    const html = secret();
    expect(html).toContain("Token di Hugging Face");
    expect(html).toContain("Serve per scaricare modelli riservati");
    expect(html).toContain("Sarà salvato in: Hugging Face (huggingface.co)");
  });

  it("the field is protected: password, no autofill, no spellcheck, no capitalisation, empty", () => {
    const html = secret();
    const input = /<input[^>]*type="password"[^>]*>/.exec(html)?.[0] ?? "";
    expect(input).not.toBe("");
    expect(input).toContain('autoComplete="off"');
    expect(input).toContain('spellCheck="false"');
    expect(input).toContain('autoCapitalize="off"');
    expect(input).toContain('value=""');
    expect(input).toMatch(/id="([^"]+)"/);
    expect(html).toMatch(/<label for="[^"]+">Valore di «Token di Hugging Face»<\/label>/);
  });

  it("the show/hide button is a toggle with a name that does not change", () => {
    const html = secret();
    expect(html).toMatch(/<button[^>]*aria-pressed="false"[^>]*>Mostra il valore<\/button>/);
  });

  it("a closed request says only that it was saved or not provided: no field, no value", () => {
    const stored = secret({ outcome: "stored" });
    expect(stored).not.toContain("<form");
    expect(stored).not.toContain("<input");
    expect(stored).toContain("Salvato: Token di Hugging Face");
    expect(secret({ outcome: "declined" })).toContain("Non fornito: Token di Hugging Face");
  });
});
