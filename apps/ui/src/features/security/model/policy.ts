// Pure mirror of crates/aiw-agent/src/policy.rs (ADR 0005): given the approval mode, the rules and
// what the tool is about to do, say whether it is allowed, needs a confirmation or is denied.
// Keep it in step with the Rust file: the test panel of the Security page relies on it.

import type { CommandAnalysis, PolicyAction, PolicyMode, PolicyRule, ToolRisk } from "../../../shared/api/types";
import { globMatches } from "./glob";

import { t, t as tr } from "../../../shared/i18n";
export type DecisionKind = "allow" | "ask" | "deny";

export interface Decision {
  kind: DecisionKind;
  /** Short explanation, in the words of the backend when it has one. */
  reason: string;
  /** Who decided: a rule (with its position), the session approval or the approval mode. */
  by: "rule" | "session" | "mode";
  ruleIndex?: number;
}

/** Does a single rule apply to this tool and subject (path, command or URL of the call)? */
export function ruleMatches(rule: PolicyRule, tool: string, subject?: string | null): boolean {
  if (!globMatches(rule.tool, tool)) return false;
  const pattern = rule.pattern;
  if (pattern === undefined || pattern === null) return true;
  if (subject === undefined || subject === null) return false;
  // `g(p, s) || (s.starts_with(p.trim_end_matches('*')) && p.ends_with('*'))`
  return globMatches(pattern, subject) || (pattern.endsWith("*") && subject.startsWith(pattern.replace(/\*+$/, "")));
}

/** Rule written the way people write it: `shell(git status*)` or `write_file`. */
export function ruleLabel(rule: Pick<PolicyRule, "tool" | "pattern">): string {
  return rule.pattern ? `${rule.tool}(${rule.pattern})` : rule.tool;
}

export interface EvaluateInput {
  mode: PolicyMode | string;
  rules: PolicyRule[];
  tool: string;
  risk: ToolRisk;
  subject?: string | null;
  /** The user already approved this tool for the session ("consenti sempre"). */
  sessionAllowed?: boolean;
  /** The OS can contain shell commands on this machine (matters for `auto`). */
  sandboxAvailable?: boolean;
  /**
   * How the backend reads a shell command line (`POST /api/policy/analyze`): the simple commands it is made of.
   * Without it a shell command is judged as the single command it is written as.
   */
  analysis?: CommandAnalysis | null;
}

/** Message of an `ask` that comes from a command line the allow rules only cover in part. */
export const partialCover = (): string => t("security.comando_composto_o_non_analizzabile");

const SHELL_TOOL = "shell";

/**
 * `Policy::evaluate`: deny rules beat allow rules, which beat ask rules; then the mode decides.
 *
 * For the shell tool the command line is split into its simple commands: a deny or ask rule applies when it matches the
 * raw text or ANY simple command; an allow rule with a pattern applies only when the parse is confident and EVERY simple
 * command is allowed by some allow rule (a rule without a pattern allows the tool as a whole).
 */
export function evaluate(input: EvaluateInput): Decision {
  const { mode, rules, tool, risk, subject, sessionAllowed = false, sandboxAvailable = true } = input;
  const split: CommandAnalysis | null =
    tool === SHELL_TOOL && typeof subject === "string" ? (input.analysis ?? { commands: [subject.trim()].filter(Boolean), also: [], confident: true }) : null;
  const toolMatches = (r: PolicyRule) => globMatches(r.tool, tool);
  const hasPattern = (r: PolicyRule) => r.pattern !== undefined && r.pattern !== null;
  const hits = (r: PolicyRule): boolean =>
    ruleMatches(r, tool, subject) || (split !== null && hasPattern(r) && toolMatches(r) && [...split.commands, ...split.also].some((c) => ruleMatches(r, tool, c)));

  const denied = rules.findIndex((r) => r.action === "deny" && hits(r));
  if (denied >= 0) return { kind: "deny", reason: t("security.regola", { rule: ruleLabel(rules[denied]) }), by: "rule", ruleIndex: denied };

  let allowed = -1;
  if (split === null) {
    allowed = rules.findIndex((r) => r.action === "allow" && ruleMatches(r, tool, subject));
  } else {
    allowed = rules.findIndex((r) => r.action === "allow" && toolMatches(r) && !hasPattern(r));
    if (allowed < 0 && split.confident && split.commands.length > 0) {
      const covering = split.commands.map((c) => rules.findIndex((r) => r.action === "allow" && toolMatches(r) && hasPattern(r) && ruleMatches(r, tool, c)));
      if (covering.every((i) => i >= 0)) allowed = covering[0];
    }
  }
  if (allowed >= 0) return { kind: "allow", reason: t("security.regola", { rule: ruleLabel(rules[allowed]) }), by: "rule", ruleIndex: allowed };

  const asked = rules.findIndex((r) => r.action === "ask" && hits(r));
  if (asked >= 0) return { kind: "ask", reason: t("security.regola_di_conferma"), by: "rule", ruleIndex: asked };
  if (sessionAllowed) return { kind: "allow", reason: t("security.gia_consentito_per_questa_sessione"), by: "session" };
  const allow = (reason: string): Decision => ({ kind: "allow", reason, by: "mode" });
  const partial =
    split !== null && rules.some((r) => r.action === "allow" && toolMatches(r) && hasPattern(r)) && (!split.confident || split.commands.length !== 1);
  const ask = (reason: string): Decision => ({ kind: "ask", reason: partial ? partialCover() : reason, by: "mode" });
  switch (mode) {
    case "yolo":
      return allow(tr("security.modalita_senza_freni_nessuna_conferma"));
    case "plan":
      return risk === "read" || risk === "network"
        ? allow(tr("security.modalita_piano_lettura_consentita"))
        : { kind: "deny", reason: t("security.modalita_piano_solo_lettura_proponi"), by: "mode" };
    case "auto":
      if (risk === "computer") return ask(tr("security.mouse_keyboard_action"));
      if (risk === "exec" && !sandboxAvailable) return ask(tr("security.comando_senza_sandbox_su_questo"));
      return allow(tr("security.modalita_auto_dentro_la_sandbox"));
    case "auto-edit":
      if (risk === "exec") return ask(tr("security.command_execution"));
      if (risk === "computer") return ask(tr("security.mouse_keyboard_action"));
      return allow(tr("security.modalita_modifica_lettura_e_scrittura"));
    default:
      if (risk === "read" || risk === "network") return allow(tr("security.lettura_consentita"));
      if (risk === "write") return ask(tr("security.file_edit"));
      if (risk === "exec") return ask(tr("security.command_execution"));
      return ask(tr("security.mouse_keyboard_action"));
  }
}

