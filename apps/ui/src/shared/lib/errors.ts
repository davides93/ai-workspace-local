/** Human readable message of anything that was thrown. */
export function errorMessage(e: unknown): string {
  if (e instanceof Error && e.message) return e.message;
  if (typeof e === "string" && e) return e;
  if (e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string") return (e as { message: string }).message;
  return "Errore sconosciuto";
}

/** HTTP status carried by an `ApiError`, when there is one. */
export function errorStatus(e: unknown): number | undefined {
  const s = e && typeof e === "object" ? (e as { status?: unknown }).status : undefined;
  return typeof s === "number" ? s : undefined;
}

/** 401: this browser must pair with the device before it can talk to the API. */
export function isUnauthorized(e: unknown): boolean {
  return errorStatus(e) === 401;
}
