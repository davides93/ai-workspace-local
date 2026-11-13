#!/usr/bin/env node
// Smoke test of the mockup build: opens dist-mockup/index.html on every route of the route table
// (src/app/routes.tsx), at 1440x900 and 390x844, in light and dark theme, and fails on
//   - console errors and uncaught page errors,
//   - the error screen of a view ("Questa pagina non si è caricata"),
//   - horizontal overflow (the document and the scrolling page area),
//   - a theme that is not the one asked for.
// Every visit runs in both interface languages (Italian and English); in English the 390 px layout is
// the one that matters most (longer words), and <html lang> must follow the language.
// It builds nothing: run `npm run build:mockup` first.
//
//   npm run test:smoke                       every route, every size and theme
//   npm run test:smoke -- --route=runtimes   only the routes whose path starts with /runtimes
//   npm run test:smoke -- --lang=en          only one language (it | en; default: both)
//   npm run test:smoke -- --shots=<dir>      also save a screenshot of every visit
//
// `AIW_SMOKE_HTML` points the script at another build.

import { createRequire } from "node:module";
import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { collectRoutePaths } from "./lib/routes.mjs";

// createRequire (not `import`) so a globally installed playwright (NODE_PATH) works too.
const { chromium } = createRequire(import.meta.url)("playwright");

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.resolve(process.env.AIW_SMOKE_HTML ?? path.join(root, "dist-mockup", "index.html"));

const arg = (name) => process.argv.find((a) => a.startsWith(`--${name}=`))?.slice(name.length + 3);
const onlyRoute = arg("route");
const shotsDir = arg("shots");
const LANGS = arg("lang") ? [arg("lang")] : ["it", "en"];

const SIZES = [
  { name: "1440", viewport: { width: 1440, height: 900 } },
  { name: "390", viewport: { width: 390, height: 844 } },
];
const THEMES = ["light", "dark"];

const { paths: allPaths, warnings } = collectRoutePaths(root);
const routePaths = allPaths.filter((p) => !onlyRoute || p.startsWith(`/${onlyRoute.replace(/^\//, "")}`));

if (!existsSync(htmlPath)) {
  console.error(`Manca ${path.relative(process.cwd(), htmlPath)}: esegui prima "npm run build:mockup".`);
  process.exit(2);
}
if (routePaths.length === 0) {
  console.error(`Nessuna rotta corrisponde a --route=${onlyRoute}.`);
  process.exit(2);
}
if (shotsDir) mkdirSync(shotsDir, { recursive: true });


async function visit(page, routePath, size, theme, lang, problems, errors) {
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
      lang: document.documentElement.lang,
      crashed: /Questa pagina non si è caricata|This page did not load/.test(document.body.innerText),
      empty: !document.querySelector(".page, .ws, .th, .kb, .chat, .boot"),
    };
  });
  const tag = `${routePath} · ${size.name} · ${theme} · ${lang}`;
  const fail = (msg) => problems.push(`${tag}: ${msg}`);
  if (m.lang !== lang) fail(`<html lang> è "${m.lang}" invece di "${lang}"`);
  if (m.theme !== theme) fail(`tema "${m.theme}" invece di "${theme}"`);
  if (m.docOverflow > 1) fail(`scroll orizzontale del documento (+${m.docOverflow}px)`);
  if (m.pageOverflow > 1) fail(`scroll orizzontale dell'area pagina (+${m.pageOverflow}px)`);
  if (m.crashed) fail("la vista mostra la schermata di errore");
  if (m.empty) fail("nessun contenuto di pagina");
  for (const e of errors) fail(e);
  if (shotsDir) {
    const file = `${routePath.replace(/\W+/g, "_").replace(/^_+|_+$/g, "") || "home"}-${size.name}-${theme}-${lang}.png`;
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
  for (const lang of LANGS) for (const size of SIZES) {
    for (const theme of THEMES) {
      const ctx = await browser.newContext({ viewport: size.viewport, colorScheme: theme, locale: lang === "it" ? "it-IT" : "en-US" });
      await ctx.addInitScript(([t, l]) => {
        try {
          localStorage.setItem("aiw.theme", t);
          localStorage.setItem("aiw.ui.language", l);
        } catch {
          /* storage unavailable */
        }
      }, [theme, lang]);
      const page = await ctx.newPage();
      const errors = [];
      page.on("console", (msg) => {
        if (msg.type() !== "error") return;
        errors.push(`errore in console: ${msg.text()}`);
      });
      // The app is local-first: a build opened from a file must not reach for any network origin (fonts included).
      page.on("request", (req) => {
        if (/^https?:/.test(req.url())) errors.push(`richiesta di rete verso ${req.url()}`);
      });
      page.on("pageerror", (e) => errors.push(`errore non gestito: ${e.message}`));
      page.on("crash", () => errors.push("la pagina è andata in crash"));

      for (const routePath of routePaths) {
        const before = problems.length;
        try {
          await visit(page, routePath, size, theme, lang, problems, errors);
        } catch (e) {
          problems.push(`${routePath} · ${size.name} · ${theme} · ${lang}: ${e.message}`);
        }
        visits++;
        const row = results.get(routePath) ?? new Map();
        row.set(`${lang} ${size.name} ${theme}`, problems.length === before);
        results.set(routePath, row);
      }
      await ctx.close();
    }
  }
} finally {
  await browser.close();
}

// ---------------------------------------------------------------- summary
const cols = LANGS.flatMap((l) => SIZES.flatMap((s) => THEMES.map((t) => `${l} ${s.name} ${t}`)));
const width = Math.max(...routePaths.map((r) => r.length), 5) + 2;
console.log(`\n${"rotta".padEnd(width)}${cols.map((c) => c.padEnd(15)).join("")}`);
for (const r of routePaths) {
  console.log(`${r.padEnd(width)}${cols.map((c) => (results.get(r)?.get(c) ? "ok" : "ERRORE").padEnd(15)).join("")}`);
}
for (const w of warnings) console.warn(`attenzione: ${w}`);
const seconds = ((Date.now() - started) / 1000).toFixed(1);
console.log(`\n${routePaths.length} rotte x ${LANGS.length} lingue x ${SIZES.length} dimensioni x ${THEMES.length} temi = ${visits} visite in ${seconds} s`);
if (problems.length > 0) {
  console.error(`\n${problems.length} problemi:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}
console.log("Smoke test superato: nessun errore in console, nessuno scroll orizzontale.");
