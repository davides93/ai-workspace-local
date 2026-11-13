// Remote agents (A2A): ids and the checks made before saving. Pure, so it can be tested without a DOM.
// The rules mirror `save_remote_agent` (crates/aiw-engine/src/a2a_client.rs).

import { SECRET_MASK, type RemoteAgent } from "../../../shared/api/types";
import { slugify, uniqueSlug } from "../../../shared/lib/slug";

import { t } from "../../../shared/i18n";
/** Ids that start like this belong to the paired devices. */
export const PEER_PREFIX = "peer-";

export const blankRemote = (): RemoteAgent => ({ id: "", name: "", url: "", token: "", description: "" });

export const remoteIdFor = (name: string, existing: readonly RemoteAgent[]): string => {
  const base = slugify(name, "agente");
  return uniqueSlug(base.startsWith(PEER_PREFIX) ? `remoto-${base}` : base, existing.map((r) => r.id));
};

/** What the form sends: trimmed, without an empty skill. */
export function normalizeRemote(r: RemoteAgent): RemoteAgent {
  const skill = r.skill?.trim();
  const out: RemoteAgent = { id: r.id, name: r.name.trim(), url: r.url.trim(), token: r.token, description: r.description.trim() };
  if (skill) out.skill = skill;
  return out;
}

export interface RemoteIssue { field: "name" | "url"; message: string }

export function validateRemote(r: RemoteAgent): RemoteIssue[] {
  const issues: RemoteIssue[] = [];
  if (!r.name.trim()) issues.push({ field: "name", message: t("integrations.dai_un_nome_all_agente") });
  const url = r.url.trim();
  if (!url) issues.push({ field: "url", message: t("integrations.indica_l_indirizzo_dell_agente") });
  else if (!/^https?:\/\/[^/\s]+/i.test(url)) issues.push({ field: "url", message: t("integrations.l_indirizzo_deve_iniziare_con") });
  return issues;
}

export const hasToken = (r: RemoteAgent): boolean => r.token === SECRET_MASK || r.token !== "";
