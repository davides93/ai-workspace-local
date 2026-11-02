// Pure mirror of crates/aiw-agent/src/policy.rs (ADR 0005): given the approval mode, the rules and
// what the tool is about to do, say whether it is allowed, needs a confirmation or is denied.
// Keep it in step with the Rust file: the test panel of the Security page relies on it.

import type { PolicyAction, PolicyMode, PolicyRule, ToolRisk } from "../../../shared/api/types";
import { globMatches } from "./glob";

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
}

/** `Policy::evaluate`: deny rules beat allow rules, which beat ask rules; then the mode decides. */
export function evaluate(input: EvaluateInput): Decision {
  const { mode, rules, tool, risk, subject, sessionAllowed = false, sandboxAvailable = true } = input;
  for (const action of ["deny", "allow", "ask"] as const) {
    const index = rules.findIndex((r) => r.action === action && ruleMatches(r, tool, subject));
    if (index < 0) continue;
    const rule = rules[index];
    if (action === "deny") return { kind: "deny", reason: `regola: ${ruleLabel(rule)}`, by: "rule", ruleIndex: index };
    if (action === "allow") return { kind: "allow", reason: `regola: ${ruleLabel(rule)}`, by: "rule", ruleIndex: index };
    return { kind: "ask", reason: "regola di conferma", by: "rule", ruleIndex: index };
  }
  if (sessionAllowed) return { kind: "allow", reason: "già consentito per questa sessione", by: "session" };
  const allow = (reason: string): Decision => ({ kind: "allow", reason, by: "mode" });
  const ask = (reason: string): Decision => ({ kind: "ask", reason, by: "mode" });
  switch (mode) {
    case "yolo":
      return allow("modalità senza freni: nessuna conferma");
    case "plan":
      return risk === "read" || risk === "network"
        ? allow("modalità piano: lettura consentita")
        : { kind: "deny", reason: "modalità piano: solo lettura, proponi le modifiche invece di eseguirle", by: "mode" };
    case "auto":
      if (risk === "computer") return ask("azione su mouse e tastiera");
      if (risk === "exec" && !sandboxAvailable) return ask("comando senza sandbox su questo sistema");
      return allow("modalità auto: dentro la sandbox");
    case "auto-edit":
      if (risk === "exec") return ask("esecuzione di un comando");
      if (risk === "computer") return ask("azione su mouse e tastiera");
      return allow("modalità modifica: lettura e scrittura consentite");
    default:
      if (risk === "read" || risk === "network") return allow("lettura consentita");
      if (risk === "write") return ask("modifica di file");
      if (risk === "exec") return ask("esecuzione di un comando");
      return ask("azione su mouse e tastiera");
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
export function runProbe(rules: readonly PolicyRule[], probe: Probe): ProbeResult | null {
  const tool = probe.tool.trim();
  if (!tool) return null;
  const subject = probe.subject.trim() === "" ? null : probe.subject;
  // Rules still being written (no tool yet) take no part in the test.
  const live = rules.map((r) => (r.tool.trim() ? r : { ...r, tool: "\u0000" }));
  const decision = evaluate({ mode: probe.mode, rules: live, tool, risk: probe.risk, subject, sessionAllowed: probe.sessionAllowed, sandboxAvailable: probe.sandboxAvailable });
  const matches = live.flatMap((r, i) => (ruleMatches(r, tool, subject) ? [i] : []));
  return { decision, matches };
}

export const DECISION_LABEL: Record<DecisionKind, string> = { allow: "Consentito", ask: "Chiede conferma", deny: "Negato" };

export const ACTION_LABEL: Record<PolicyAction, string> = { allow: "Consenti", ask: "Chiedi", deny: "Nega" };

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
    if (!tool) issues.push({ index, level: "error", message: "Indica il tool a cui si applica la regola." });
    else {
      const p = problem(tool);
      if (p) issues.push({ index, level: "error", message: `Il tool «${tool}» non è un glob valido (${p}).` });
    }
    if (!ACTIONS.includes(r.action)) issues.push({ index, level: "error", message: `Azione «${String(r.action)}» non valida: usa allow, ask o deny.` });
    const pattern = r.pattern?.trim();
    if (pattern) {
      const p = problem(pattern);
      if (p) issues.push({ index, level: "error", message: `Il pattern «${pattern}» non è un glob valido (${p}).` });
    }
  });
  const seen = new Map<string, number>();
  rules.forEach((r, index) => {
    const key = `${r.tool}\u0000${r.pattern ?? ""}\u0000${r.action}`;
    const first = seen.get(key);
    if (first === undefined) seen.set(key, index);
    else issues.push({ index, level: "warn", message: `Uguale alla regola ${first + 1}.` });
  });
  return issues;
}

/** Rules as sent to the backend: trimmed, empty pattern as null. */
export function normalizeRules(rules: readonly PolicyRule[]): PolicyRule[] {
  return rules.map((r) => ({ tool: r.tool.trim(), pattern: r.pattern?.trim() ? r.pattern.trim() : null, action: r.action }));
}
