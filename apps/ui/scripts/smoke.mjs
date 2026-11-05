#!/usr/bin/env node
// Smoke test of the mockup build: opens dist-mockup/index.html on every route of the route table
// (src/app/routes.tsx), at 1440x900 and 390x844, in light and dark theme, and fails on
//   - console errors and uncaught page errors,
//   - the error screen of a view ("Questa pagina non si è caricata"),
//   - horizontal overflow (the document and the scrolling page area),
//   - a theme that is not the one asked for.
// It builds nothing: run `npm run build:mockup` first.
//
//   npm run test:smoke                       every route, every size and theme
//   npm run test:smoke -- --route=runtimes   only the routes whose path starts with /runtimes
//   npm run test:smoke -- --shots=<dir>      also save a screenshot of every visit
//
// `AIW_SMOKE_HTML` points the script at another build.

import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// createRequire (not `import`) so a globally installed playwright (NODE_PATH) works too.
const { chromium } = createRequire(import.meta.url)("playwright");

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.resolve(process.env.AIW_SMOKE_HTML ?? path.join(root, "dist-mockup", "index.html"));

const arg = (name) => process.argv.find((a) => a.startsWith(`--${name}=`))?.slice(name.length + 3);
const onlyRoute = arg("route");
const shotsDir = arg("shots");

const SIZES = [
  { name: "1440", viewport: { width: 1440, height: 900 } },
  { name: "390", viewport: { width: 390, height: 844 } },
];
const THEMES = ["light", "dark"];

/**
 * Values to try for the `:param` segments of the route table, by `<route id>/<param>`. A route
 * with an optional parameter is always visited without it as well. A parameter missing here is
 * reported as a warning, so a new parametric route is not silently left untested.
 */
const SAMPLES = {
  "chat/sessionId": ["ses_pdf", "ses_bug"],
  "agents/agentId": ["coder"],
  "teams/teamId": ["revisione"],
  "workflows/workflowId": ["inbox-digest"],
  "capabilities/tab": ["tools", "mcp", "browser"],
  "models/tab": ["installed", "discover", "downloads", "catalog"],
  "runtimes/runtimeId": ["llama-server", "vllm"],
  "security/tab": ["policy", "hooks", "sandbox"],
};
/** Paths that no route matches: the "not found" page. */
const EXTRA_PATHS = ["/questa-pagina-non-esiste"];

/** `{ id, path }` of every entry of the route table, read from the source so the list cannot drift. */
function readRouteTable() {
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
      warnings.push(`${id}: nessun valore di prova per :${name} (aggiungilo a SAMPLES in scripts/smoke.mjs)`);
      continue;
    }
    for (const v of samples) out.add(`${base === "/" ? "" : base}/${encodeURIComponent(v)}`);
  }
  return [...out];
}

const warnings = [];
const routePaths = [...new Set([...readRouteTable().flatMap((r) => expandRoute(r, warnings)), ...EXTRA_PATHS])]
  .filter((p) => !onlyRoute || p.startsWith(`/${onlyRoute.replace(/^\//, "")}`));

if (!existsSync(htmlPath)) {
  console.error(`Manca ${path.relative(process.cwd(), htmlPath)}: esegui prima "npm run build:mockup".`);
  process.exit(2);
}
if (routePaths.length === 0) {
  console.error(`Nessuna rotta corrisponde a --route=${onlyRoute}.`);
  process.exit(2);
}
if (shotsDir) mkdirSync(shotsDir, { recursive: true });

/** Google Fonts are loaded from the network; offline they fail and that is not a defect of the page. */
const isFontRequest = (url = "") => /fonts\.(googleapis|gstatic)\.com/.test(url);

