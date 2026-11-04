import type { RunStatus } from "../../../shared/api/types";
import type { Item } from "./items";

export type NoticeTone = "neutral" | "warn" | "crit";

export interface RunNoticeInfo {
  tone: NoticeTone;
  icon: string;
  title: string;
  hint?: string;
  link?: { to: string; label: string };
}

/**
 * How a failed run is told. The backend marks the end of a run with `RUN_ERROR` and a code:
 * `cancelled` (the user stopped it: not an error), `runtime` (the model backend failed),
 * `hook_blocked` (a UserPromptSubmit hook refused the prompt). Anything else is a generic failure.
 */
export function describeRunError(code?: string): RunNoticeInfo {
  switch (code) {
    case "cancelled":
      return { tone: "neutral", icon: "stop", title: "Run interrotto", hint: "Hai fermato l'esecuzione. Quello che l'agente aveva già prodotto resta nella conversazione." };
    case "runtime":
      return {
        tone: "crit", icon: "alert", title: "Errore del runtime",
        hint: "Il modello non ha risposto. Controlla che il runtime sia avviato e che il modello sia caricato, poi riprova.",
        link: { to: "/runtimes", label: "Apri i runtime" },
      };
    case "hook_blocked":
      return { tone: "warn", icon: "shield", title: "Bloccato da un hook", hint: "Un hook di sicurezza ha respinto il messaggio prima che arrivasse al modello.", link: { to: "/security/hooks", label: "Vedi gli hook" } };
    default:
      return { tone: "crit", icon: "alert", title: "Run non riuscito" };
  }
}

/** The standard texts of the backend need no repeating under a title that already says it. */
export const isStandardErrorMessage = (code: string | undefined, message: string): boolean =>
  (code === "cancelled" && /^Run annullato/i.test(message)) || message.trim() === "";

/**
 * Error code of a run error that was stored in the history. The live `RUN_ERROR` event carries a `code`, the stored
 * `system` message (`aiw.run_error`) only the text, so the code is read back from the wording of the runner
 * (crates/aiw-agent/src/runner.rs); anything else is a generic failure.
 */
export function runErrorCode(message: string): string | undefined {
  const m = message.trim();
  if (/^Run annullato/i.test(m)) return "cancelled";
  if (/^(Errore del runtime|Runtime non disponibile)\b/.test(m)) return "runtime";
  if (/^Prompt bloccato da un hook\b/.test(m)) return "hook_blocked";
  return undefined;
}

/** Prefixes that only repeat the title of the row ("Errore del runtime: …" under "Errore del runtime"). */
const TITLE_PREFIX: Record<string, RegExp> = {
  runtime: /^Errore del runtime:\s*/,
  hook_blocked: /^Prompt bloccato da un hook:\s*/,
};

/** The text shown under the title of an error row, or undefined when the title already says everything. */
export function errorDetail(code: string | undefined, message: string): string | undefined {
  if (isStandardErrorMessage(code, message)) return undefined;
  const prefix = code ? TITLE_PREFIX[code] : undefined;
  const text = (prefix ? message.replace(prefix, "") : message).trim();
  // What follows the prefix starts a sentence of its own.
  return text === "" ? undefined : prefix ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/**
 * Note under a history that ends with a stopped or failed run: the stored messages hold what the
 * run produced, but not how it ended (that came with the live events, which are gone).
 */
export function lastRunNotice(status: RunStatus | undefined, items: Item[], running: boolean): RunNoticeInfo | null {
  if (running || items.length === 0 || items[items.length - 1].kind === "error") return null;
  if (status === "cancelled") {
    return { tone: "neutral", icon: "stop", title: "L'ultimo run è stato interrotto", hint: "Il lavoro già svolto è conservato qui sopra: puoi proseguire scrivendo un nuovo messaggio." };
  }
  if (status === "failed") {
    return { tone: "crit", icon: "alert", title: "L'ultimo run non è andato a buon fine", hint: "Qui sopra c'è quello che l'agente aveva prodotto prima dell'errore. Controlla il runtime e riprova." , link: { to: "/runtimes", label: "Apri i runtime" } };
  }
  return null;
}
