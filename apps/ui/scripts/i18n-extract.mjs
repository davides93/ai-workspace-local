// Migration aid: moves the simple hard-coded Italian literals of the given files/folders into the
// Italian catalog (src/shared/i18n/messages/it/<area>.ts) and rewrites the code to `t("key")`.
// The Italian wording is copied verbatim, so the Italian interface does not change.
//
//   node scripts/i18n-extract.mjs src/features/home src/app   (files or folders; .ts and .tsx)
//   node scripts/i18n-extract.mjs --dry ...                   (report only)
//
// Handled: text between tags (also text mixed with `{expressions}` -> `{name}` parameters),
// text-bearing JSX attributes, ternary/`||`/`??` branches and template literals in those
// positions, `toast("...")`, `new Error("...")` and `label:`/`title:`/... properties inside
// function bodies. Not handled (do them by hand): text mixed with JSX elements, plurals,
// module-level constants (they would be frozen in the language active at import time), HTML
// entities, strings containing `{` or `}`.
//
// It prints `n|Italian` lines for the entries it added; the English wording goes into
// messages/en/<area>.ts (see scripts/i18n-apply-en.mjs, which takes `n|English` lines).
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { tmpdir } from "node:os";
import ts from "typescript";

const SRC = resolve(new URL("../src", import.meta.url).pathname);
const MSG = join(SRC, "shared/i18n/messages");
const args = process.argv.slice(2);
const dry = args.includes("--dry");
// --sweep: also converts any user-facing string literal (multi-word or capitalised) that the targeted rules above miss.
// In function scope it becomes `t("key")`; as a property of a module-level object it becomes a getter, so it follows the
// language; anything else at module level is only reported. --skip=regex leaves matching texts alone.
const sweep = args.includes("--sweep");
const skipArg = args.find((a) => a.startsWith("--skip="));
const skipText = skipArg ? new RegExp(skipArg.slice(7)) : null;
const targets = args.filter((a) => !a.startsWith("--"));

const SKIP_ATTRS = new Set(["className", "key", "id", "htmlFor", "to", "href", "name", "type", "role", "variant", "tone", "size", "icon", "value", "data-testid", "style", "target", "rel", "autoComplete", "inputMode", "method", "kind", "mode", "align", "dir", "viewBox", "d", "fill", "stroke", "width", "height", "idPrefix", "tag", "as", "accept", "pattern", "src", "lang", "format", "layout"]);
const SKIP_PROPS = new Set(["id", "key", "kind", "type", "tone", "icon", "to", "href", "path", "url", "value", "method", "className", "class", "lang", "scope", "field", "route", "selector", "event", "format", "mode", "status", "state", "group", "level", "source", "origin", "category", "role"]);
const SKIP_CALLEES = /^(t|tr|tNode|trNode|labelMap|tFields|useT|console\.\w+|describe|it|test|expect|require|cx|clsx|classNames|fetch|invoke|open|setItem|getItem|removeItem|querySelector|querySelectorAll|setAttribute|getAttribute|addEventListener|removeEventListener|URL|RegExp|Symbol|isoDate|JSON\.parse|JSON\.stringify|encodeURIComponent|decodeURIComponent|navigate|go|setQuery|splitKeywords)$/;
const SKIP_METHODS = new Set(["includes", "startsWith", "endsWith", "indexOf", "lastIndexOf", "split", "join", "replace", "replaceAll", "match", "matchAll", "test", "localeCompare", "get", "set", "has", "add", "delete", "append", "padStart", "padEnd", "repeat", "toLowerCase", "normalize", "concat", "getItem", "setItem", "removeItem", "querySelector", "closest", "contains", "setAttribute", "getAttribute", "addEventListener", "removeEventListener", "toLocaleString", "toLocaleDateString", "toLocaleTimeString", "DateTimeFormat", "NumberFormat", "PluralRules", "ListFormat", "RelativeTimeFormat", "invalidateQueries", "setQueryData", "getQueryData", "prefetchQuery", "ensureQueryData", "open", "log", "warn", "error"]);
const TEXT_ATTRS = new Set(["data-label", "aria-label", "aria-description", "placeholder", "title", "alt", "label", "hint", "description", "subtitle", "legend", "emptyText", "empty", "confirmLabel", "addLabel", "saveLabel", "valuePlaceholder", "blocker", "subject", "noun"]);
const TEXT_PROPS = new Set(["label", "title", "description", "hint", "placeholder", "empty", "emptyText", "subtitle", "caption", "heading", "header", "message", "summary", "detail", "reason"]);
const SKIP_TAGS = new Set(["code", "kbd", "pre", "samp", "style", "script"]);
const letters = /\p{L}/u;

