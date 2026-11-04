import type { ModelProviderInfo, PullRequest } from "../../../shared/api/types";

// Validation and request building of the two hand-written forms: direct URL and provider settings.

export function isHttpUrl(value: string): boolean {
  try {
    const u = new URL(value.trim());
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname !== "";
  } catch {
    return false;
  }
}

export interface UrlFormValues {
  url: string;
  fileName: string;
  sha256: string;
}

export type UrlFormErrors = Partial<Record<keyof UrlFormValues, string>>;

const SHA256 = /^[0-9a-f]{64}$/i;
/** A bare file name: no folders, no parent references, no control characters. */
const FILE_NAME = /^[^\\/:*?"<>|\u0000-\u001f]+$/;

/** Errors of the direct URL form; an empty object means it can be submitted. */
export function validateUrlForm(v: UrlFormValues): UrlFormErrors {
  const e: UrlFormErrors = {};
  if (!v.url.trim()) e.url = "Inserisci l'indirizzo del file.";
  else if (!isHttpUrl(v.url)) e.url = "Serve un indirizzo che inizia con http:// o https://.";
  const name = v.fileName.trim();
  if (name && (!FILE_NAME.test(name) || name === "." || name === "..")) e.fileName = "Solo il nome del file, senza cartelle.";
  const sha = v.sha256.trim();
  if (sha && !SHA256.test(sha)) e.sha256 = "Lo sha256 ha 64 caratteri esadecimali (0-9, a-f).";
  return e;
}

/** `{url, file_name?, sha256?}`: empty optional fields are left out, the hash is lowercased. */
export function buildUrlRequest(v: UrlFormValues): PullRequest {
  const req: { url: string; file_name?: string; sha256?: string } = { url: v.url.trim() };
  if (v.fileName.trim()) req.file_name = v.fileName.trim();
  if (v.sha256.trim()) req.sha256 = v.sha256.trim().toLowerCase();
  return req;
}

export interface AuthFormValues {
  token: string;
  endpoint: string;
  /** The user asked to go back to the provider's default endpoint. */
  resetEndpoint: boolean;
}

/** Body fields of `PUT …/auth`: `undefined` means "leave as it is" (the field is not sent). */
export interface AuthPayload {
  token: string | undefined;
  endpoint: string | undefined;
}

/**
 * Translates the settings form into the backend semantics (docs/architecture/model-hub.md §9):
 * an empty token box leaves the stored token alone, a changed endpoint is sent as typed, and
 * "ripristina" sends the empty string, which restores the default.
 */
export function authPayload(v: AuthFormValues, current: Pick<ModelProviderInfo, "endpoint">): AuthPayload {
  const token = v.token.trim();
  let endpoint: string | undefined;
  if (v.resetEndpoint) endpoint = "";
  else if (v.endpoint.trim() !== current.endpoint) endpoint = v.endpoint.trim();
  return { token: token || undefined, endpoint };
}

export function hasAuthChanges(p: AuthPayload): boolean {
  return p.token !== undefined || p.endpoint !== undefined;
}

/** Error of the endpoint box, or null: empty is allowed (it means the default), otherwise http(s). */
export function validateEndpoint(value: string): string | null {
  const t = value.trim();
  if (!t) return null;
  return isHttpUrl(t) ? null : "Serve un indirizzo che inizia con http:// o https://.";
}

/** Host that receives the token, for the reassurance line of the dialog ("huggingface.co"). */
export function hostOf(endpoint: string): string | null {
  try {
    return new URL(endpoint).hostname || null;
  } catch {
    return null;
  }
}
