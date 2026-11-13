// Companion of i18n-extract.mjs: writes the English wording of the messages it just added and
// regenerates the catalog index files (src/shared/i18n/it.ts and en.ts).
//
//   node scripts/i18n-apply-en.mjs translations.txt   lines "n|English" (n from the extract output)
//   node scripts/i18n-apply-en.mjs --index            only regenerate it.ts / en.ts
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const I18N = new URL("../src/shared/i18n", import.meta.url).pathname;
const MSG = join(I18N, "messages");
const ident = (area) => area.replace(/[^A-Za-z0-9]/g, "_");

function readEntries(file) {
  const m = new Map();
  if (!existsSync(file)) return m;
  for (const line of readFileSync(file, "utf8").split("\n")) {
    const x = line.match(/^\s*("[^"]+"):\s*(".*"),?\s*$/);
    if (x) m.set(JSON.parse(x[1]), JSON.parse(x[2]));
  }
  return m;
}
function writeEn(area, entries) {
  mkdirSync(join(MSG, "en"), { recursive: true });
  const body = [...entries].map(([k, v]) => `  ${JSON.stringify(k)}: ${JSON.stringify(v)},`).join("\n");
  writeFileSync(
    join(MSG, "en", `${area}.ts`),
    `// English messages of "${area}". One entry per line; must have exactly the keys of the Italian file.\nimport type { ${ident(area)}It } from "../it/${area}";\n\nexport const ${ident(area)}En: Record<keyof typeof ${ident(area)}It, string> = {\n${body}\n};\n`,
  );
}

const arg = process.argv[2];
if (arg && arg !== "--index") {
  const manifest = JSON.parse(readFileSync(join(process.env.I18N_SCRATCH ?? tmpdir(), "i18n-manifest.json"), "utf8"));
  const byN = new Map(manifest.map((e) => [e.n, e]));
  const given = new Map();
  for (const line of readFileSync(arg, "utf8").split("\n")) {
    const i = line.indexOf("|");
    if (i < 0) continue;
    const n = Number(line.slice(0, i));
    if (Number.isInteger(n)) given.set(n, line.slice(i + 1).trim());
  }
  const partial = process.argv.includes("--partial");
  const missing = manifest.filter((e) => !given.get(e.n));
  if (missing.length && !partial) {
    console.error(`Missing English for: ${missing.map((e) => e.n).join(", ")}`);
    process.exit(1);
  }
  const areas = new Map();
  for (const e of manifest) {
    if (!given.get(e.n)) continue;
    if (!areas.has(e.area)) areas.set(e.area, readEntries(join(MSG, "en", `${e.area}.ts`)));
    // keep the edge spaces of the Italian text ("Da fare: "), which the n|text lines cannot carry
    const itText = readEntries(join(MSG, "it", `${e.area}.ts`)).get(e.key) ?? "";
    const lead = itText.match(/^\s*/)[0];
    const trail = itText.match(/\s*$/)[0];
    areas.get(e.area).set(e.key, lead + given.get(e.n) + (itText.trim() ? trail : ""));
  }
  for (const [area, entries] of areas) writeEn(area, entries);
}

// index files
const areas = readdirSync(join(MSG, "it")).filter((f) => f.endsWith(".ts")).map((f) => f.slice(0, -3)).sort();
writeFileSync(
  join(I18N, "it.ts"),
  `// Italian catalog: the source of truth. Keys are typed (\`MessageKey\`); \`en.ts\` must provide every one.\n${areas.map((a) => `import { ${ident(a)}It } from "./messages/it/${a}";`).join("\n")}\n\nexport const it = {\n${areas.map((a) => `  ...${ident(a)}It,`).join("\n")}\n} as const;\n\nexport type MessageKey = keyof typeof it;\n`,
);
writeFileSync(
  join(I18N, "en.ts"),
  `// English catalog. A key missing here (or one that does not exist in it.ts) is a type error.\n//\n// Glossary (keep the terms consistent):\n//   Assistente -> Assistant        Agente -> Agent            Insegna -> Teach\n//   Proposte -> Suggestions        Panoramica -> Home         Workspace (chat) -> Chat\n//   Modelli -> Models              Impostazioni -> Settings   Sicurezza -> Security\n//   Documenti -> Documents         Canali -> Channels         Dispositivi -> Devices\n//   Attività -> Activity           Conferma / approvazione -> Approval\n//   Ricordi -> Memories            Lezione -> Lesson          Workflow, Team, Runtime, MCP: unchanged\nimport type { MessageKey } from "./it";\n${areas.map((a) => `import { ${ident(a)}En } from "./messages/en/${a}";`).join("\n")}\n\nexport const en: Record<MessageKey, string> = {\n${areas.map((a) => `  ...${ident(a)}En,`).join("\n")}\n};\n`,
);
console.log(`index regenerated for ${areas.length} areas: ${areas.join(", ")}`);