const files = [];
const walk = (p) => {
  if (statSync(p).isDirectory()) readdirSync(p).forEach((f) => walk(join(p, f)));
  else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p) && !p.endsWith(".d.ts") && !p.includes(`${sep}i18n${sep}`) && !p.includes(`${sep}mock${sep}`)) files.push(p);
};
targets.forEach((t) => walk(resolve(t)));

function areaOf(file) {
  const rel = relative(SRC, file).split(sep);
  if (rel[0] === "features") return rel[1];
  if (rel[0] === "shared") return rel[1] === "ui" ? "ui" : "lib";
  return "shell"; // app/, main.tsx
}

// ---- catalogs -------------------------------------------------------------------------------
const catalogs = new Map(); // area -> { entries: Map(key,text), byText: Map(text,key), added: [] }
function catalog(area) {
  if (catalogs.has(area)) return catalogs.get(area);
  const c = { entries: new Map(), byText: new Map(), added: [] };
  const f = join(MSG, "it", `${area}.ts`);
  if (existsSync(f)) {
    for (const line of readFileSync(f, "utf8").split("\n")) {
      const m = line.match(/^\s*("[^"]+"):\s*(".*"),?\s*$/);
      if (m) {
        const k = JSON.parse(m[1]);
        const v = JSON.parse(m[2]);
        c.entries.set(k, v);
        if (!c.byText.has(v)) c.byText.set(v, k);
      }
    }
  }
  catalogs.set(area, c);
  return c;
}
const slug = (text) => {
  const s = text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/\{[^}]*\}/g, " ").replace(/[^a-z0-9]+/g, " ").trim().split(" ").filter(Boolean).slice(0, 5).join("_");
  return s || "text";
};
function keyFor(area, text) {
  const c = catalog(area);
  const found = c.byText.get(text);
  if (found) return found;
  const base = `${area}.${slug(text)}`;
  let key = base;
  for (let i = 2; c.entries.has(key); i++) key = `${base}_${i}`;
  c.entries.set(key, text);
  c.byText.set(text, key);
  c.added.push(key);
  return key;
}

