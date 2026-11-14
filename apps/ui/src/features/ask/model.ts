import type { McpElicitation, Question, QuestionAnswer, QuestionField, QuestionKind, QuestionOutcome, QuestionReply, SecretOutcome, SecretRequest } from "../../shared/api/types";
import { errorStatus } from "../../shared/lib/errors";
import { initialValues, toContent, type FormValues } from "../mcp/model";

import { t } from "../../shared/i18n";
/** The tool the agent asks a question with (a row of the conversation, `kind: "question"`). */
export const ASK_TOOL = "ask_user";
/** The tool the agent asks for a secret with (a row of the conversation, `kind: "secret"`): it never carries the value. */
export const SECRET_TOOL = "aiw_secret_request";

/** `name` of the custom events of the questions and the secret requests. */
export const QUESTION_EVENT = "aiw.question";
export const QUESTION_RESOLVED_EVENT = "aiw.question_resolved";
export const SECRET_EVENT = "aiw.secret_request";
export const SECRET_RESOLVED_EVENT = "aiw.secret_resolved";

/** The management tools of the Assistente: `aiw_*` and `ask_user` (`is_management` of the engine). */
export const isManagementTool = (name: string): boolean => name.startsWith("aiw_") || name === ASK_TOOL;

const KINDS: readonly QuestionKind[] = ["choice", "confirm", "form"];
const QUESTION_OUTCOMES: readonly QuestionOutcome[] = ["answered", "declined", "timeout", "cancelled", "unsupported"];
const SECRET_OUTCOMES: readonly SecretOutcome[] = ["stored", "declined", "timeout", "cancelled", "unsupported"];
const FIELD_TYPES: readonly QuestionField["type"][] = ["string", "number", "integer", "boolean", "enum"];

const isRecord = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);
const strOrNull = (v: unknown): string | null => (typeof v === "string" ? v : null);

function parseOptions(v: unknown): Question["options"] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((o) => (isRecord(o) && typeof o.id === "string" && typeof o.label === "string" ? [{ id: o.id, label: o.label, description: strOrNull(o.description) }] : []));
}

function parseFields(v: unknown): QuestionField[] {
  if (!Array.isArray(v)) return [];
  return v.flatMap((f): QuestionField[] => {
    if (!isRecord(f) || typeof f.name !== "string" || typeof f.label !== "string" || !FIELD_TYPES.includes(f.type as QuestionField["type"])) return [];
    const options = Array.isArray(f.options) ? f.options.filter((o): o is string => typeof o === "string") : undefined;
    return [{ name: f.name, label: f.label, type: f.type as QuestionField["type"], ...(options ? { options } : {}), required: f.required === true, placeholder: strOrNull(f.placeholder) }];
  });
}

/**
 * A question from what the engine sent (`aiw.question`, `GET /api/questions`) or from the arguments of a stored `ask_user` call.
 * Tolerant of what is missing; `null` when it cannot be shown (no id, no text, a kind that does not exist).
 */
export function parseQuestion(value: unknown, fallback: { id?: string; run_id?: string } = {}): Question | null {
  if (!isRecord(value)) return null;
  const id = str(value.id) ?? fallback.id;
  const kind = value.kind as QuestionKind;
  if (!id || !KINDS.includes(kind) || typeof value.message !== "string") return null;
  return {
    id, run_id: str(value.run_id) ?? fallback.run_id ?? "", kind, title: strOrNull(value.title), message: value.message,
    options: parseOptions(value.options), multiple: value.multiple === true, allow_other: value.allow_other === true, fields: parseFields(value.fields),
    confirm_label: strOrNull(value.confirm_label), deny_label: strOrNull(value.deny_label), expires_at: strOrNull(value.expires_at),
  };
}

/** A question rebuilt from the arguments (JSON text) of a stored `ask_user` call: the id is the id of the call. */
export function parseQuestionArgs(callId: string, args: string): Question | null {
  try {
    return parseQuestion(JSON.parse(args), { id: callId });
  } catch {
    return null;
  }
}

function parseJson(content: string | undefined): Record<string, unknown> | null {
  if (!content) return null;
  try {
    const v: unknown = JSON.parse(content);
    return isRecord(v) ? v : null;
  } catch {
    return null;
  }
}

/** The outcome of a question as the engine names it, or `null` for anything else. */
export const asQuestionOutcome = (v: unknown): QuestionOutcome | null => (QUESTION_OUTCOMES.includes(v as QuestionOutcome) ? (v as QuestionOutcome) : null);
/** The outcome of a request for a secret as the engine names it, or `null` for anything else. */
export const asSecretOutcome = (v: unknown): SecretOutcome | null => (SECRET_OUTCOMES.includes(v as SecretOutcome) ? (v as SecretOutcome) : null);

