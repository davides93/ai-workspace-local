#!/usr/bin/env node
// Time to the first usable screen of the mockup (demo transport, opened from the file system).
// It only REPORTS: there is no threshold, the numbers depend on the machine. See docs/benchmarks.md.
//
// "Usable" = the page title (h1) and the navigation are on screen, no loading skeleton is left and the
// fonts are in. Measured in the page, from the start of the navigation (performance.now() = 0), in a
// fresh browser context per run (nothing cached). First contentful paint is reported as well.
//
//   npm run build:mockup && npm run bench:startup
//   npm run bench:startup -- --runs=15 --json
//
// `AIW_SMOKE_HTML` points the script at another build, like the smoke test.
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright");
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.resolve(process.env.AIW_SMOKE_HTML ?? path.join(root, "dist-mockup", "index.html"));
const arg = (name) => process.argv.find((a) => a.startsWith(`--${name}=`))?.slice(name.length + 3);
const runs = Number(arg("runs") ?? 7);
const asJson = process.argv.includes("--json");

if (!existsSync(htmlPath)) {
  console.error(`Missing ${htmlPath}: run "npm run build:mockup" first.`);
  process.exit(2);
}

const SIZES = [
  { name: "1440x900", viewport: { width: 1440, height: 900 } },
  { name: "390x844", viewport: { width: 390, height: 844 } },
];

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const browser = await chromium.launch();
const out = [];
try {
  for (const size of SIZES) {
    const usable = [];
    const fcp = [];
    const dcl = [];
    for (let i = 0; i < runs; i++) {
      const ctx = await browser.newContext({ viewport: size.viewport, locale: "it-IT" });
      await ctx.addInitScript(() => {
        window.__aiwUsableAt = undefined;
        const check = () => {
          if (window.__aiwUsableAt !== undefined) return;
          const ready = document.querySelector("h1") && document.querySelector("nav") && !document.querySelector('[aria-label="Caricamento"]');
          if (ready) window.__aiwUsableAt = performance.now();
        };
        new MutationObserver(check).observe(document, { childList: true, subtree: true });
      });
      const page = await ctx.newPage();
      await page.goto(`${pathToFileURL(htmlPath).href}#/`);
      await page.waitForFunction(() => window.__aiwUsableAt !== undefined, null, { timeout: 20_000 });
      await page.evaluate(() => document.fonts.ready);
      const m = await page.evaluate(() => ({
        usable: window.__aiwUsableAt,
        fcp: performance.getEntriesByName("first-contentful-paint")[0]?.startTime,
        dcl: performance.getEntriesByType("navigation")[0]?.domContentLoadedEventEnd,
      }));
      usable.push(m.usable);
      if (m.fcp !== undefined) fcp.push(m.fcp);
      if (m.dcl !== undefined) dcl.push(m.dcl);
      await ctx.close();
    }
    const row = { viewport: size.name, runs, usable_ms: { median: median(usable), min: Math.min(...usable), max: Math.max(...usable) }, fcp_ms_median: fcp.length ? median(fcp) : null, dom_content_loaded_ms_median: dcl.length ? median(dcl) : null };
    out.push(row);
  }
} finally {
  await browser.close();
}

const machine = { cpu: os.cpus()[0]?.model, cores: os.cpus().length, platform: `${os.platform()} ${os.arch()}`, node: process.version };
if (asJson) {
  console.log(JSON.stringify({ machine, results: out }, null, 2));
} else {
  console.log(`Time to first usable screen of the mockup (${runs} runs each, fresh context, indicative only)`);
  console.log(`machine: ${machine.cpu} x${machine.cores}, ${machine.platform}, node ${machine.node}\n`);
  for (const r of out) {
    const f = (n) => (n === null ? "n/a" : `${Math.round(n)} ms`);
    console.log(`${r.viewport}: usable median ${f(r.usable_ms.median)} (min ${f(r.usable_ms.min)}, max ${f(r.usable_ms.max)}); first contentful paint ${f(r.fcp_ms_median)}; DOMContentLoaded ${f(r.dom_content_loaded_ms_median)}`);
  }
}
