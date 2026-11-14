import { describe, expect, it } from "vitest";
import { ApiError } from "../../shared/api/http";
import type { Question } from "../../shared/api/types";
import {
  answerSummary, bannerSession, choiceCount, choiceReply, closedText, confirmReply, digitChoice, emptyChoice, formReply, initialFormValues, isManagementTool,
  isTypingTarget, parseQuestion, parseQuestionArgs, parseQuestionResult, parseSecret, parseSecretResult, pickChoice, questionTitle, secretClosedText, secretFailure, waitingElsewhere,
} from "./model";

const base: Question = {
  id: "ask_1", run_id: "run_1", kind: "choice", title: null, message: "Quale modello?",
  options: [{ id: "q4", label: "Q4_K_M · 4,7 GB" }, { id: "q8", label: "Q8_0 · 8,5 GB" }, { id: "f16", label: "F16" }],
  multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null,
};

describe("management tools", () => {
  it("are the aiw_* tools and ask_user, like is_management of the engine", () => {
    expect(isManagementTool("aiw_model_download")).toBe(true);
    expect(isManagementTool("aiw_secret_request")).toBe(true);
    expect(isManagementTool("ask_user")).toBe(true);
    expect(isManagementTool("shell")).toBe(false);
    expect(isManagementTool("mcp__aiw_x__run")).toBe(false);
    expect(isManagementTool("my_aiw_tool")).toBe(false);
  });
});