async function visit(page, routePath, size, theme, problems, errors) {
  errors.length = 0;
  const url = `${pathToFileURL(htmlPath).href}#${routePath}`;
  await page.goto(url);
  // Going to a URL that differs only in the hash does not reload: the router handles it.
  await page.waitForFunction(() => !document.querySelector('[aria-label="Caricamento"]'), null, { timeout: 8000 }).catch(() => {
    errors.push("la pagina è rimasta in caricamento (skeleton) per 8 s");
  });
  await page.waitForTimeout(250);

  const m = await page.evaluate(() => {
    const over = (el) => (el ? el.scrollWidth - el.clientWidth : 0);
    const page = document.querySelector(".page");
    return {
      theme: document.documentElement.getAttribute("data-theme"),
      docOverflow: document.documentElement.scrollWidth - window.innerWidth,
      pageOverflow: over(page),
      crashed: !!document.body.innerText.includes("Questa pagina non si è caricata"),
      empty: !document.querySelector(".page, .ws, .chat, .boot"),
    };
  });
  const tag = `${routePath} · ${size.name} · ${theme}`;
  const fail = (msg) => problems.push(`${tag}: ${msg}`);
  if (m.theme !== theme) fail(`tema "${m.theme}" invece di "${theme}"`);
  if (m.docOverflow > 1) fail(`scroll orizzontale del documento (+${m.docOverflow}px)`);
  if (m.pageOverflow > 1) fail(`scroll orizzontale dell'area pagina (+${m.pageOverflow}px)`);
  if (m.crashed) fail("la vista mostra la schermata di errore");
  if (m.empty) fail("nessun contenuto di pagina");
  for (const e of errors) fail(e);
  if (shotsDir) {
    const file = `${routePath.replace(/\W+/g, "_").replace(/^_+|_+$/g, "") || "home"}-${size.name}-${theme}.png`;
    await page.screenshot({ path: path.join(shotsDir, file) });
  }
}

const started = Date.now();
let browser;
try {
  browser = await chromium.launch();
} catch (e) {
  console.error(`Impossibile avviare Chromium: ${e.message}\nInstalla il browser con "npx playwright install chromium".`);
  process.exit(2);
}

const problems = [];
const results = new Map(); // route -> Map("1440 light" -> ok)
let visits = 0;

try {
  for (const size of SIZES) {
    for (const theme of THEMES) {
      const ctx = await browser.newContext({ viewport: size.viewport, colorScheme: theme });
      await ctx.addInitScript((t) => {
        try {
          localStorage.setItem("aiw.theme", t);
        } catch {
          /* storage unavailable */
        }
      }, theme);
      const page = await ctx.newPage();
      const errors = [];
      page.on("console", (msg) => {
        if (msg.type() !== "error") return;
        if (isFontRequest(msg.location().url)) return;
        errors.push(`errore in console: ${msg.text()}`);
      });
      page.on("pageerror", (e) => errors.push(`errore non gestito: ${e.message}`));
      page.on("crash", () => errors.push("la pagina è andata in crash"));

      for (const routePath of routePaths) {
        const before = problems.length;
        try {
          await visit(page, routePath, size, theme, problems, errors);
        } catch (e) {
          problems.push(`${routePath} · ${size.name} · ${theme}: ${e.message}`);
        }
        visits++;
        const row = results.get(routePath) ?? new Map();
        row.set(`${size.name} ${theme}`, problems.length === before);
        results.set(routePath, row);
      }
      await ctx.close();
    }
  }
} finally {
  await browser.close();
}

// ---------------------------------------------------------------- summary
const cols = SIZES.flatMap((s) => THEMES.map((t) => `${s.name} ${t}`));
const width = Math.max(...routePaths.map((r) => r.length), 5) + 2;
console.log(`\n${"rotta".padEnd(width)}${cols.map((c) => c.padEnd(11)).join("")}`);
for (const r of routePaths) {
  console.log(`${r.padEnd(width)}${cols.map((c) => (results.get(r)?.get(c) ? "ok" : "ERRORE").padEnd(11)).join("")}`);
}
for (const w of warnings) console.warn(`attenzione: ${w}`);
const seconds = ((Date.now() - started) / 1000).toFixed(1);
console.log(`\n${routePaths.length} rotte x ${SIZES.length} dimensioni x ${THEMES.length} temi = ${visits} visite in ${seconds} s`);
if (problems.length > 0) {
  console.error(`\n${problems.length} problemi:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}
console.log("Smoke test superato: nessun errore in console, nessuno scroll orizzontale.");