/** How a question ended, from the result of the `ask_user` call (`{"outcome":"answered","selected":["q4"],…}`); `null` when the text is not one. */
export function parseQuestionResult(content: string | undefined): { outcome: QuestionOutcome; answer: QuestionAnswer | null } | null {
  const v = parseJson(content);
  const outcome = asQuestionOutcome(v?.outcome);
  if (!v || !outcome) return null;
  return { outcome, answer: outcome === "answered" ? toAnswer(v) : null };
}

/** The answer part of an event or of a result: what does not apply is `null`. */
export function toAnswer(v: unknown): QuestionAnswer | null {
  if (!isRecord(v)) return null;
  return {
    selected: Array.isArray(v.selected) ? v.selected.filter((x): x is string => typeof x === "string") : null,
    other: strOrNull(v.other),
    confirmed: typeof v.confirmed === "boolean" ? v.confirmed : null,
    values: isRecord(v.values) ? (v.values as QuestionAnswer["values"]) : null,
  };
}

/** A request for a secret from an event or from the arguments of a stored call. It has no value to read: there never is one here. */
export function parseSecret(value: unknown, fallback: { id?: string; run_id?: string } = {}): SecretRequest | null {
  if (!isRecord(value)) return null;
  const id = str(value.id) ?? fallback.id;
  if (!id || typeof value.label !== "string") return null;
  return {
    id, run_id: str(value.run_id) ?? fallback.run_id ?? "", target: str(value.target) ?? "", label: value.label, reason: strOrNull(value.reason),
    destination: str(value.destination) ?? "", expires_at: strOrNull(value.expires_at),
  };
}

export function parseSecretArgs(callId: string, args: string): SecretRequest | null {
  try {
    return parseSecret(JSON.parse(args), { id: callId });
  } catch {
    return null;
  }
}

export function parseSecretResult(content: string | undefined): SecretOutcome | null {
  return asSecretOutcome(parseJson(content)?.outcome);
}

// ---------------------------------------------------------------- the answer to a choice

/** What the person has picked in a `choice` question: option ids, and «Altro» with its text. */
export interface ChoiceState {
  selected: string[];
  otherOn: boolean;
  otherText: string;
}

export const emptyChoice: ChoiceState = { selected: [], otherOn: false, otherText: "" };

/** Number of things to pick from: the options, plus «Altro» when the question allows it. */
export const choiceCount = (q: Question): number => q.options.length + (q.allow_other ? 1 : 0);

/**
 * Picks the `index`-th choice (the options in order, then «Altro»). With a single answer it replaces the
 * pick; with several it toggles. An index that does not exist leaves the state as it is.
 */
export function pickChoice(q: Question, st: ChoiceState, index: number): ChoiceState {
  if (index < 0 || index >= choiceCount(q)) return st;
  if (index === q.options.length) return q.multiple ? { ...st, otherOn: !st.otherOn } : { ...st, selected: [], otherOn: true };
  const id = q.options[index].id;
  if (!q.multiple) return { ...st, selected: [id], otherOn: false };
  return { ...st, selected: st.selected.includes(id) ? st.selected.filter((x) => x !== id) : [...st.selected, id] };
}

/** Which choice the digit key `1`..`9` stands for (0-based), or `null` when there is no such choice. */
export function digitChoice(key: string, q: Question): number | null {
  if (!/^[1-9]$/.test(key)) return null;
  const i = Number(key) - 1;
  return i < choiceCount(q) ? i : null;
}

/** A keyboard target where digits and Enter belong to the text being typed. */
export function isTypingTarget(el: { tagName?: string; type?: string; isContentEditable?: boolean } | null | undefined): boolean {
  if (!el) return false;
  if (el.isContentEditable) return true;
  const tag = (el.tagName ?? "").toUpperCase();
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  return tag === "INPUT" && !["radio", "checkbox", "button", "submit", "reset"].includes((el.type ?? "text").toLowerCase());
}

export type Built = { ok: true; reply: QuestionReply } | { ok: false; error: string };

const answer = (a: Partial<QuestionAnswer>): QuestionReply => ({ action: "answer", selected: null, other: null, confirmed: null, values: null, ...a });

/** The reply to a `choice` question, or what is missing. Only ids of existing options are sent; with a single answer, one pick at most. */
export function choiceReply(q: Question, st: ChoiceState): Built {
  const known = st.selected.filter((id) => q.options.some((o) => o.id === id));
  const ids = q.multiple ? known : known.slice(0, 1);
  const other = q.allow_other && st.otherOn ? st.otherText.trim() : "";
  if (q.allow_other && st.otherOn && other === "") return { ok: false, error: t("ask.scrivi_la_risposta_in_altro") };
  if (ids.length === 0 && other === "") return { ok: false, error: t("ask.scegli_un_opzione") };
  return { ok: true, reply: answer({ selected: ids.length > 0 ? ids : null, other: other || null }) };
}

