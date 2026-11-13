// Lifecycle hooks (crates/aiw-agent/src/hooks.rs and runner.rs): what each event passes to the
// command, what the command can do about it, and the checks the editor makes before saving.

import type { HookConfig, HookEvent } from "../../../shared/api/types";

import { t, t as tr } from "../../../shared/i18n";
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
    id: "UserPromptSubmit", get label() { return tr("security.invio_del_messaggio"); }, tools: false, active: true,
    stdin: '{ "prompt", "run_id" }',
    get can() { return tr("security.bloccare_il_messaggio_uscita_con"); },
  },
  {
    id: "PreToolUse", get label() { return tr("security.prima_di_un_tool"); }, tools: true, active: true,
    stdin: '{ "tool", "args", "run_id" }',
    get can() { return tr("security.hook_can_pre_tool", { decision: '{"decision":"block"}', updated: '{"updatedInput":{…}}' }); },
  },
  {
    id: "PostToolUse", get label() { return tr("security.dopo_un_tool"); }, tools: true, active: true,
    stdin: '{ "tool", "args", "output", "is_error", "run_id" }',
    get can() { return tr("security.hook_can_post_tool", { context: '{"additionalContext":"…"}' }); },
  },
  {
    id: "Stop", get label() { return tr("security.fine_del_run"); }, tools: false, active: true,
    stdin: '{ "run_id", "final" }',
    get can() { return tr("security.solo_osservare_notifiche_log_un"); },
  },
  {
    id: "SessionStart", get label() { return tr("security.avvio_sessione"); }, tools: false, active: true,
    stdin: '{ "session_id", "run_id", "agent" }',
    get can() { return tr("security.aggiungere_contesto_al_prompt_di"); },
  },
  {
    id: "SubagentStart", get label() { return tr("security.avvio_di_un_subagente"); }, tools: true, active: true,
    stdin: '{ "agent", "task", "run_id", "depth" }',
    get can() { return tr("security.hook_can_subagent_start", { updated: '{"updatedInput":{"task":"…"}}' }); },
  },
  {
    id: "SubagentStop", get label() { return tr("security.fine_di_un_subagente"); }, tools: true, active: true,
    stdin: '{ "agent", "task", "result", "is_error", "run_id" }',
    get can() { return tr("security.hook_can_subagent_stop", { context: '{"additionalContext":"…"}' }); },
  },
  {
    id: "PreCompact", get label() { return tr("security.prima_della_compattazione"); }, tools: false, active: true,
    stdin: '{ "run_id", "messages", "estimated_tokens" }',
    get can() { return tr("security.saltare_la_compattazione_del_contesto"); },
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
    if (!info) issues.push({ index, level: "error", message: t("security.invalid_hook_event", { event: String(h.event), list: HOOK_EVENTS.map((e) => e.id).join(", ") }) });
    if (!h.command.trim()) issues.push({ index, level: "error", message: t("security.scrivi_il_comando_da_eseguire") });
    if (!Number.isInteger(h.timeout_s) || h.timeout_s < 1 || h.timeout_s > MAX_TIMEOUT_S) {
      issues.push({ index, level: "error", message: t("security.il_timeout_deve_essere_tra", { max: MAX_TIMEOUT_S }) });
    }
    if (info && !info.active) issues.push({ index, level: "warn", message: t("security.il_backend_non_esegue_ancora") });
    // The filter is sent only for the events that have a tool (see `normalizeHooks`).
    const matcher = info?.tools ? h.matcher?.trim() : "";
    if (matcher) {
      const p = globProblem(matcher);
      if (p) issues.push({ index, level: "error", message: t("security.il_filtro_non_e_un", { matcher, error: p }) });
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
