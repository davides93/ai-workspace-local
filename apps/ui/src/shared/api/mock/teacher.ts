import type { Memory, MemoryKind } from "../types";
import { rank, tokenize } from "./memories";

// What the demo's Insegnante does with a sentence: the same moves the real agent makes with the memory tools
// (save one memory per fact, search for "cosa sai di me?", update a correction, forget), decided by simple rules
// because the demo has no model to ask.

export type TeacherStep =
  | { tool: "memory_save"; args: { text: string; kind: MemoryKind; explicit: true; pinned?: boolean; due_at?: string }; summary: string }
  | { tool: "memory_search"; args: { query: string }; summary: string }
  | { tool: "memory_update"; args: { id: string; text: string }; summary: string }
  | { tool: "memory_forget"; args: { id: string }; summary: string };

export interface TeacherTurn {
  steps: TeacherStep[];
  reply: string;
}

const cap = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : s);
const MAX_FACTS = 6;
const WEEKDAYS = ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"];

/** `domani`, `dopodomani` or the next given weekday, at 09:00; `undefined` when there is no date in the sentence. */
export function dueFrom(text: string, now: Date = new Date()): string | undefined {
  const t = text.toLowerCase();
  const at = (days: number) => {
    const d = new Date(now);
    d.setDate(d.getDate() + days);
    d.setHours(9, 0, 0, 0);
    return d.toISOString();
  };
  if (/\bdopodomani\b/.test(t)) return at(2);
  if (/\bdomani\b/.test(t)) return at(1);
  const day = WEEKDAYS.findIndex((w) => t.includes(w));
  return day < 0 ? undefined : at(((day - now.getDay() + 7) % 7) || 7);
}

/** The kind of a sentence. */
export function guessKind(s: string): MemoryKind {
  const t = s.toLowerCase();
  if (/\b(ricordami|promemoria|non dimenticare|scadenza|entro)\b/.test(t)) return "reminder";
  if (/\b(rispondi|scrivi|usa|dammi|chiamami|quando (ti chiedo|lavori)|sempre|mai|prima di)\b/.test(t)) return "how_to_work";
  if (/\b(preferisco|mi piace|non mi piace|amo|odio|adoro|detesto)\b/.test(t)) return "preference";
  if (/\b(mi chiamo|sono nat|ho \d+ anni|vivo|abito|lavoro come|sono un|sono una|la mia famiglia)\b/.test(t)) return "about_you";
  return "fact";
}

/** The sentence as a note about the person (the real agent writes it in the third person). */
export function asNote(sentence: string): string {
  const s = sentence.trim().replace(/[.!?]+$/, "");
  const rules: [RegExp, string][] = [
    [/^mi chiamo (.+)/i, "Si chiama $1"],
    [/^preferisco (.+)/i, "Preferisce $1"],
    [/^vivo (.+)/i, "Vive $1"],
    [/^abito (.+)/i, "Abita $1"],
    [/^ricordami di (.+)/i, "Da ricordare: $1"],
    [/^rispondi (.+)/i, "Vuole che l'assistente risponda $1"],
    [/^(?:io )?ho (.+)/i, "Ha $1"],
    [/^mi piace (.+)/i, "Gli piace $1"],
    [/^non mi piace (.+)/i, "Non gli piace $1"],
    [/^lavoro (.+)/i, "Lavora $1"],
  ];
  for (const [re, to] of rules) if (re.test(s)) return `${cap(s.replace(re, to))}.`;
  return `${cap(s)}.`;
}

/** Splits what the person said into distinct facts: sentences, and "… e preferisco …"-style joins. */
export function splitFacts(input: string): string[] {
  return input
    .split(/[.;!\n]+|\s+e\s+(?=(?:preferisco|ho|mi piace|non mi piace|rispondi|vivo|abito|lavoro|ricordami)\b)/i)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .slice(0, MAX_FACTS);
}

const list = (ms: Memory[]) => ms.map((m) => `- ${m.text}`).join("\n");

/** What the Insegnante does for `input`, given what it knows now. */
export function teacherTurn(input: string, known: Memory[], now: Date = new Date()): TeacherTurn {
  const active = known.filter((m) => m.status === "active");
  const text = input.trim();

  if (/\b(cosa|che cosa|quel che|quello che) (sai|ricordi)\b/i.test(text) || /\bcosa ricordi\b/i.test(text)) {
    return {
      steps: [{ tool: "memory_search", args: { query: text }, summary: active.length ? `${active.length} ricordi` : "nessun ricordo" }],
      reply: active.length
        ? `Ecco cosa so finora:\n${list(active.slice(0, 12))}\n\nTrovi tutto, diviso per argomento, nel pannello «Cosa so».`
        : "Non so ancora nulla di te: raccontami qualcosa e lo ricordo.",
    };
  }

  const forget = /^(?:dimentica|scordati|cancella dai ricordi)\s+(.+)/i.exec(text);
  if (forget) {
    const hit = rank(forget[1], active)[0];
    if (!hit) return { steps: [], reply: `Non trovo nulla su «${forget[1].replace(/[.!?]+$/, "")}» tra i ricordi.` };
    return {
      steps: [{ tool: "memory_forget", args: { id: hit.id }, summary: hit.text }],
      reply: `Fatto, ho dimenticato: «${hit.text}». Se ci ripensi puoi ripristinarlo dalla lista «Cosa so».`,
    };
  }

  const fix = /^(?:in realtà|anzi|correggi|correzione)[:,]?\s+(.+)/i.exec(text);
  if (fix) {
    const note = asNote(fix[1]);
    const hit = rank(fix[1], active)[0];
    if (hit) {
      return { steps: [{ tool: "memory_update", args: { id: hit.id, text: note }, summary: note }], reply: `Corretto: «${note}»` };
    }
  }

  const facts = splitFacts(fix ? fix[1] : text);
  // One short question when it is not clear what to keep.
  if (facts.length === 0 || (facts.length === 1 && tokenize(facts[0]).length < 2 && !/[0-9]/.test(facts[0]))) {
    return { steps: [], reply: "Non ho capito bene cosa devo ricordare: puoi dirmelo con una frase intera?" };
  }
  const steps: TeacherStep[] = facts.map((f): TeacherStep => {
    const kind = guessKind(f);
    const note = asNote(f);
    const pinned = kind === "how_to_work" || /^si chiama/i.test(note) ? true : undefined;
    const due_at = kind === "reminder" ? dueFrom(f, now) : undefined;
    return { tool: "memory_save", args: { text: note, kind, explicit: true, ...(pinned ? { pinned } : {}), ...(due_at ? { due_at } : {}) }, summary: note };
  });
  return { steps, reply: steps.length === 1 ? `Ricordato: ${(steps[0] as { summary: string }).summary}` : `Ricordate ${steps.length} cose:\n${steps.map((s) => `- ${s.summary}`).join("\n")}` };
}
