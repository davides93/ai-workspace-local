// Model roles of an agent: Automatico / Un solo modello / Personalizzato. Pure, tested without a DOM.

import type { AgentProfile, AgentRoles, ModelRole, RoleProfiles } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
export const ROLE_KEYS: readonly ModelRole[] = ["plan", "act", "check", "summarize"];

export type RolesMode = "single" | "auto" | "custom";

export const ROLE_INFO: Record<ModelRole, { label: string; hint: string; empty: string }> = {
  plan: { get label() { return t("agents.pianifica"); }, get hint() { return t("agents.il_primo_passo_e_quello"); }, get empty() { return t("agents.come_l_agente"); } },
  act: { get label() { return t("agents.esegue"); }, get hint() { return t("agents.i_passi_che_proseguono_dopo"); }, get empty() { return t("agents.come_l_agente"); } },
  check: { get label() { return t("agents.verifica"); }, get hint() { return t("agents.un_controllo_finale_prima_di"); }, get empty() { return t("agents.nessuna_verifica"); } },
  summarize: { get label() { return t("agents.riassume"); }, get hint() { return t("agents.accorcia_la_conversazione_quando_diventa"); }, get empty() { return t("agents.come_l_agente"); } },
};

export const MODES: readonly { value: RolesMode; label: string; hint: string }[] = [
  { value: "auto", get label() { return t("agents.automatico"); }, get hint() { return t("agents.decide_l_app_in_base"); } },
  { value: "single", get label() { return t("agents.un_solo_modello"); }, get hint() { return t("agents.tutto_il_lavoro_lo_fa"); } },
  { value: "custom", get label() { return t("agents.personalizzato"); }, get hint() { return t("agents.scegli_tu_un_profilo_per"); } },
];

export function rolesMode(r: AgentRoles | undefined): RolesMode {
  if (r === undefined) return "single";
  return r === "auto" ? "auto" : "custom";
}

/** Switching to "Personalizzato" keeps what was picked before; the other two modes carry nothing. */
export function withRolesMode(a: AgentProfile, mode: RolesMode): AgentProfile {
  const { roles, ...rest } = a;
  if (mode === "single") return rest;
  if (mode === "auto") return { ...rest, roles: "auto" };
  return { ...rest, roles: typeof roles === "object" ? roles : {} };
}

export function withRole(a: AgentProfile, role: ModelRole, profileId: string): AgentProfile {
  const current: RoleProfiles = typeof a.roles === "object" ? a.roles : {};
  const next: RoleProfiles = { ...current };
  if (profileId) next[role] = profileId;
  else delete next[role];
  return { ...a, roles: next };
}

/** The roles as the backend keeps them: empty picks dropped, an empty custom plan means "one model". */
export function normalizeRoles(r: AgentRoles | undefined): AgentRoles | undefined {
  if (r === undefined || r === "auto") return r;
  const out: RoleProfiles = {};
  for (const k of ROLE_KEYS) if (r[k]) out[k] = r[k];
  return Object.keys(out).length > 0 ? out : undefined;
}

/** One line for the list or the summary: what the roles mean for this agent. */
export function rolesSummary(r: AgentRoles | undefined): string {
  const mode = rolesMode(normalizeRoles(r));
  if (mode === "single") return t("agents.un_solo_modello");
  if (mode === "auto") return t("agents.ruoli_automatici");
  const n = ROLE_KEYS.filter((k) => typeof r === "object" && r[k]).length;
  return t("agents.ruoli_personalizzati", { n });
}