describe("parsing", () => {
  it("reads the event of the contract (snake_case, nulls)", () => {
    const q = parseQuestion({
      id: "ask_01", run_id: "run_1", kind: "choice", title: "Quale modello?", message: "Ho trovato tre varianti.",
      options: [{ id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" }, { id: "q8", label: "Q8_0", description: null }],
      multiple: false, allow_other: false, fields: [], confirm_label: null, deny_label: null, expires_at: "2026-11-14T10:05:00Z",
    });
    expect(q).toMatchObject({ id: "ask_01", run_id: "run_1", kind: "choice", title: "Quale modello?", multiple: false, expires_at: "2026-11-14T10:05:00Z" });
    expect(q?.options).toEqual([{ id: "q4", label: "Q4_K_M · 4,7 GB", description: "Consigliata" }, { id: "q8", label: "Q8_0", description: null }]);
  });
  it("fills in what is missing and refuses what cannot be shown", () => {
    expect(parseQuestion({ id: "a", kind: "confirm", message: "Procedo?" })).toMatchObject({ options: [], fields: [], multiple: false, allow_other: false });
    expect(parseQuestion({ id: "a", kind: "poll", message: "x" })).toBeNull();
    expect(parseQuestion({ kind: "confirm", message: "x" })).toBeNull();
    expect(parseQuestion({ id: "a", kind: "confirm" })).toBeNull();
    expect(parseQuestion("testo")).toBeNull();
  });
  it("rebuilds the question of a stored call from its arguments, with the id of the call", () => {
    const q = parseQuestionArgs("call_9", JSON.stringify({ kind: "form", message: "Dati", fields: [{ name: "eta", label: "Età", type: "integer", required: true }, { name: "x", label: "X", type: "weird" }] }));
    expect(q).toMatchObject({ id: "call_9", kind: "form" });
    expect(q?.fields).toEqual([{ name: "eta", label: "Età", type: "integer", required: true, placeholder: null }]);
    expect(parseQuestionArgs("c", "{rotto")).toBeNull();
  });
  it("reads the result of the tool for the model", () => {
    expect(parseQuestionResult('{"outcome":"answered","selected":["q4"],"other":null,"confirmed":null,"values":null}')).toEqual({
      outcome: "answered", answer: { selected: ["q4"], other: null, confirmed: null, values: null },
    });
    expect(parseQuestionResult('{"outcome":"declined"}')).toEqual({ outcome: "declined", answer: null });
    expect(parseQuestionResult('{"outcome":"mah"}')).toBeNull();
    expect(parseQuestionResult("ERRORE: no")).toBeNull();
    expect(parseQuestionResult(undefined)).toBeNull();
  });
  it("a request for a secret is read without any value, and its outcome from the result", () => {
    const s = parseSecret({ id: "sec_1", run_id: "r", target: "provider_token:hf", label: "Token", reason: null, destination: "Hugging Face", value: "hf_TEST_ONLY_not_a_real_token" });
    expect(s).toEqual({ id: "sec_1", run_id: "r", target: "provider_token:hf", label: "Token", reason: null, destination: "Hugging Face", expires_at: null });
    expect(JSON.stringify(s)).not.toContain("hf_TEST_ONLY");
    expect(parseSecretResult('{"outcome":"stored","target":"provider_token:hf"}')).toBe("stored");
    expect(parseSecretResult('{"outcome":"x"}')).toBeNull();
  });
});

describe("choosing", () => {
  it("a single answer replaces the pick; «Altro» replaces the options", () => {
    const q = { ...base, allow_other: true };
    let st = pickChoice(q, emptyChoice, 0);
    expect(st.selected).toEqual(["q4"]);
    st = pickChoice(q, st, 2);
    expect(st).toMatchObject({ selected: ["f16"], otherOn: false });
    st = pickChoice(q, st, 3);
    expect(st).toMatchObject({ selected: [], otherOn: true });
    expect(pickChoice(q, st, 1)).toMatchObject({ selected: ["q8"], otherOn: false });
  });
  it("several answers toggle, «Altro» too", () => {
    const q = { ...base, multiple: true, allow_other: true };
    let st = pickChoice(q, emptyChoice, 0);
    st = pickChoice(q, st, 2);
    st = pickChoice(q, st, 3);
    expect(st).toMatchObject({ selected: ["q4", "f16"], otherOn: true });
    st = pickChoice(q, st, 0);
    st = pickChoice(q, st, 3);
    expect(st).toMatchObject({ selected: ["f16"], otherOn: false });
  });
  it("a choice that does not exist changes nothing", () => {
    expect(pickChoice(base, emptyChoice, 3)).toBe(emptyChoice);
    expect(pickChoice(base, emptyChoice, -1)).toBe(emptyChoice);
    expect(choiceCount(base)).toBe(3);
    expect(choiceCount({ ...base, allow_other: true })).toBe(4);
  });
  it("digits 1-9 stand for the choices in order, «Altro» last", () => {
    expect(digitChoice("1", base)).toBe(0);
    expect(digitChoice("3", base)).toBe(2);
    expect(digitChoice("4", base)).toBeNull();
    expect(digitChoice("4", { ...base, allow_other: true })).toBe(3);
    expect(digitChoice("0", base)).toBeNull();
    expect(digitChoice("a", base)).toBeNull();
    expect(digitChoice("10", base)).toBeNull();
  });
  it("digits and Enter belong to the text while typing, not on radios, checkboxes and buttons", () => {
    expect(isTypingTarget({ tagName: "INPUT", type: "text" })).toBe(true);
    expect(isTypingTarget({ tagName: "INPUT" })).toBe(true);
    expect(isTypingTarget({ tagName: "INPUT", type: "password" })).toBe(true);
    expect(isTypingTarget({ tagName: "INPUT", type: "number" })).toBe(true);
    expect(isTypingTarget({ tagName: "TEXTAREA" })).toBe(true);
    expect(isTypingTarget({ tagName: "SELECT" })).toBe(true);
    expect(isTypingTarget({ tagName: "DIV", isContentEditable: true })).toBe(true);
    expect(isTypingTarget({ tagName: "INPUT", type: "radio" })).toBe(false);
    expect(isTypingTarget({ tagName: "INPUT", type: "checkbox" })).toBe(false);
    expect(isTypingTarget({ tagName: "BUTTON" })).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
});

describe("the answer to a choice", () => {
  it("sends the picked id and nulls for what does not apply", () => {
    expect(choiceReply(base, pickChoice(base, emptyChoice, 0))).toEqual({ ok: true, reply: { action: "answer", selected: ["q4"], other: null, confirmed: null, values: null } });
  });
  it("asks for a pick when there is none", () => {
    expect(choiceReply(base, emptyChoice)).toEqual({ ok: false, error: "Scegli un'opzione." });
  });
  it("with «Altro» sends the text; an empty «Altro» is not an answer", () => {
    const q = { ...base, allow_other: true };
    expect(choiceReply(q, { ...pickChoice(q, emptyChoice, 3), otherText: "  Phi-4 mini " })).toEqual({ ok: true, reply: { action: "answer", selected: null, other: "Phi-4 mini", confirmed: null, values: null } });
    expect(choiceReply(q, pickChoice(q, emptyChoice, 3))).toEqual({ ok: false, error: "Scrivi la tua risposta in «Altro»." });
  });
  it("several answers: all the ids, and the text of «Altro» next to them", () => {
    const q = { ...base, multiple: true, allow_other: true };
    let st = pickChoice(q, emptyChoice, 0);
    st = pickChoice(q, st, 1);
    expect(choiceReply(q, st)).toMatchObject({ ok: true, reply: { selected: ["q4", "q8"], other: null } });
    st = { ...pickChoice(q, st, 3), otherText: "altro" };
    expect(choiceReply(q, st)).toMatchObject({ ok: true, reply: { selected: ["q4", "q8"], other: "altro" } });
  });
  it("never sends an id that is not an option, nor two ids to a single answer, nor «Altro» when it is not allowed", () => {
    expect(choiceReply(base, { selected: ["boh"], otherOn: false, otherText: "" })).toMatchObject({ ok: false });
    expect(choiceReply(base, { selected: ["q4", "q8"], otherOn: false, otherText: "" })).toMatchObject({ ok: true, reply: { selected: ["q4"] } });
    expect(choiceReply(base, { selected: [], otherOn: true, otherText: "x" })).toMatchObject({ ok: false });
  });
  it("confirm sends only the boolean", () => {
    expect(confirmReply(true)).toEqual({ action: "answer", selected: null, other: null, confirmed: true, values: null });
    expect(confirmReply(false)).toMatchObject({ confirmed: false });
  });
});

describe("the answer to a form", () => {
  const form: Question = {
    ...base, kind: "form", options: [],
    fields: [
      { name: "nome", label: "Nome", type: "string", required: true },
      { name: "eta", label: "Età", type: "integer" },
      { name: "peso", label: "Peso", type: "number" },
      { name: "ok", label: "Va bene", type: "boolean" },
      { name: "lingua", label: "Lingua", type: "enum", options: ["it", "en"] },
    ],
  };
  it("starts with booleans off and the first value of an enum", () => {
    expect(initialFormValues(form.fields)).toEqual({ nome: "", eta: "", peso: "", ok: false, lingua: "it" });
  });
  it("sends the typed values, leaving out the optional ones that are empty", () => {
    expect(formReply(form, { nome: " Ada ", eta: "36", peso: "", ok: true, lingua: "en" })).toEqual({
      ok: true, reply: { action: "answer", selected: null, other: null, confirmed: null, values: { nome: "Ada", eta: 36, ok: true, lingua: "en" } },
    });
  });
  it("says what is wrong, in Italian", () => {
    expect(formReply(form, { nome: " ", eta: "", peso: "", ok: false, lingua: "it" })).toEqual({ ok: false, error: "Compila «Nome»" });
    expect(formReply(form, { nome: "a", eta: "3.5", peso: "", ok: false, lingua: "it" })).toMatchObject({ ok: false, error: expect.stringContaining("intero") });
    expect(formReply(form, { nome: "a", eta: "", peso: "x", ok: false, lingua: "it" })).toMatchObject({ ok: false, error: expect.stringContaining("numero") });
    expect(formReply(form, { nome: "a", eta: "", peso: "", ok: false, lingua: "fr" })).toMatchObject({ ok: false, error: expect.stringContaining("valori proposti") });
  });
});

describe("what a closed question says", () => {
  const q = { ...base, title: "  ", allow_other: true };
  it("names the question: its own title, or «Domanda»", () => {
    expect(questionTitle({ ...base, title: "Quale modello?" })).toBe("Quale modello?");
    expect(questionTitle(q)).toBe("Domanda");
  });
  it("shows the labels picked (not the ids), and the text of «Altro»", () => {
    expect(answerSummary(q, { selected: ["q4"], other: null, confirmed: null, values: null })).toBe("Q4_K_M · 4,7 GB");
    expect(answerSummary({ ...q, multiple: true }, { selected: ["q4", "f16"], other: "Phi", confirmed: null, values: null })).toBe("Q4_K_M · 4,7 GB, F16, «Phi»");
    expect(answerSummary(q, { selected: ["gone"], other: null, confirmed: null, values: null })).toBe("gone");
    expect(answerSummary(q, null)).toBe("");
  });
  it("shows the label of the button for a confirmation, and the values of a form", () => {
    const c: Question = { ...base, kind: "confirm", confirm_label: "Scarica", deny_label: "Lascia stare" };
    expect(answerSummary(c, { selected: null, other: null, confirmed: true, values: null })).toBe("Scarica");
    expect(answerSummary(c, { selected: null, other: null, confirmed: false, values: null })).toBe("Lascia stare");
    expect(answerSummary({ ...c, confirm_label: null, deny_label: null }, { selected: null, other: null, confirmed: true, values: null })).toBe("Sì");
    const f: Question = { ...base, kind: "form", fields: [{ name: "n", label: "Nome", type: "string" }, { name: "ok", label: "Ok", type: "boolean" }] };
    expect(answerSummary(f, { selected: null, other: null, confirmed: null, values: { n: "Ada", ok: false } })).toBe("Nome: Ada · Ok: No");
  });
  it("has a sentence for every outcome", () => {
    const a = { selected: ["q4"], other: null, confirmed: null, values: null };
    expect(closedText(q, "answered", a)).toBe("Hai risposto: Q4_K_M · 4,7 GB");
    for (const o of ["declined", "timeout", "unsupported", "cancelled"] as const) expect(closedText(q, o, null)).not.toBe("");
    for (const o of ["stored", "declined", "timeout", "unsupported", "cancelled"] as const) expect(secretClosedText("Token", o)).toContain("Token");
    expect(secretClosedText("Token di Hugging Face", "stored")).toBe("Salvato: Token di Hugging Face");
  });
});

describe("a secret that fails", () => {
  it("tells a refused value (try again) from a device that may not (403) and a request already closed", () => {
    expect(secretFailure(new ApiError(400, "x"))).toBe("rejected");
    expect(secretFailure(new ApiError(401, "x"))).toBe("rejected");
    expect(secretFailure(new ApiError(403, "x"))).toBe("forbidden");
    expect(secretFailure(new ApiError(404, "x"))).toBe("closed");
    expect(secretFailure(new ApiError(500, "x"))).toBe("other");
    expect(secretFailure(new TypeError("Failed to fetch"))).toBe("other");
  });
});

describe("what waits in another conversation", () => {
  const sessionOf = (run: string) => ({ r1: "s1", r2: "s2" })[run];
  const list = [{ run_id: "r1" }, { run_id: "r2" }, { run_id: "r9" }];
  it("leaves out what belongs to the open conversation", () => {
    expect(waitingElsewhere(list, "s1", sessionOf).map((x) => x.run_id)).toEqual(["r2", "r9"]);
  });
  it("keeps what comes from a run this tab does not know, and everything outside the chat", () => {
    expect(waitingElsewhere(list, "s2", sessionOf).map((x) => x.run_id)).toEqual(["r1", "r9"]);
    expect(waitingElsewhere(list, null, sessionOf)).toHaveLength(3);
    expect(waitingElsewhere(undefined, "s1", sessionOf)).toEqual([]);
  });
  it("leads to the first conversation it knows, or to none", () => {
    expect(bannerSession([{ run_id: "r9" }, { run_id: "r2" }], sessionOf)).toBe("s2");
    expect(bannerSession([{ run_id: "r9" }], sessionOf)).toBeNull();
    expect(bannerSession([], sessionOf)).toBeNull();
  });
});
