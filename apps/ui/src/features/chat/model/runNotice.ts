import type { RunStatus } from "../../../shared/api/types";
import type { Item } from "./items";

import { t } from "../../../shared/i18n";
export type NoticeTone = "neutral" | "warn" | "crit";

/**
 * What a note offers in the simple interface, where its usual link would lead to a section that is not there: put a
 * sentence in the message box for the assistant to act on, or show every section (developer mode) and open the one that helps.
 */
export type NoticeAction =
  | { kind: "ask"; label: string; text: string }
  | { kind: "developer"; label: string; to: string };

export interface RunNoticeInfo {
  tone: NoticeTone;
  icon: string;
  title: string;
  hint?: string;
  /** Developer mode: a link to the section that helps. */
  link?: { to: string; label: string };
  /** Simple mode: what to do instead of a link (never present together with `link`). */
  actions?: NoticeAction[];
}

const askAssistant = (text: string): NoticeAction => ({ kind: "ask", label: t("chat.chiedi_all_assistente"), text });
const showAll = (to: string): NoticeAction => ({ kind: "developer", label: t("shell.mostra_tutte_le_sezioni"), to });

/**
 * How a failed run is told. The backend marks the end of a run with `RUN_ERROR` and a code:
 * `cancelled` (the user stopped it: not an error), `runtime` (the model backend failed),
 * `hook_blocked` (a UserPromptSubmit hook refused the prompt). Anything else is a generic failure.
 * `simple`: the simple interface has none of the sections the links point to, so they become `actions`.
 */
export function describeRunError(code?: string, simple = false): RunNoticeInfo {
  switch (code) {
    case "cancelled":
      return { tone: "neutral", icon: "stop", title: t("chat.run_interrotto"), hint: t("chat.hai_fermato_l_esecuzione_quello") };
    case "runtime":
      return simple
        ? {
            tone: "crit", icon: "alert", title: t("chat.errore_del_runtime"), hint: t("chat.il_modello_non_ha_risposto_semplice"),
            actions: [askAssistant(t("chat.richiesta_controlla_il_motore")), showAll("/runtimes")],
          }
        : {
            tone: "crit", icon: "alert", title: t("chat.errore_del_runtime"),
            hint: t("chat.il_modello_non_ha_risposto"),
            link: { to: "/runtimes", label: t("chat.apri_i_runtime") },
          };
    case "hook_blocked":
      return simple
        ? { tone: "warn", icon: "shield", title: t("chat.bloccato_da_un_hook"), hint: t("chat.un_hook_di_sicurezza_ha"), actions: [showAll("/security/hooks")] }
        : { tone: "warn", icon: "shield", title: t("chat.bloccato_da_un_hook"), hint: t("chat.un_hook_di_sicurezza_ha"), link: { to: "/security/hooks", label: t("chat.vedi_gli_hook") } };
    default:
      return { tone: "crit", icon: "alert", title: t("chat.run_non_riuscito") };
  }
}

/** The small note under a message whose images the model cannot see. */
export const attachmentsNotice = (simple = false): RunNoticeInfo =>
  simple
    ? {
        tone: "warn", icon: "alert", title: t("chat.questo_modello_non_vede_le"), hint: t("chat.l_immagine_e_stata_allegata_semplice"),
        actions: [askAssistant(t("chat.richiesta_modello_con_visione"))],
      }
    : {
        tone: "warn", icon: "alert", title: t("chat.questo_modello_non_vede_le"),
        hint: t("chat.l_immagine_e_stata_allegata"),
        link: { to: "/models", label: t("chat.apri_modelli") },
      };

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
export function lastRunNotice(status: RunStatus | undefined, items: Item[], running: boolean, simple = false): RunNoticeInfo | null {
  if (running || items.length === 0 || items[items.length - 1].kind === "error") return null;
  if (status === "cancelled") {
    return { tone: "neutral", icon: "stop", title: t("chat.l_ultimo_run_e_stato"), hint: t("chat.il_lavoro_gia_svolto_e") };
  }
  if (status === "failed") {
    return simple
      ? { tone: "crit", icon: "alert", title: t("chat.l_ultimo_run_non_e"), hint: t("chat.qui_sopra_c_e_quello_semplice"), actions: [askAssistant(t("chat.richiesta_controlla_il_motore")), showAll("/runtimes")] }
      : { tone: "crit", icon: "alert", title: t("chat.l_ultimo_run_non_e"), hint: t("chat.qui_sopra_c_e_quello") , link: { to: "/runtimes", label: t("chat.apri_i_runtime") } };
  }
  return null;
}
