// The neural voice picker of the «Voce» section: which voices a language offers, and the value written back. Pure logic.

import type { TtsStatus, TtsVoice } from "../../../shared/api/types";

/** Languages the picker always offers, besides what the engine reports. */
const BASE_LANGUAGES = ["it", "en"];

export const LANGUAGE_NAME: Record<string, string> = { it: "Italiano", en: "Inglese" };
export const languageName = (code: string) => LANGUAGE_NAME[code] ?? code.toUpperCase();

/** What «Prova» reads for a language. */
export const SAMPLE: Record<string, string> = {
  it: "Ciao, questa è la mia voce. Ti piace come suona?",
  en: "Hello, this is my voice. Do you like how it sounds?",
};

/** The languages to show a picker for. */
export function pickerLanguages(tts: TtsStatus): string[] {
  const extra = [...tts.languages, ...tts.voices.map((v) => v.language)];
  return [...new Set([...BASE_LANGUAGES, ...extra])];
}

/** Voices of one language, in the order the engine lists them. */
export const voicesFor = (tts: TtsStatus, language: string): TtsVoice[] => tts.voices.filter((v) => v.language === language);

/** The saved choice for a language (undefined = automatic), read from the `voice.tts.voice` setting. */
export function savedPick(value: unknown, language: string): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const v = (value as Record<string, unknown>)[language];
  return typeof v === "string" && v ? v : undefined;
}

/** The new `voice.tts.voice` after choosing `id` for `language`; an empty id goes back to automatic. `null` when nothing is left (the key is removed). */
export function nextPicks(value: unknown, language: string, id: string): Record<string, string> | null {
  const old = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(old)) if (typeof v === "string" && v && k !== language) out[k] = v;
  if (id) out[language] = id;
  return Object.keys(out).length ? out : null;
}

/** One line on what speaks now, for the status of the section. */
export function engineLine(tts: TtsStatus): { tone: "ok" | "info"; text: string } {
  if (tts.neural) return { tone: "ok", text: `Voce neurale: ${tts.backend ?? tts.engine}` };
  return { tone: "info", text: tts.detail ?? "Si usano le voci del sistema operativo" };
}
