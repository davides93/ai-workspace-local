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

/** 404: the thing is gone (a session or an MCP server deleted from another client, an unknown path). */
export function isNotFound(e: unknown): boolean {
  return errorStatus(e) === 404;
}

/** Waits for `p` and treats a 404 as success: deleting what is already gone has done what was asked. */
export async function ignoreNotFound(p: Promise<unknown>): Promise<void> {
  try {
    await p;
  } catch (e) {
    if (!isNotFound(e)) throw e;
  }
}
