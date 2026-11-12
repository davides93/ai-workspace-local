// Model roles of an agent: Automatico / Un solo modello / Personalizzato. Pure, tested without a DOM.

import type { AgentProfile, AgentRoles, ModelRole, RoleProfiles } from "../../../shared/api/types";

export const ROLE_KEYS: readonly ModelRole[] = ["plan", "act", "check", "summarize"];

export type RolesMode = "single" | "auto" | "custom";

export const ROLE_INFO: Record<ModelRole, { label: string; hint: string; empty: string }> = {
  plan: { label: "Pianifica", hint: "Il primo passo e quello dopo un errore: il modello più capace, con il ragionamento acceso.", empty: "Come l'agente" },
  act: { label: "Esegue", hint: "I passi che proseguono dopo un risultato riuscito: può essere un modello più veloce, senza ragionamento.", empty: "Come l'agente" },
  check: { label: "Verifica", hint: "Un controllo finale prima di concludere, solo se l'agente ha modificato qualcosa. Al massimo un giro in più.", empty: "Nessuna verifica" },
  summarize: { label: "Riassume", hint: "Accorcia la conversazione quando diventa lunga: il modello più piccolo che hai.", empty: "Come l'agente" },
};

export const MODES: readonly { value: RolesMode; label: string; hint: string }[] = [
  { value: "auto", label: "Automatico", hint: "Decide l'app in base al tuo dispositivo: su un computer piccolo usa un solo modello, su uno grande può usarne due che stanno insieme in memoria." },
  { value: "single", label: "Un solo modello", hint: "Tutto il lavoro lo fa il modello scelto in «Modello». È il comportamento di sempre." },
  { value: "custom", label: "Personalizzato", hint: "Scegli tu un profilo per ogni ruolo; quelli lasciati vuoti usano il modello dell'agente." },
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
  if (mode === "single") return "Un solo modello";
  if (mode === "auto") return "Ruoli automatici";
  const n = ROLE_KEYS.filter((k) => typeof r === "object" && r[k]).length;
  return `Ruoli personalizzati (${n})`;
}
