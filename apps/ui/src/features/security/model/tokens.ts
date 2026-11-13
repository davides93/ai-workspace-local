import { currentLocaleTag } from "../../../shared/lib/format";
import type { ApiTokenInfo } from "../../../shared/api/types";

export const EXPIRY_CHOICES = [
  { days: 0, label: "Non scade" },
  { days: 30, label: "30 giorni" },
  { days: 90, label: "90 giorni" },
  { days: 365, label: "1 anno" },
];

export type TokenState = "active" | "revoked" | "expired";

export function tokenState(t: Pick<ApiTokenInfo, "revoked_at" | "expires_at">, now = Date.now()): TokenState {
  if (t.revoked_at) return "revoked";
  if (t.expires_at && Date.parse(t.expires_at) <= now) return "expired";
  return "active";
}

const fmt = (iso: string) => new Date(iso).toLocaleString(currentLocaleTag(), { dateStyle: "short", timeStyle: "short" });

export const describeLastUse = (t: Pick<ApiTokenInfo, "last_used_at" | "last_used_ip">) =>
  t.last_used_at ? `ultimo uso ${fmt(t.last_used_at)}${t.last_used_ip ? ` da ${t.last_used_ip}` : ""}` : "mai usato";

export const describeExpiry = (t: Pick<ApiTokenInfo, "expires_at">) => (t.expires_at ? `scade il ${fmt(t.expires_at)}` : "non scade");

/** How an app that speaks the OpenAI API is pointed at this computer. */
export const exampleFor = (token: string, base = "http://127.0.0.1:7777") =>
  `export OPENAI_BASE_URL=${base}/v1\nexport OPENAI_API_KEY=${token}`;
