// Teams: defaults, ids, normalization and the checks made before saving. Pure, so it can be tested
// without a DOM. The rules mirror `save_team` (crates/aiw-engine/src/lib.rs).

import { REMOTE_PREFIX, TEAM_PREFIX, type AgentProfile, type RemoteAgent, type TeamDef, type TeamMember, type TeamStrategy } from "../../../shared/api/types";
import { slugify as slug, uniqueSlug } from "../../../shared/lib/slug";
import { cleanBudget } from "../../agents/model/agents";

import { t as tr, tFields } from "../../../shared/i18n";
/** URL segment of a team that does not exist yet (`#/teams/new`); never used as a real id. */
export const NEW_TEAM = "new";
/** Most members a team can have. */
export const MAX_MEMBERS = 8;

export const STRATEGIES: { id: TeamStrategy; label: string; hint: string }[] = [
  tFields({ id: "supervisor" as const }, { label: "teams.strategy_supervisor", hint: "teams.strategy_supervisor_hint" }),
  tFields({ id: "parallel" as const }, { label: "teams.strategy_parallel", hint: "teams.strategy_parallel_hint" }),
  tFields({ id: "pipeline" as const }, { label: "teams.strategy_pipeline", hint: "teams.strategy_pipeline_hint" }),
];

export const strategyInfo = (id: string) => STRATEGIES.find((s) => s.id === id);

/** `team:<id>` is how a chat session names a team as its agent. */
export const teamAgentId = (teamId: string): string => `${TEAM_PREFIX}${teamId}`;
export const isTeamAgentId = (agentId: string): boolean => agentId.startsWith(TEAM_PREFIX);
export const teamIdOf = (agentId: string): string | null => (isTeamAgentId(agentId) ? agentId.slice(TEAM_PREFIX.length) : null);
export const isRemoteAgent = (agentId: string): boolean => agentId.startsWith(REMOTE_PREFIX);

/** `Revisione` → `revisione`; never the reserved `new`. */
export const slugify = (name: string, fallback = "team"): string => slug(name, fallback);
export const idForNew = (name: string, existing: readonly TeamDef[]): string =>
  uniqueSlug(slugify(name), existing.map((t) => t.id), [NEW_TEAM]);

/** A new team starts as a chain of nothing: the user adds members. */
export function blankTeam(): TeamDef {
  return { id: "", name: "", description: "", strategy: "supervisor", members: [] };
}

