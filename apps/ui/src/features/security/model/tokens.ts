import { currentLocaleTag } from "../../../shared/lib/format";
import type { ApiTokenInfo } from "../../../shared/api/types";

import { t as tr } from "../../../shared/i18n";
export const EXPIRY_CHOICES = [
  { days: 0, get label() { return tr("security.non_scade"); } },
  { days: 30, get label() { return tr("security.30_giorni"); } },
  { days: 90, get label() { return tr("security.90_giorni"); } },
  { days: 365, get label() { return tr("security.1_anno"); } },
];

export type TokenState = "active" | "revoked" | "expired";

export function tokenState(t: Pick<ApiTokenInfo, "revoked_at" | "expires_at">, now = Date.now()): TokenState {
  if (t.revoked_at) return "revoked";
  if (t.expires_at && Date.parse(t.expires_at) <= now) return "expired";
  return "active";
}

const fmt = (iso: string) => new Date(iso).toLocaleString(currentLocaleTag(), { dateStyle: "short", timeStyle: "short" });

export const describeLastUse = (t: Pick<ApiTokenInfo, "last_used_at" | "last_used_ip">) =>
  t.last_used_at ? tr(t.last_used_ip ? "security.last_use_from" : "security.ultimo_uso", { when: fmt(t.last_used_at), ip: t.last_used_ip ?? "" }) : tr("security.mai_usato");

export const describeExpiry = (t: Pick<ApiTokenInfo, "expires_at">) => (t.expires_at ? tr("security.scade_il", { date: fmt(t.expires_at) }) : tr("security.non_scade_2"));

/** How an app that speaks the OpenAI API is pointed at this computer. */
export const exampleFor = (token: string, base = "http://127.0.0.1:7777") =>
  `export OPENAI_BASE_URL=${base}/v1\nexport OPENAI_API_KEY=${token}`;
