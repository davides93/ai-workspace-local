// Fails when a production build references a third-party origin.
//
// A local-first app must work offline and must not phone home on every start, so the
// built page may not load fonts, scripts, styles or images from anywhere but itself.
//
//  - HTML: no http(s) in src/href/srcset, no @import, no url(http...).
//  - CSS: no http(s) URL at all.
//  - JS: every http(s) origin found in a string must be on the allowlist below (user
//    clickable links, examples/placeholders, local endpoints, XML namespaces) and
//    must not look like a CDN/font host. The app itself only fetches from its own base.
//
// Usage: node scripts/check-no-external.mjs [dir ...]   (default: dist)
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, extname } from "node:path";

/** Hosts that may appear as text in JS, each with the reason. */
const ALLOWED_HOSTS = [
  [/^www\.w3\.org$/, "XML/SVG/MathML namespace identifiers (not requests)"],
  [/^react\.dev$/, "React error decoder link inside an error message (not requested)"],
  [/^(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])$/, "local endpoints (engine, runtimes, Tauri)"],
  [/^(10|192\.168|172\.(1[6-9]|2\d|3[01]))\./, "private LAN addresses used as placeholders/examples"],
  [/(^|\.)(example|test|local|localhost|invalid|esempio)(\.[a-z]+)?$/, "reserved/placeholder example hostnames"],
  [/^[^.]+$/, "single-label hosts (placeholders)"],
  [/^(huggingface\.co|hf-mirror\.com|www\.modelscope\.cn|ollama\.com|registry\.ollama\.ai)$/, "model hubs: link targets and default mirrors the person triggers explicitly"],
  [/^github\.com$/, "release / project links opened on click"],
];
const BANNED = /(googleapis|gstatic|googleusercontent|jsdelivr|unpkg|cdnjs|cloudflare|fontawesome|typekit|bootstrapcdn|fonts\.)/i;

const files = [];
const walk = (p) => {
  const s = statSync(p);
  if (s.isDirectory()) readdirSync(p).forEach((f) => walk(join(p, f)));
  else files.push(p);
};
const dirs = process.argv.slice(2);
for (const d of dirs.length ? dirs : ["dist"]) walk(d);

const problems = [];
const origins = new Map();
for (const f of files) {
  const ext = extname(f);
  if (![".html", ".css", ".js", ".mjs"].includes(ext)) continue;
  const text = readFileSync(f, "utf8");
  if (ext === ".html") {
    for (const m of text.matchAll(/\b(?:src|href|srcset|poster|action)\s*=\s*["']\s*(?:https?:)?\/\/[^"']+/gi)) problems.push(`${f}: external reference ${m[0]}`);
    for (const m of text.matchAll(/@import\s+(?:url\()?["']?(?:https?:)?\/\//gi)) problems.push(`${f}: external @import`);
    for (const m of text.matchAll(/url\(\s*["']?(?:https?:)?\/\/[^)]*/gi)) problems.push(`${f}: external url() ${m[0]}`);
  }
  if (ext === ".css") {
    for (const m of text.matchAll(/https?:\/\/[^\s"')]+/g)) problems.push(`${f}: URL in CSS ${m[0]}`);
  }
  if (ext === ".js" || ext === ".mjs" || ext === ".html") {
    for (const m of text.matchAll(/https?:\/\/([A-Za-z0-9.\-[\]:]*)/g)) {
      const host = m[1].replace(/:\d+$/, "").replace(/\.$/, "");
      if (!host) continue;
      origins.set(host, (origins.get(host) ?? 0) + 1);
      if (BANNED.test(host)) problems.push(`${f}: banned host ${host}`);
      else if (!ALLOWED_HOSTS.some(([re]) => re.test(host))) problems.push(`${f}: origin not on the allowlist: ${host}`);
    }
  }
}

if (problems.length) {
  console.error("Third-party origins found in the production build:\n" + [...new Set(problems)].map((p) => "  " + p).join("\n"));
  process.exit(1);
}
console.log(`check-no-external: ${files.length} files, ${origins.size} distinct hosts in text, all allowed.`);
