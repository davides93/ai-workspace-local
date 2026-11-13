#!/usr/bin/env node
// Bundle size budget: fails when a build is bigger than the budget in scripts/bundle-budget.json.
// The budgets are the sizes of a reference build plus about 15 %: they catch a dependency or an
// asset that slips in by accident, not the slow growth of the app. When the app really needs more,
// raise the number in the same commit and say why.
//
//   npm run build && npm run build:mockup && npm run check:bundle
//   node scripts/check-bundle-size.mjs --report     only print the sizes (no budget check)
//
// Sizes are bytes on disk; "gzip" is the gzip (level 9) size of every .js and .css file, summed, which
// is what a browser would download from `aiw serve` (the mockup is one HTML file, measured raw).
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const budgets = JSON.parse(readFileSync(path.join(root, "scripts", "bundle-budget.json"), "utf8"));
const reportOnly = process.argv.includes("--report");

const walk = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
const kb = (n) => `${(n / 1024).toFixed(1)} KiB`;

/** Measures one build folder; `undefined` when it has not been built. */
function measure(dir) {
  const abs = path.join(root, dir);
  if (!existsSync(abs)) return undefined;
  const files = walk(abs);
  const total = files.reduce((n, f) => n + statSync(f).size, 0);
  const code = files.filter((f) => /\.(js|css|html)$/.test(f));
  const gzip = code.reduce((n, f) => n + gzipSync(readFileSync(f), { level: 9 }).length, 0);
  const biggest = files.map((f) => ({ f: path.relative(abs, f), size: statSync(f).size })).sort((a, b) => b.size - a.size)[0];
  return { total, gzip, files: files.length, biggest };
}

const problems = [];
let measured = 0;
for (const [dir, budget] of Object.entries(budgets.builds)) {
  const m = measure(dir);
  if (!m) {
    problems.push(`${dir}: not built (run the build first)`);
    continue;
  }
  measured++;
  console.log(`${dir}: ${m.files} files, ${kb(m.total)} on disk (budget ${kb(budget.total)}), ${kb(m.gzip)} gzip (budget ${kb(budget.gzip)}); biggest ${m.biggest.f} ${kb(m.biggest.size)}`);
  if (reportOnly) continue;
  if (m.total > budget.total) problems.push(`${dir}: ${m.total} bytes on disk, over the budget of ${budget.total} (+${m.total - budget.total})`);
  if (m.gzip > budget.gzip) problems.push(`${dir}: ${m.gzip} bytes gzip, over the budget of ${budget.gzip} (+${m.gzip - budget.gzip})`);
}
if (problems.length > 0) {
  console.error(`\nBundle budget:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}
console.log(measured > 0 ? "\nBundle within budget." : "\nNothing measured.");