const copyName = (name: string, existing: readonly string[]): string => {
  const base = tr("teams.copy_of", { name });
  const taken = new Set(existing.map((n) => n.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base} (${n})`.toLowerCase())) return `${base} (${n})`;
};

export const duplicateTeam = (src: TeamDef, existing: readonly TeamDef[]): TeamDef => ({
  ...structuredClone(src),
  id: "",
  name: copyName(src.name, existing.map((t) => t.name)),
});

/** Form values trimmed the way the backend will keep them; fields that do not apply to the strategy are dropped. */
export function normalizeTeam(t: TeamDef): TeamDef {
  const { budget: _budget, supervisor: _supervisor, aggregator: _aggregator, ...rest } = t;
  const budget = cleanBudget(t.budget);
  const supervisor = t.strategy === "supervisor" ? t.supervisor?.trim() || undefined : undefined;
  const aggregator = t.strategy === "parallel" ? t.aggregator?.trim() || undefined : undefined;
  return {
    ...rest,
    name: t.name.trim(),
    description: t.description.trim(),
    members: t.members.map((m): TeamMember => ({ agent: m.agent.trim(), role: m.role.trim() })),
    ...(supervisor ? { supervisor } : {}),
    ...(aggregator ? { aggregator } : {}),
    ...(budget ? { budget } : {}),
  };
}

export type TeamField = "name" | "supervisor" | "members" | "aggregator" | "budget";

export interface TeamIssue {
  field: TeamField;
  level: "error" | "warn";
  message: string;
}

export interface TeamContext {
  /** Ids of the local agents. */
  agentIds: readonly string[];
  /** The agent list was loaded (an empty list may just not have arrived). */
  agentsLoaded?: boolean;
  otherNames: readonly string[];
}

/** The rules of `save_team`: a known strategy, 1–8 distinct members that exist, a supervisor that is not a member. */
export function validateTeam(t: TeamDef, ctx: TeamContext): TeamIssue[] {
  const issues: TeamIssue[] = [];
  const name = t.name.trim();
  if (!name) issues.push({ field: "name", level: "error", message: tr("teams.dai_un_nome_al_team") });
  else if (ctx.otherNames.some((n) => n.trim().toLowerCase() === name.toLowerCase())) {
    issues.push({ field: "name", level: "warn", message: tr("teams.esiste_gia_un_altro_team") });
  }
  const loaded = ctx.agentsLoaded ?? ctx.agentIds.length > 0;
  const missing = (id: string) => loaded && !isRemoteAgent(id) && !ctx.agentIds.includes(id);
  if (t.members.length === 0) issues.push({ field: "members", level: "error", message: tr("teams.aggiungi_almeno_un_membro") });
  if (t.members.length > MAX_MEMBERS) issues.push({ field: "members", level: "error", message: tr("teams.un_team_ha_al_massimo", { max: MAX_MEMBERS }) });
  const seen = new Set<string>();
  for (const m of t.members) {
    if (!m.agent) issues.push({ field: "members", level: "error", message: tr("teams.scegli_l_agente_di_ogni") });
    else if (seen.has(m.agent)) issues.push({ field: "members", level: "error", message: tr("teams.l_agente_compare_piu_volte", { agent: m.agent }) });
    else if (missing(m.agent)) issues.push({ field: "members", level: "error", message: tr("teams.l_agente_non_esiste_piu", { agent: m.agent }) });
    seen.add(m.agent);
  }
  if (t.strategy === "supervisor") {
    if (!t.supervisor) issues.push({ field: "supervisor", level: "error", message: tr("teams.scegli_il_supervisore") });
    else if (missing(t.supervisor)) issues.push({ field: "supervisor", level: "error", message: tr("teams.il_supervisore_non_esiste_piu", { supervisor: t.supervisor }) });
    else if (seen.has(t.supervisor)) issues.push({ field: "supervisor", level: "error", message: tr("teams.il_supervisore_non_puo_essere") });
  }
  if (t.strategy === "parallel" && t.aggregator && missing(t.aggregator)) {
    issues.push({ field: "aggregator", level: "error", message: tr("teams.l_aggregatore_non_esiste_piu", { aggregator: t.aggregator }) });
  }
  for (const v of [t.budget?.max_tokens, t.budget?.max_turns, t.budget?.max_seconds]) {
    if (v !== undefined && Number.isFinite(v) && (!Number.isInteger(v) || v < 1)) {
      issues.push({ field: "budget", level: "error", message: tr("teams.i_limiti_del_budget_sono") });
      break;
    }
  }
  return issues;
}

export const hasErrors = (issues: readonly TeamIssue[]): boolean => issues.some((i) => i.level === "error");

/** One choice of the member picker: a local agent, or a remote one. */
export interface AgentOption {
  id: string;
  name: string;
  remote?: boolean;
}

export const localOptions = (agents: readonly AgentProfile[]): AgentOption[] => agents.map((a) => ({ id: a.id, name: a.name }));

/** The remote agents (registered ones and paired devices) as `remote:<id>` choices. */
export const remoteOptions = (remotes: readonly RemoteAgent[]): AgentOption[] =>
  remotes.map((r) => ({ id: `${REMOTE_PREFIX}${r.id}`, name: r.name, remote: true }));

/** Display name of an agent id of a team: the agent's name, `remote:` ids as they are written. */
export const agentLabel = (id: string, options: readonly AgentOption[]): string => options.find((o) => o.id === id)?.name ?? id;

/**
 * The profile the chat header shows for a team session, as the engine runs it
 * (crates/aiw-engine/src/lib.rs `session_agent`): a supervisor team runs as its supervisor; the other
 * strategies have no model in charge, so the first member's profile stands in under the team's name, with the
 * widest mode and sandbox among the local agents involved.
 */
export function teamAsAgent(team: TeamDef, agents: readonly AgentProfile[]): AgentProfile | null {
  const named = { id: teamAgentId(team.id), name: team.name, description: team.description };
  const supervisor = team.strategy === "supervisor" ? agents.find((a) => a.id === team.supervisor) : undefined;
  if (supervisor) return { ...supervisor, ...named };
  const ids = [...team.members.map((m) => m.agent), ...(team.aggregator ? [team.aggregator] : [])];
  const local = ids.map((id) => agents.find((a) => a.id === id)).filter((a): a is AgentProfile => !!a);
  const lead = local[0];
  if (!lead) return null;
  const MODES = ["plan", "ask", "auto-edit", "auto", "yolo"];
  const SANDBOXES = ["read-only", "workspace-write", "off"];
  const widest = <K extends "mode" | "sandbox">(key: K, order: string[]): AgentProfile[K] =>
    local.reduce((w, a) => (order.indexOf(a[key]) > order.indexOf(w) ? a[key] : w), lead[key]);
  return { ...lead, ...named, mode: widest("mode", MODES), sandbox: widest("sandbox", SANDBOXES) };
}
