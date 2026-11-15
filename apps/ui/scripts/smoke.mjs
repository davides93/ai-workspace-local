#!/usr/bin/env node
// Smoke test of the mockup build: opens dist-mockup/index.html on every route of the route table
// (src/app/routes.tsx), at 1440x900 and 390x844, in light and dark theme, and fails on
//   - console errors and uncaught page errors,
//   - the error screen of a view ("Questa pagina non si è caricata"),
//   - horizontal overflow (the document and the scrolling page area),
//   - a theme that is not the one asked for,
//   - a route that did not open as asked: the routes are visited in developer mode (`?ui=developer`, the simple
//     interface has the chat only and sends every other address to it), so the address must still be the route's
//     after the page has settled. Without that check a broken redirect to the chat would pass as 19 good visits.
// The simple interface is visited too: the chat, and addresses that must end up in the chat (a cold load of
// a hidden route, an old hash, an unknown path).
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

/**
 * Visits of the simple interface (no `?ui=developer`): `hash` is what is typed, `lands` the path it must end up on (a session of the chat
 * counts as the chat). Each one is a cold load, as a person who opens a link would get.
 */
const SIMPLE_VISITS = [
  { label: "(semplice) /chat", hash: "/chat", lands: "/chat" },
  { label: "(semplice) / → /chat", hash: "/", lands: "/chat" },
  { label: "(semplice) /settings → /chat", hash: "/settings", lands: "/chat" },
  { label: "(semplice) #tools → /chat", hash: "tools", lands: "/chat" },
  { label: "(semplice) /questa-pagina-non-esiste → /chat", hash: "/questa-pagina-non-esiste", lands: "/chat" },
].filter((v) => !onlyRoute || v.hash.replace(/^\//, "").startsWith(onlyRoute.replace(/^\//, "")));

if (!existsSync(htmlPath)) {
  console.error(`Manca ${path.relative(process.cwd(), htmlPath)}: esegui prima "npm run build:mockup".`);
  process.exit(2);
}
if (routePaths.length === 0) {
  console.error(`Nessuna rotta corrisponde a --route=${onlyRoute}.`);
  process.exit(2);
}
if (shotsDir) mkdirSync(shotsDir, { recursive: true });


/** `simple`: a visit of the simple interface that must end on `simple.lands`; otherwise a developer visit that must stay on `routePath`. */
async function visit(page, routePath, size, theme, lang, problems, errors, simple) {
  errors.length = 0;
  const base = pathToFileURL(htmlPath).href;
  // A cold load every time for the simple visits (an address typed into a fresh page); the developer visits only change the hash.
  if (simple) await page.goto("about:blank");
  await page.goto(simple ? `${base}#${routePath}` : `${base}?ui=developer#${routePath}`);
  // Going to a URL that differs only in the hash does not reload: the router handles it.
  await page.waitForSelector("#main", { timeout: 8000 }).catch(() => {
    errors.push("l'app non si è avviata in 8 s (manca #main)");
  });
  await page.waitForFunction(() => !document.querySelector('[aria-label="Caricamento"]'), null, { timeout: 8000 }).catch(() => {
    errors.push("la pagina è rimasta in caricamento (skeleton) per 8 s");
  });
  await page.waitForTimeout(250);

  const m = await page.evaluate(() => {
    const over = (el) => (el ? el.scrollWidth - el.clientWidth : 0);
    const page = document.querySelector(".page");
    return {
      hash: location.hash.replace(/^#/, "").split("?")[0],
      theme: document.documentElement.getAttribute("data-theme"),
      docOverflow: document.documentElement.scrollWidth - window.innerWidth,
      pageOverflow: over(page),
      lang: document.documentElement.lang,
      crashed: /Questa pagina non si è caricata|This page didn['’]t load/.test(document.body.innerText),
      empty: !document.querySelector(".page, .ws, .th, .kb, .chat, .boot"),
    };
  });
  const label = simple ? simple.label : routePath;
  const tag = `${label} · ${size.name} · ${theme} · ${lang}`;
  const fail = (msg) => problems.push(`${tag}: ${msg}`);
  const wanted = simple ? simple.lands : routePath;
  // The route opened as asked (a session of the chat counts as the chat).
  if (m.hash !== wanted && !m.hash.startsWith(`${wanted}/`)) fail(`l'indirizzo finale è "#${m.hash}" invece di "#${wanted}"`);
  if (m.lang !== lang) fail(`<html lang> è "${m.lang}" invece di "${lang}"`);
  if (m.theme !== theme) fail(`tema "${m.theme}" invece di "${theme}"`);
  if (m.docOverflow > 1) fail(`scroll orizzontale del documento (+${m.docOverflow}px)`);
  if (m.pageOverflow > 1) fail(`scroll orizzontale dell'area pagina (+${m.pageOverflow}px)`);
  if (m.crashed) fail("la vista mostra la schermata di errore");
  if (m.empty) fail("nessun contenuto di pagina");
  for (const e of errors) fail(e);
  if (shotsDir) {
    const file = `${(simple ? `simple-${routePath}` : routePath).replace(/\W+/g, "_").replace(/^_+|_+$/g, "") || "home"}-${size.name}-${theme}-${lang}.png`;
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

      const todo = [
        ...SIMPLE_VISITS.map((v) => ({ key: v.label, routePath: v.hash, simple: v })),
        ...routePaths.map((routePath) => ({ key: routePath, routePath, simple: undefined })),
      ];
      for (const { key, routePath, simple } of todo) {
        const before = problems.length;
        try {
          await visit(page, routePath, size, theme, lang, problems, errors, simple);
        } catch (e) {
          problems.push(`${key} · ${size.name} · ${theme} · ${lang}: ${e.message}`);
        }
        visits++;
        const row = results.get(key) ?? new Map();
        row.set(`${lang} ${size.name} ${theme}`, problems.length === before);
        results.set(key, row);
      }
      await ctx.close();
    }
  }
} finally {
  await browser.close();
}

// ---------------------------------------------------------------- summary
const cols = LANGS.flatMap((l) => SIZES.flatMap((s) => THEMES.map((t) => `${l} ${s.name} ${t}`)));
const rows = [...SIMPLE_VISITS.map((v) => v.label), ...routePaths];
const width = Math.max(...rows.map((r) => r.length), 5) + 2;
console.log(`\n${"rotta".padEnd(width)}${cols.map((c) => c.padEnd(15)).join("")}`);
for (const r of rows) {
  console.log(`${r.padEnd(width)}${cols.map((c) => (results.get(r)?.get(c) ? "ok" : "ERRORE").padEnd(15)).join("")}`);
}
for (const w of warnings) console.warn(`attenzione: ${w}`);
const seconds = ((Date.now() - started) / 1000).toFixed(1);
console.log(`\n${routePaths.length} rotte di Sviluppatore + ${SIMPLE_VISITS.length} visite della modalità semplice x ${LANGS.length} lingue x ${SIZES.length} dimensioni x ${THEMES.length} temi = ${visits} visite in ${seconds} s`);
if (problems.length > 0) {
  console.error(`\n${problems.length} problemi:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}
console.log("Smoke test superato: nessun errore in console, nessuno scroll orizzontale.");