// ---- helpers ----------------------------------------------------------------------------------
const worthy = (text) => {
  if (!letters.test(text)) return false;
  if (/[{}]/.test(text)) return false;
  if (/&[#\w]+;/.test(text)) return false;
  const core = text.trim().replace(/[:.…?!]+$/, "");
  if (!/\s/.test(core) && /[_./\\:@]/.test(core)) return false;
  return true;
};
function jsxText(raw) {
  const lines = raw.split(/\r\n|\n|\r/);
  let out = "";
  lines.forEach((line, i) => {
    let l = line.replace(/\t/g, " ");
    if (i > 0) l = l.replace(/^ +/, "");
    if (i < lines.length - 1) l = l.replace(/ +$/, "");
    if (!l) return;
    out += (out && !out.endsWith(" ") ? " " : "") + l;
  });
  return out;
}
const containsJsx = (n) => {
  let found = false;
  (function v(x) {
    if (found) return;
    if (ts.isJsxElement(x) || ts.isJsxSelfClosingElement(x) || ts.isJsxFragment(x) || ts.isArrowFunction(x) || ts.isFunctionExpression(x)) found = true;
    else ts.forEachChild(x, v);
  })(n);
  return found;
};
const insideFunction = (n) => {
  for (let p = n.parent; p; p = p.parent) if (ts.isFunctionLike(p)) return true;
  return false;
};

// ---- per file --------------------------------------------------------------------------------
let totalEdits = 0;
const report = [];
for (const file of files) {
  const src = readFileSync(file, "utf8");
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true, file.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const area = areaOf(file);
  const usesT = (() => { let u = false; (function v(n) { if (ts.isIdentifier(n) && n.text === "t") u = true; ts.forEachChild(n, v); })(sf); return u; })();
  const fn = usesT && !/import\s*\{[^}]*\bt as tr\b/.test(src) ? "tr" : "t";
  const edits = [];
  let usesNode = false;
  let usesGetter = false;
  let skippedModule = 0;

  const paramName = (e, used) => {
    let n = ts.isIdentifier(e) ? e.text : ts.isPropertyAccessExpression(e) ? e.name.text : "";
    if (!/^[A-Za-z_]\w*$/.test(n)) n = "p";
    let name = n;
    for (let i = 2; used.has(name); i++) name = `${n}${i}`;
    used.add(name);
    return name;
  };
  const call = (text, params) => {
    const k = keyFor(area, text);
    if (!params.length) return `${fn}("${k}")`;
    return `${fn}("${k}", { ${params.map((p) => (p.name === p.expr ? p.name : `${p.name}: ${p.expr}`)).join(", ")} })`;
  };
  // expression -> replacement text, or null when it is not a convertible string
  function convertExpr(e) {
    if (ts.isParenthesizedExpression(e)) {
      const r = convertExpr(e.expression);
      return r === null ? null : `(${r})`;
    }
    if (ts.isStringLiteral(e) || ts.isNoSubstitutionTemplateLiteral(e)) return worthy(e.text) ? call(e.text, []) : null;
    if (ts.isTemplateExpression(e)) {
      let text = e.head.text;
      const params = [];
      const used = new Set();
      for (const span of e.templateSpans) {
        const ex = span.expression;
        if (ex.getText(sf).includes("\n") || containsJsx(ex)) return null;
        const name = paramName(ex, used);
        params.push({ name, expr: ex.getText(sf) });
        text += `{${name}}` + span.literal.text;
      }
      return worthy(text.replace(/\{\w+\}/g, "")) && worthy(text.replace(/\{\w+\}/g, "x")) ? call(text, params) : null;
    }
    if (ts.isConditionalExpression(e)) {
      const a = convertExpr(e.whenTrue);
      const b = convertExpr(e.whenFalse);
      if (a === null && b === null) return null;
      return `${e.condition.getText(sf)} ? ${a ?? e.whenTrue.getText(sf)} : ${b ?? e.whenFalse.getText(sf)}`;
    }
    if (ts.isBinaryExpression(e) && [ts.SyntaxKind.AmpersandAmpersandToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken].includes(e.operatorToken.kind)) {
      const r = convertExpr(e.right);
      return r === null ? null : `${e.left.getText(sf)} ${e.operatorToken.getText(sf)} ${r}`;
    }
    return null;
  }
  const replace = (node, text) => edits.push({ start: node.getStart(sf), end: node.getEnd(), text });

  // ---- sweep: strings the rules above do not reach --------------------------------------------
  const moduleLeft = [];
  const hasWorthyString = (n) => {
    let found = false;
    (function v(x) {
      if (found) return;
      if ((ts.isStringLiteral(x) || ts.isNoSubstitutionTemplateLiteral(x)) && sweepText(x.text)) found = true;
      else ts.forEachChild(x, v);
    })(n);
    return found;
  };
  function sweepText(text) {
    if (!worthy(text)) return false;
    const clean = text.trim();
    if (skipText && skipText.test(clean)) return false;
    if (/\s/.test(clean)) return !/^[\w.\/:@#~=&?%$<>,|*+-]+(\s[\w.\/:@#~=&?%$<>,|*+-]+)*$/.test(clean) || /[A-Za-zÀ-ÿ]{4,}\s+[A-Za-zÀ-ÿ]{3,}/.test(clean);
    return /^[A-ZÀ-Ù][a-zà-ù]{3,}$/.test(clean);
  }
  function sweepOk(node, text) {
    if (!sweepText(text)) return false;
    for (let p = node.parent; p; p = p.parent) {
      if (ts.isCallExpression(p) || ts.isNewExpression(p)) {
        const callee = p.expression.getText(sf);
        if (SKIP_CALLEES.test(callee)) return false;
        if (ts.isPropertyAccessExpression(p.expression) && SKIP_METHODS.has(p.expression.name.text)) return false;
      }
      if (ts.isImportDeclaration(p) || ts.isExportDeclaration(p) || ts.isTypeNode(p) || ts.isLiteralTypeNode(p)) return false;
      if (ts.isJsxAttribute(p) && SKIP_ATTRS.has(p.name.getText(sf))) return false;
      if (ts.isTaggedTemplateExpression(p)) return false;
    }
    const par = node.parent;
    if (ts.isBinaryExpression(par) && [ts.SyntaxKind.EqualsEqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsEqualsToken, ts.SyntaxKind.EqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsToken].includes(par.operatorToken.kind)) return false;
    if (ts.isElementAccessExpression(par) || ts.isCaseClause(par) || ts.isEnumMember(par)) return false;
    if (ts.isPropertyAssignment(par) && (par.name === node || SKIP_PROPS.has(par.name.getText(sf)))) return false;
    if (ts.isTemplateSpan(par)) return false;
    return true;
  }
  function trySweep(node) {
    let text;
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) text = node.text;
    else if (ts.isTemplateExpression(node)) text = node.head.text + node.templateSpans.map((x) => "x" + x.literal.text).join("");
    else return false;
    if (!sweepOk(node, text)) return false;
    if (ts.isTemplateExpression(node) && node.templateSpans.some((x) => hasWorthyString(x.expression))) {
      moduleLeft.push(`${relative(SRC, file)}:${sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1} template with nested text`);
      return false;
    }
    const r = convertExpr(node);
    if (r === null) return false;
    const mod = !insideFunction(node);
    if (!mod) { replace(node, r); return true; }
    const par = node.parent;
    if (ts.isPropertyAssignment(par) && par.initializer === node && (ts.isIdentifier(par.name) || ts.isStringLiteral(par.name))) {
      edits.push({ start: par.getStart(sf), end: par.getEnd(), text: `get ${par.name.getText(sf)}() { return ${r}; }` });
      return true;
    }
    moduleLeft.push(`${relative(SRC, file)}:${sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1} ${JSON.stringify(text.slice(0, 60))}`);
    return false;
  }

  function visit(node) {
    if (sweep && trySweep(node)) { if (!insideFunction(node) && ts.isPropertyAssignment(node.parent)) usesGetter = true; return; }
    const fnScope = insideFunction(node);
    if ((ts.isJsxElement(node) || ts.isJsxFragment(node)) && fnScope) {
      const tag = ts.isJsxElement(node) ? node.openingElement.tagName.getText(sf) : "";
      const all = [...node.children];
      const hasLetters = (k) => ts.isJsxText(k) && letters.test(k.text);
      if (!SKIP_TAGS.has(tag) && all.some(hasLetters)) {
        // Elements at the edges (an icon before the label) stay where they are; only the middle is translated.
        const isEdge = (k) => (ts.isJsxText(k) && k.text.trim() === "") || ts.isJsxSelfClosingElement(k);
        let a = 0;
        let b = all.length;
        while (a < b && isEdge(all[a])) a++;
        while (b > a && isEdge(all[b - 1])) b--;
        const mid = all.slice(a, b);
        // A "simple" element inside the text (<b>word</b>, <kbd>Esc</kbd>, <Icon/>) becomes a {slot}.
        const simpleEl = (k) =>
          ts.isJsxSelfClosingElement(k) ||
          (ts.isJsxElement(k) && k.children.length <= 1 && k.children.every((c) => ts.isJsxText(c) && !/[{}]/.test(c.text)) &&
            k.openingElement.attributes.properties.every((pr) => ts.isJsxAttribute(pr) && (!pr.initializer || ts.isStringLiteral(pr.initializer) || (ts.isJsxExpression(pr.initializer) && pr.initializer.expression && !containsJsx(pr.initializer.expression)))));
        const ok = mid.length > 0 && mid.some(hasLetters) && mid.every((k) => ts.isJsxText(k) || simpleEl(k) || (ts.isJsxExpression(k) && k.expression && !containsJsx(k.expression)));
        if (ok) {
          let text = "";
          const params = [];
          const slots = [];
          const used = new Set();
          for (const k of mid) {
            if (ts.isJsxText(k)) text += jsxText(k.text);
            else if (ts.isJsxExpression(k)) {
              const ex = k.expression;
              if (ts.isStringLiteral(ex) || ts.isNoSubstitutionTemplateLiteral(ex)) { text += ex.text; continue; }
              if (ts.isCallExpression(ex) && ["t", "tr"].includes(ex.expression.getText(sf)) && ex.arguments.length === 1 && ts.isStringLiteral(ex.arguments[0]) && catalog(area).entries.has(ex.arguments[0].text)) {
                text += catalog(area).entries.get(ex.arguments[0].text);
                continue;
              }
              if (ex.getText(sf).includes("\n")) { text = null; break; }
              const name = paramName(ex, used);
              params.push({ name, expr: ex.getText(sf) });
              text += `{${name}}`;
            } else {
              const name = paramName(ts.factory.createIdentifier(ts.isJsxElement(k) ? k.openingElement.tagName.getText(sf).toLowerCase().replace(/[^a-z0-9]/g, "") || "el" : "el"), used);
              let src = k.getText(sf);
              if (ts.isJsxElement(k) && k.children.length === 1 && ts.isJsxText(k.children[0])) {
                const inner = jsxText(k.children[0].text);
                if (worthy(inner)) src = `${k.openingElement.getText(sf)}{${call(inner, [])}}${k.closingElement.getText(sf)}`;
              }
              slots.push({ name, src });
              text += `{${name}}`;
            }
          }
          const bare = text === null ? "" : text.replace(/\{\w+\}/g, "");
          if (text !== null && worthy(bare) && !/[{}]/.test(bare)) {
            const lead = /^ /.test(text) ? '{" "}' : "";
            const trail = / $/.test(text) ? '{" "}' : "";
            const core = text.trim();
            const mk = (c) => c;
            let expr = call(core, params);
            if (slots.length) expr = `${fn === "tr" ? "trNode" : "tNode"}("${keyFor(area, core)}", { ${slots.map((sl) => `${sl.name}: ${sl.src}`).join(", ")} }${params.length ? `, { ${params.map((p) => (p.name === p.expr ? p.name : `${p.name}: ${p.expr}`)).join(", ")} }` : ""})`;
            edits.push({ start: mid[0].getStart(sf, true), end: mid[mid.length - 1].getEnd(), text: `${lead}{${mk(expr)}}${trail}` });
            usesNode ||= slots.length > 0;
            return;
          }
        }
      }
    }
    if (ts.isJsxExpression(node) && node.expression && fnScope && node.parent && (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))) {
      const r = convertExpr(node.expression);
      if (r !== null && !ts.isStringLiteral(node.expression)) { replace(node.expression, r); return; }
      if (r !== null) { replace(node.expression, r); return; }
    }
    if (ts.isJsxAttribute(node) && node.initializer && fnScope) {
      const name = node.name.getText(sf);
      if (TEXT_ATTRS.has(name)) {
        const init = node.initializer;
        if (ts.isStringLiteral(init)) {
          if (worthy(init.text) && !/&[#\w]+;/.test(init.getText(sf))) replace(init, `{${call(init.text, [])}}`);
          return;
        }
        if (ts.isJsxExpression(init) && init.expression) {
          const r = convertExpr(init.expression);
          if (r !== null) { replace(init.expression, r); return; }
        }
      }
    }
    if (ts.isCallExpression(node) && fnScope && ts.isIdentifier(node.expression) && node.expression.text === "toast" && node.arguments[0]) {
      const r = convertExpr(node.arguments[0]);
      if (r !== null) replace(node.arguments[0], r);
    }
    if (ts.isNewExpression(node) && fnScope && node.expression.getText(sf) === "Error" && node.arguments?.[0]) {
      const r = convertExpr(node.arguments[0]);
      if (r !== null) replace(node.arguments[0], r);
    }
    if (ts.isPropertyAssignment(node) && TEXT_PROPS.has(node.name.getText(sf))) {
      if (fnScope) {
        const r = convertExpr(node.initializer);
        if (r !== null) { replace(node.initializer, r); return; }
      } else if (ts.isStringLiteral(node.initializer) && worthy(node.initializer.text)) skippedModule++;
    }
    ts.forEachChild(node, visit);
  }
  visit(sf);

  for (const m of moduleLeft) report.push(`MANUAL ${m}`);
  if (skippedModule) report.push(`${relative(SRC, file)}: ${skippedModule} module-level literal(s) left (convert to a function by hand)`);
  if (!edits.length) continue;
  // drop edits nested in others
  edits.sort((a, b) => a.start - b.start || b.end - a.end);
  const clean = [];
  for (const e of edits) if (!clean.length || e.start >= clean[clean.length - 1].end) clean.push(e);
  let out = src;
  for (const e of [...clean].reverse()) out = out.slice(0, e.start) + e.text + out.slice(e.end);
  // import { t, tNode } from ".../shared/i18n" (merged into an existing import of the module when there is one)
  const importPath = relative(dirname(file), join(SRC, "shared/i18n")).split(sep).join("/");
  const wanted = [fn === "tr" ? "t as tr" : "t", ...(usesNode ? [fn === "tr" ? "tNode as trNode" : "tNode"] : [])];
  const existing = out.match(/import \{([^}]*)\} from "((?:\.\.?\/)+(?:[\w./-]*\/)?(?:shared\/)?i18n)";/);
  if (existing) {
    const have = existing[1].split(",").map((x) => x.trim()).filter(Boolean);
    const merged = [...have, ...wanted.filter((w) => !have.includes(w))];
    out = out.replace(existing[0], `import { ${merged.join(", ")} } from "${existing[2]}";`);
  } else {
    const imports = [...out.matchAll(/^import[^;]*;\s*$/gm)];
    const at = imports.length ? imports[imports.length - 1].index + imports[imports.length - 1][0].length : 0;
    const rel = importPath.startsWith(".") ? importPath : `./${importPath}`;
    out = out.slice(0, at) + `${at ? "\n" : ""}import { ${wanted.join(", ")} } from "${rel}";` + (at ? "" : "\n") + out.slice(at);
  }
  totalEdits += clean.length;
  if (!dry) writeFileSync(file, out);
}

// ---- write catalogs -------------------------------------------------------------------------------
const lines = [];
let n = 0;
const manifest = [];
for (const [area, c] of catalogs) {
  if (!c.added.length) continue;
  if (!dry) {
    mkdirSync(join(MSG, "it"), { recursive: true });
    const body = [...c.entries].map(([k, v]) => `  ${JSON.stringify(k)}: ${JSON.stringify(v)},`).join("\n");
    writeFileSync(join(MSG, "it", `${area}.ts`), `// Italian messages of "${area}" (source of truth). One entry per line.\nexport const ${area.replace(/[^A-Za-z0-9]/g, "_")}It = {\n${body}\n} as const;\n`);
  }
  for (const k of c.added) {
    n++;
    manifest.push({ n, area, key: k });
    lines.push(`${n}|${c.entries.get(k)}`);
  }
}
if (!dry) writeFileSync(join(process.env.I18N_SCRATCH ?? tmpdir(), "i18n-manifest.json"), JSON.stringify(manifest));
console.log(lines.join("\n"));
console.error(`\n${totalEdits} edits in ${files.length} files, ${n} new messages${dry ? " (dry run)" : ""}`);
for (const r of report) console.error("  note:", r);