/** What the rule tester is asked: a tool call as the agent would make it, under a given mode. */
export interface Probe {
  tool: string;
  subject: string;
  mode: PolicyMode;
  /** Risk of the tool; the tester fills it from the tool catalogue when it knows the tool. */
  risk: ToolRisk;
  sessionAllowed: boolean;
  sandboxAvailable: boolean;
}

export interface ProbeResult {
  decision: Decision;
  /** Positions of every rule that applies to the call, in list order. */
  matches: number[];
}

/** Runs the draft rules against a probe; null while no tool is named. */
export function runProbe(rules: readonly PolicyRule[], probe: Probe, analysis?: CommandAnalysis | null): ProbeResult | null {
  const tool = probe.tool.trim();
  if (!tool) return null;
  const subject = probe.subject.trim() === "" ? null : probe.subject;
  // Rules still being written (no tool yet) take no part in the test.
  const live = rules.map((r) => (r.tool.trim() ? r : { ...r, tool: "\u0000" }));
  const decision = evaluate({ mode: probe.mode, rules: live, tool, risk: probe.risk, subject, sessionAllowed: probe.sessionAllowed, sandboxAvailable: probe.sandboxAvailable, analysis });
  const parts = tool === SHELL_TOOL && analysis ? [...analysis.commands, ...analysis.also] : [];
  const matches = live.flatMap((r, i) => (ruleMatches(r, tool, subject) || (r.pattern && parts.some((c) => ruleMatches(r, tool, c))) ? [i] : []));
  return { decision, matches };
}

export const DECISION_LABEL: Record<DecisionKind, string> = { get allow() { return tr("security.consentito"); }, get ask() { return tr("security.chiede_conferma"); }, get deny() { return tr("security.negato"); } };

export const ACTION_LABEL: Record<PolicyAction, string> = { get allow() { return tr("security.consenti"); }, get ask() { return tr("security.chiedi"); }, get deny() { return tr("security.nega"); } };

/** Moves the item at `from` by `delta` positions (clamped); returns a new array. */
export function moveItem<T>(list: readonly T[], from: number, delta: number): T[] {
  const to = Math.min(list.length - 1, Math.max(0, from + delta));
  if (from < 0 || from >= list.length || to === from) return [...list];
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

export interface RuleIssue {
  index: number;
  level: "error" | "warn";
  message: string;
}

const ACTIONS: readonly PolicyAction[] = ["allow", "ask", "deny"];

/**
 * Problems of the rule list: the backend refuses (400) a rule with no tool, an unknown action or a glob it cannot
 * parse (`PolicyRule::validated`), so those block saving here too; a duplicate is only a warning.
 */
export function checkRules(rules: readonly PolicyRule[], problem: (glob: string) => string | null): RuleIssue[] {
  const issues: RuleIssue[] = [];
  rules.forEach((r, index) => {
    const tool = r.tool.trim();
    if (!tool) issues.push({ index, level: "error", message: t("security.indica_il_tool_a_cui") });
    else {
      const p = problem(tool);
      if (p) issues.push({ index, level: "error", message: t("security.il_tool_non_e_un", { tool, error: p }) });
    }
    if (!ACTIONS.includes(r.action)) issues.push({ index, level: "error", message: t("security.azione_non_valida_usa_allow", { action: String(r.action) }) });
    const pattern = r.pattern?.trim();
    if (pattern) {
      const p = problem(pattern);
      if (p) issues.push({ index, level: "error", message: t("security.il_pattern_non_e_un", { pattern, error: p }) });
    }
  });
  const seen = new Map<string, number>();
  rules.forEach((r, index) => {
    const key = `${r.tool}\u0000${r.pattern ?? ""}\u0000${r.action}`;
    const first = seen.get(key);
    if (first === undefined) seen.set(key, index);
    else issues.push({ index, level: "warn", message: t("security.uguale_alla_regola", { number: first + 1 }) });
  });
  return issues;
}

/** Rules as sent to the backend: trimmed, empty pattern as null. */
export function normalizeRules(rules: readonly PolicyRule[]): PolicyRule[] {
  return rules.map((r) => ({ tool: r.tool.trim(), pattern: r.pattern?.trim() ? r.pattern.trim() : null, action: r.action }));
}
