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
    id: "SessionStart", label: "Avvio sessione", tools: false, active: true,
    stdin: '{ "session_id", "run_id", "agent" }',
    can: "Aggiungere contesto al prompt di sistema (testo su stdout); parte solo al primo messaggio di una conversazione e non può bloccare.",
  },
  {
    id: "SubagentStart", label: "Avvio di un subagente", tools: true, active: true,
    stdin: '{ "agent", "task", "run_id", "depth" }',
    can: "Bloccare la delega (codice 2), riscrivere il compito ({\"updatedInput\":{\"task\":\"…\"}}) o aggiungere contesto. Il filtro si applica al nome dell'agente, non a un tool.",
  },
  {
    id: "SubagentStop", label: "Fine di un subagente", tools: true, active: true,
    stdin: '{ "agent", "task", "result", "is_error", "run_id" }',
    can: "Aggiungere contesto al risultato del subagente ({\"additionalContext\":\"…\"}). Il filtro si applica al nome dell'agente.",
  },
  {
    id: "PreCompact", label: "Prima della compattazione", tools: false, active: true,
    stdin: '{ "run_id", "messages", "estimated_tokens" }',
    can: "Saltare la compattazione del contesto in quel turno (codice 2).",
  },
];

export const hookEvent = (id: string): HookEventInfo | undefined => HOOK_EVENTS.find((e) => e.id === id);

export const MAX_TIMEOUT_S = 600;

export interface HookIssue {
  index: number;
  level: "error" | "warn";
  message: string;
}

/**
 * Problems of the hook list: the backend refuses (400) an unknown event, an empty command, a timeout outside 1 to
 * 600 seconds and a filter that is not a valid glob (`HookConfig::validated`), so those block saving here too;
 * an event the backend does not fire yet is only a warning.
 */
export function checkHooks(hooks: readonly HookConfig[], globProblem: (glob: string) => string | null): HookIssue[] {
  const issues: HookIssue[] = [];
  hooks.forEach((h, index) => {
    const info = hookEvent(h.event);
    if (!info) issues.push({ index, level: "error", message: `Evento «${String(h.event)}» non valido: usa ${HOOK_EVENTS.map((e) => e.id).join(", ")}.` });
    if (!h.command.trim()) issues.push({ index, level: "error", message: "Scrivi il comando da eseguire." });
    if (!Number.isInteger(h.timeout_s) || h.timeout_s < 1 || h.timeout_s > MAX_TIMEOUT_S) {
      issues.push({ index, level: "error", message: `Il timeout deve essere tra 1 e ${MAX_TIMEOUT_S} secondi.` });
    }
    if (info && !info.active) issues.push({ index, level: "warn", message: "Il backend non esegue ancora questo evento: l'hook viene salvato ma non partirà." });
    // The filter is sent only for the events that have a tool (see `normalizeHooks`).
    const matcher = info?.tools ? h.matcher?.trim() : "";
    if (matcher) {
      const p = globProblem(matcher);
      if (p) issues.push({ index, level: "error", message: `Il filtro «${matcher}» non è un glob valido (${p}).` });
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
