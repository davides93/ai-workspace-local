/**
 * API tokens are bound to the origin of the server that issued them. A link such as
 * `…/?server=https://evil.example` must never make the UI send a token obtained from the user's
 * own workspace to another host.
 */

/** Origin of `server` (the `?server=` value; empty = the page's own origin), or null if invalid. */
export function serverOrigin(server: string, pageHref: string): string | null {
  try {
    return new URL(server || "/", pageHref).origin;
  } catch {
    return null;
  }
}

export const tokenKey = (origin: string) => `aiw.token:${origin}`;

/** Token stored for `origin`. The legacy unscoped key only counts for the page's own origin. */
export function storedToken(storage: Pick<Storage, "getItem"> | null, origin: string | null, pageOrigin: string): string | null {
  if (!storage || !origin) return null;
  try {
    return storage.getItem(tokenKey(origin)) ?? (origin === pageOrigin ? storage.getItem("aiw.token") : null);
  } catch {
    return null;
  }
}

export function storeToken(storage: Pick<Storage, "setItem"> | null, origin: string | null, token: string): void {
  if (!storage || !origin) return;
  try {
    storage.setItem(tokenKey(origin), token);
  } catch {
    /* storage unavailable: the token lives only in memory */
  }
}

export function safeStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
