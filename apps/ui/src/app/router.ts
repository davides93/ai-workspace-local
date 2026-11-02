import { createContext, createElement, useCallback, useContext, useEffect, useMemo, useSyncExternalStore, type AnchorHTMLAttributes, type ReactNode } from "react";

// Hash router written in-house (no dependency). Locations look like `#/models/discover?provider=hf&q=qwen`.
// Everything up to `useRoute` is pure so it can be unit tested without a DOM.

export type Params = Record<string, string>;
export type Query = Record<string, string>;

/** One entry of the route table: `path` may contain `:param` and optional `:param?` segments. */
export interface RoutePattern {
  id: string;
  path: string;
}

export interface Resolved<T extends RoutePattern = RoutePattern> {
  route: T;
  params: Params;
  query: Query;
  /** Normalized path without query ("/models/discover"). */
  path: string;
}

/** Splits `#/a/b?x=1` into a normalized path and the query. */
export function parseHash(hash: string): { path: string; query: Query } {
  let h = hash.startsWith("#") ? hash.slice(1) : hash;
  const qi = h.indexOf("?");
  const search = qi >= 0 ? h.slice(qi + 1) : "";
  if (qi >= 0) h = h.slice(0, qi);
  const path = normalizePath(h);
  const query: Query = {};
  for (const [k, v] of new URLSearchParams(search)) query[k] = v;
  return { path, query };
}

/** Leading slash, no trailing slash, no empty segments; the root is "/". */
export function normalizePath(p: string): string {
  const segs = p.split("/").filter(Boolean);
  return `/${segs.join("/")}`;
}

/** Matches a path against a pattern; returns the decoded params or null. */
export function matchPath(pattern: string, path: string): Params | null {
  const pat = pattern.split("/").filter(Boolean);
  const segs = path.split("/").filter(Boolean);
  const params: Params = {};
  for (let i = 0; i < pat.length; i++) {
    const p = pat[i];
    if (p.startsWith(":")) {
      const optional = p.endsWith("?");
      const name = p.slice(1, optional ? -1 : undefined);
      const seg = segs[i];
      if (seg === undefined) {
        if (optional) continue;
        return null;
      }
      try {
        params[name] = decodeURIComponent(seg);
      } catch {
        params[name] = seg;
      }
    } else if (segs[i] !== p) {
      return null;
    }
  }
  return segs.length > pat.length ? null : params;
}

/** First route of the table whose pattern matches the hash. */
export function resolveRoute<T extends RoutePattern>(routes: readonly T[], hash: string): Resolved<T> | null {
  const { path, query } = parseHash(hash);
  for (const route of routes) {
    const params = matchPath(route.path, path);
    if (params) return { route, params, query, path };
  }
  return null;
}

/** Builds a concrete path from a pattern: `/chat/:sessionId?` + `{sessionId: "a b"}` → `/chat/a%20b`. */
export function buildPath(pattern: string, params: Params = {}, query: Query = {}): string {
  const out: string[] = [];
  for (const seg of pattern.split("/").filter(Boolean)) {
    if (!seg.startsWith(":")) {
      out.push(seg);
      continue;
    }
    const optional = seg.endsWith("?");
    const value = params[seg.slice(1, optional ? -1 : undefined)];
    if (value === undefined || value === "") {
      if (optional) continue;
      throw new Error(`Parametro mancante per ${pattern}`);
    }
    out.push(encodeURIComponent(value));
  }
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) if (v !== "") q.set(k, v);
  const qs = q.toString();
  return `/${out.join("/")}${qs ? `?${qs}` : ""}`;
}

/** Old section hashes (`#chat`, `#tools`, `#preferences`…) and where they live now. */
const LEGACY: Record<string, string> = {
  chat: "/chat",
  agents: "/agents",
  workflows: "/workflows",
  runtimes: "/runtimes",
  models: "/models",
  devices: "/devices",
  tools: "/capabilities/tools",
  activity: "/activity",
  preferences: "/settings",
};

/**
 * Redirect target of a legacy hash, or null when the hash is already in the new format.
 * An empty hash goes to the home page.
 */
export function legacyRedirect(hash: string): string | null {
  const h = hash.startsWith("#") ? hash.slice(1) : hash;
  if (h === "" || h === "/") return h === "" && hash !== "" ? "#/" : null;
  if (h.startsWith("/")) return null;
  const key = h.split("?")[0].toLowerCase();
  const target = LEGACY[key];
  if (target) return `#${target}${h.includes("?") ? `?${h.split("?")[1]}` : ""}`;
  return `#/${h}`;
}

// ---------------------------------------------------------------- React bindings

function subscribe(cb: () => void) {
  window.addEventListener("hashchange", cb);
  return () => window.removeEventListener("hashchange", cb);
}
const snapshot = () => location.hash;

/** Current location hash; redirects legacy hashes once on the way. */
export function useHash(): string {
  const hash = useSyncExternalStore(subscribe, snapshot, () => "");
  useEffect(() => {
    const target = legacyRedirect(hash);
    if (target !== null) navigate(target, { replace: true });
  }, [hash]);
  return hash;
}

/** Resolves the current hash against the route table. `null` = unknown path (not found). */
export function useRoute<T extends RoutePattern>(routes: readonly T[]): Resolved<T> | null {
  const hash = useHash();
  return useMemo(() => resolveRoute(routes, hash), [routes, hash]);
}

/** Goes to `to` (`/models/discover?q=x` or `#/…`). `replace` does not add a history entry. */
export function navigate(to: string, opts: { replace?: boolean } = {}): void {
  const hash = to.startsWith("#") ? to : `#${to.startsWith("/") ? to : `/${to}`}`;
  if (location.hash === hash) return;
  if (opts.replace) {
    history.replaceState(null, "", `${location.pathname}${location.search}${hash}`);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  } else {
    location.hash = hash;
  }
}

export function useNavigate(): typeof navigate {
  return useCallback((to: string, opts?: { replace?: boolean }) => navigate(to, opts), []);
}

/** Updates the query string of the current path keeping the rest (filters, search). Empty values are removed. */
export function setQuery(patch: Record<string, string | undefined>, opts: { replace?: boolean } = { replace: true }): void {
  const { path, query } = parseHash(location.hash);
  const next: Query = { ...query };
  for (const [k, v] of Object.entries(patch)) {
    if (v === undefined || v === "") delete next[k];
    else next[k] = v;
  }
  const q = new URLSearchParams(next).toString();
  navigate(`${path}${q ? `?${q}` : ""}`, opts);
}

/** Anchor to an in-app path; it is a plain `<a href="#/…">` so it works with middle click and without JS handlers. */
export function Link({ to, children, ...rest }: { to: string; children?: ReactNode } & Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href">) {
  const href = to.startsWith("#") ? to : `#${to.startsWith("/") ? to : `/${to}`}`;
  return createElement("a", { href, ...rest }, children);
}

// ---------------------------------------------------------------- params for the rendered view

interface RouteState {
  params: Params;
  query: Query;
}
const RouteContext = createContext<RouteState>({ params: {}, query: {} });

/** Wraps the rendered view so it can read the params and query of its own route. */
export const RouteProvider = RouteContext.Provider;

/** `:param` values of the current route (`sessionId` of `/chat/:sessionId?`). */
export const useParams = (): Params => useContext(RouteContext).params;

/** Query string values of the current location (`?provider=hf&q=qwen`). */
export const useQueryParams = (): Query => useContext(RouteContext).query;
