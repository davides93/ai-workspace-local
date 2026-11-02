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
