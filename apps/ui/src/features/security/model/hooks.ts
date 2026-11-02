// Lifecycle hooks (crates/aiw-agent/src/hooks.rs and runner.rs): what each event passes to the
// command, what the command can do about it, and the checks the editor makes before saving.

import type { HookConfig, HookEvent } from "../../../shared/api/types";

export interface HookEventInfo {
  id: HookEvent;
  label: string;
  /** The event concerns a tool call, so `matcher` (a glob on the tool name) applies. */
  tools: boolean;
  /** Does the backend fire it today? */
  active: boolean;
  /** JSON the command receives on stdin. */
  stdin: string;
  /** What the command can do. */
  can: string;
}

export const HOOK_EVENTS: HookEventInfo[] = [
  {
    id: "UserPromptSubmit", label: "Invio del messaggio", tools: false, active: true,
    stdin: '{ "prompt", "run_id" }',
    can: "Bloccare il messaggio (uscita con codice 2) o aggiungere contesto (testo su stdout).",
  },
  {
    id: "PreToolUse", label: "Prima di un tool", tools: true, active: true,
    stdin: '{ "tool", "args", "run_id" }',
    can: "Bloccare la chiamata (codice 2 o {\"decision\":\"block\"}) o riscrivere gli argomenti ({\"updatedInput\":{…}}).",
  },
  {
    id: "PostToolUse", label: "Dopo un tool", tools: true, active: true,
    stdin: '{ "tool", "args", "output", "is_error", "run_id" }',
    can: "Aggiungere contesto al risultato ({\"additionalContext\":\"…\"}); non può annullare la chiamata già eseguita.",
  },
  {
    id: "Stop", label: "Fine del run", tools: false, active: true,
    stdin: '{ "run_id", "final" }',
    can: "Solo osservare (notifiche, log): un blocco viene registrato ma l'agente non riprende.",
  },
  {
    id: "SessionStart", label: "Avvio sessione", tools: false, active: false,
    stdin: "—",
    can: "Il formato lo prevede, ma oggi il backend non esegue questo evento.",
  },
];

export const hookEvent = (id: string): HookEventInfo | undefined => HOOK_EVENTS.find((e) => e.id === id);

export const MAX_TIMEOUT_S = 600;

export interface HookIssue {
  index: number;
  level: "error" | "warn";
  message: string;
}

/** Problems of the hook list: an empty command or a wrong timeout block saving; the rest are warnings. */
export function checkHooks(hooks: readonly HookConfig[], globProblem: (glob: string) => string | null): HookIssue[] {
  const issues: HookIssue[] = [];
  hooks.forEach((h, index) => {
    if (!h.command.trim()) issues.push({ index, level: "error", message: "Scrivi il comando da eseguire." });
    if (!Number.isInteger(h.timeout_s) || h.timeout_s < 1 || h.timeout_s > MAX_TIMEOUT_S) {
      issues.push({ index, level: "error", message: `Il timeout deve essere tra 1 e ${MAX_TIMEOUT_S} secondi.` });
    }
    const info = hookEvent(h.event);
    if (info && !info.active) issues.push({ index, level: "warn", message: "Il backend non esegue ancora questo evento: l'hook viene salvato ma non partirà." });
    if (info?.tools && h.matcher?.trim()) {
      const p = globProblem(h.matcher.trim());
      if (p) issues.push({ index, level: "warn", message: `Il filtro non è un glob valido (${p}): verrà confrontato come testo.` });
    }
  });
  return issues;
}

/** Hooks as sent to the backend: trimmed, and a matcher only for the events that have a tool. */
export function normalizeHooks(hooks: readonly HookConfig[]): HookConfig[] {
  return hooks.map((h) => ({
    event: h.event,
    matcher: hookEvent(h.event)?.tools && h.matcher?.trim() ? h.matcher.trim() : null,
    command: h.command.trim(),
    timeout_s: h.timeout_s,
  }));
}

export function blankHook(): HookConfig {
  return { event: "PreToolUse", matcher: "", command: "", timeout_s: 30 };
}
