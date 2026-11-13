// Lists the user-visible literals still hard-coded in JSX (outside src/shared/i18n): text between tags,
// string expressions `{"..."}` and the string value of text-bearing attributes. It measures
// how much of the interface still has to move into the message catalogs.
//
//   npm run i18n:scan              summary per feature folder
//   npm run i18n:scan -- --list    every literal with file:line
//   npm run i18n:scan -- --json    machine readable summary
//
// Limits: only JSX is scanned. Strings in plain .ts modules (label maps, validators, toasts
// built outside components) are not counted; tests and mock fixtures are skipped.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import ts from "typescript";

const ROOT = process.env.I18N_SCAN_ROOT ?? new URL("../src", import.meta.url).pathname;
const TEXT_ATTRS = new Set([
  "aria-label", "aria-description", "aria-placeholder", "placeholder", "title", "alt", "label", "description", "hint", "helper",
  "empty", "emptyText", "tooltip", "caption", "heading", "subtitle", "eyebrow", "text", "message", "confirmLabel", "cancelLabel",
  "submitLabel", "legend", "summary", "error", "note", "sub", "badge", "actionLabel", "okLabel", "addLabel", "saveLabel", "valuePlaceholder", "blocker", "subject", "noun",
]);

const letters = /\p{L}/u;

const files = [];
(function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) {
      if (f === "i18n" || f === "mock") continue;
      walk(p);
    } else if (p.endsWith(".tsx") && !p.endsWith(".test.tsx")) files.push(p);
  }
})(ROOT);

const hits = [];
for (const file of files) {
  const src = readFileSync(file, "utf8");
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const rel = relative(ROOT, file).split(sep).join("/");
  const add = (node, text, kind) => {
    const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf));
    hits.push({ file: rel, line: line + 1, kind, text: text.replace(/\s+/g, " ").trim() });
  };
  const visit = (node) => {
    if (ts.isJsxText(node)) {
      const text = node.getText(sf);
      if (letters.test(text) && !/^[\s]*[{}]/.test(text)) add(node, text, "text");
    } else if (ts.isJsxExpression(node) && node.expression && node.parent && (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))) {
      const e = node.expression;
      if ((ts.isStringLiteral(e) || ts.isNoSubstitutionTemplateLiteral(e)) && letters.test(e.text)) add(node, e.text, "expr");
    } else if (ts.isJsxAttribute(node) && node.initializer) {
      const name = node.name.getText(sf);
      const tag = node.parent?.parent?.tagName?.getText(sf) ?? "";
      const init = node.initializer;
      const lit = ts.isStringLiteral(init) ? init : ts.isJsxExpression(init) && init.expression && (ts.isStringLiteral(init.expression) || ts.isNoSubstitutionTemplateLiteral(init.expression)) ? init.expression : null;
      if (lit && TEXT_ATTRS.has(name) && letters.test(lit.text)) add(node, `${name}="${lit.text}"`, "attr");
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
}

const area = (f) => {
  const [a, b] = f.split("/");
  return a === "features" ? `features/${b}` : a === "shared" ? `shared/${f.split("/")[1]}` : a;
};
const byArea = new Map();
for (const h of hits) byArea.set(area(h.file), (byArea.get(area(h.file)) ?? 0) + 1);
const rows = [...byArea.entries()].sort((a, b) => b[1] - a[1]);

if (process.argv.includes("--json")) {
  console.log(JSON.stringify({ total: hits.length, byArea: Object.fromEntries(rows) }, null, 2));
} else {
  if (process.argv.includes("--list")) for (const h of hits) console.log(`${h.file}:${h.line}  [${h.kind}] ${h.text.slice(0, 90)}`);
  console.log("\nJSX literals still outside the catalogs:");
  for (const [a, n] of rows) console.log(`  ${String(n).padStart(5)}  ${a}`);
  console.log(`  ${String(hits.length).padStart(5)}  TOTAL (${files.length} .tsx files scanned)`);
}
