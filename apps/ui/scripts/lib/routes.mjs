// Concrete route paths of the app, read from the route table (src/app/routes.tsx) so the list cannot
// drift. Shared by the smoke test (scripts/smoke.mjs) and the accessibility tests (e2e/a11y).
import { readFileSync } from "node:fs";
import path from "node:path";

/**
 * Values to try for the `:param` segments of the route table, by `<route id>/<param>`. A route
 * with an optional parameter is always visited without it as well. A parameter missing here is
 * reported as a warning, so a new parametric route is not silently left untested.
 */
export const SAMPLES = {
  "chat/sessionId": ["ses_pdf", "ses_bug"],
  "agents/agentId": ["coder"],
  "teams/teamId": ["revisione"],
  "workflows/workflowId": ["inbox-digest"],
  "evals/tab": ["suites", "reports"],
  "training/tab": ["jobs", "datasets", "adapters"],
  "capabilities/tab": ["tools", "mcp", "browser"],
  "models/tab": ["installed", "discover", "downloads", "catalog"],
  "runtimes/runtimeId": ["llama-server", "vllm"],
  "security/tab": ["policy", "hooks", "sandbox"],
};
/** Paths that no route matches: the "not found" page. */
export const EXTRA_PATHS = ["/questa-pagina-non-esiste"];

/** `{ id, path }` of every entry of the route table, read from the source so the list cannot drift. */
function readRouteTable(root) {
  const src = readFileSync(path.join(root, "src", "app", "routes.tsx"), "utf8");
  const routes = [...src.matchAll(/\{\s*id:\s*"(\w+)",\s*path:\s*"([^"]+)"/g)].map((m) => ({ id: m[1], path: m[2] }));
  if (routes.length === 0) throw new Error("Nessuna rotta trovata in src/app/routes.tsx: il formato della tabella è cambiato?");
  return routes;
}

/** Concrete paths of one route: the bare path, then one per sample of each parameter. */
function expandRoute({ id, path: pattern }, warnings) {
  const segments = pattern.split("/").filter(Boolean);
  const staticSegs = segments.filter((s) => !s.startsWith(":"));
  const params = segments.filter((s) => s.startsWith(":"));
  const base = `/${staticSegs.join("/")}`;
  const out = new Set();
  const hasRequired = params.some((p) => !p.endsWith("?"));
  if (!hasRequired) out.add(base);
  for (const p of params) {
    const name = p.slice(1).replace(/\?$/, "");
    const samples = SAMPLES[`${id}/${name}`];
    if (!samples) {
      warnings.push(`${id}: nessun valore di prova per :${name} (aggiungilo a SAMPLES in scripts/lib/routes.mjs)`);
      continue;
    }
    for (const v of samples) out.add(`${base === "/" ? "" : base}/${encodeURIComponent(v)}`);
  }
  return [...out];
}


/**
 * Every path to visit: the routes of the table with sample parameters, plus the "not found" page.
 * `warnings` lists the parametric routes that have no sample value.
 */
export function collectRoutePaths(root) {
  const warnings = [];
  const paths = [...new Set([...readRouteTable(root).flatMap((r) => expandRoute(r, warnings)), ...EXTRA_PATHS])];
  return { paths, warnings };
}