export const confirmReply = (confirmed: boolean): QuestionReply => answer({ confirmed });

// ---------------------------------------------------------------- the answer to a form

/** The fields of a `form` question as the schema of an MCP question: the check of the values is the same one. */
export function fieldsSchema(fields: QuestionField[]): McpElicitation["schema"] {
  return {
    type: "object",
    properties: Object.fromEntries(fields.map((f) => [f.name, { type: f.type === "enum" ? "string" : f.type, title: f.label, ...(f.type === "enum" ? { enum: f.options ?? [] } : {}) } as const])),
    required: fields.filter((f) => f.required).map((f) => f.name),
  };
}

/** Starting values of a form: booleans off, the first value of an enum, the rest empty. */
export const initialFormValues = (fields: QuestionField[]): FormValues => initialValues(fieldsSchema(fields));

/** The reply to a `form` question, or the first problem found (the engine checks the values again). */
export function formReply(q: Question, values: FormValues): Built {
  const r = toContent(fieldsSchema(q.fields), values);
  return "error" in r ? { ok: false, error: r.error } : { ok: true, reply: answer({ values: r.content }) };
}

// ---------------------------------------------------------------- what a closed question says

/** The title of a question: its own, or «Domanda». */
export const questionTitle = (q: Question): string => q.title?.trim() || t("ask.domanda");

const yesNo = (b: boolean) => (b ? t("ask.si") : t("ask.no"));

/** The answer in a few words: the labels picked, «Sì»/«No» (or the labels of the buttons), the values of the form. */
export function answerSummary(q: Question, a: QuestionAnswer | null): string {
  if (!a) return "";
  if (q.kind === "confirm") return a.confirmed === null ? "" : a.confirmed ? q.confirm_label?.trim() || t("ask.si") : q.deny_label?.trim() || t("ask.no");
  if (q.kind === "form") {
    const values = a.values ?? {};
    return q.fields.filter((f) => values[f.name] !== undefined).map((f) => `${f.label}: ${typeof values[f.name] === "boolean" ? yesNo(values[f.name] as boolean) : String(values[f.name])}`).join(" · ");
  }
  const labels = (a.selected ?? []).map((id) => q.options.find((o) => o.id === id)?.label ?? id);
  if (a.other) labels.push(`«${a.other}»`);
  return labels.join(", ");
}

/** The sentence a closed question shows. */
export function closedText(q: Question, outcome: QuestionOutcome, a: QuestionAnswer | null): string {
  switch (outcome) {
    case "answered": return t("ask.risposta", { answer: answerSummary(q, a) });
    case "declined": return t("ask.hai_scelto_di_non_rispondere");
    case "timeout": return t("ask.tempo_scaduto");
    case "unsupported": return t("ask.non_mostrata");
    case "cancelled": return t("ask.senza_risposta");
  }
}

/** The sentence a closed request for a secret shows (never a value: there is none). */
export function secretClosedText(label: string, outcome: SecretOutcome): string {
  switch (outcome) {
    case "stored": return t("ask.segreto_salvato", { label });
    case "declined": return t("ask.segreto_non_fornito", { label });
    case "timeout": return t("ask.segreto_scaduto", { label });
    case "unsupported": return t("ask.segreto_non_mostrato", { label });
    case "cancelled": return t("ask.segreto_annullato", { label });
  }
}

// ---------------------------------------------------------------- the secret that the destination refuses, the device that may not

/**
 * What an answer to a request for a secret that failed means: the destination refused the value (400, 401: asking again with
 * another value is fine), this device may not provide secrets (403), the request is already closed (404), or something else.
 */
export function secretFailure(e: unknown): "rejected" | "forbidden" | "closed" | "other" {
  const s = errorStatus(e);
  if (s === 400 || s === 401 || s === 422) return "rejected";
  if (s === 403) return "forbidden";
  if (s === 404 || s === 410) return "closed";
  return "other";
}

// ---------------------------------------------------------------- the banner for what waits in another conversation

/** Of the things waiting, those whose run is not in the open conversation (`sessionOf`: where the run belongs, when this tab knows). */
export function waitingElsewhere<T extends { run_id: string }>(list: readonly T[] | undefined, open: string | null, sessionOf: (runId: string) => string | undefined): T[] {
  return (list ?? []).filter((x) => open === null || sessionOf(x.run_id) !== open);
}

/** The conversation the banner leads to: the first one this tab knows among those waiting; `null` when it knows none. */
export function bannerSession(waiting: readonly { run_id: string }[], sessionOf: (runId: string) => string | undefined): string | null {
  for (const w of waiting) {
    const s = sessionOf(w.run_id);
    if (s) return s;
  }
  return null;
}
