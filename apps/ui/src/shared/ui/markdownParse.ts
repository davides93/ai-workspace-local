// Safe Markdown → AST. The output is plain data that `Markdown.tsx` turns into React elements, so
// nothing is ever injected as HTML: raw tags in the source stay text, `javascript:` links lose their
// link, and images are shown as links (a remote <img> would let a prompt-injected answer leak data).
//
// Supported: headings 1–3 (deeper levels collapse to 3), paragraphs with soft line breaks, **bold**,
// *italic*, ~~strike~~, `code`, links and bare URLs (http, https, mailto), fenced code with a
// language label, bullet/ordered/task lists (nested, ordered start number), blockquotes, rules and
// GFM tables with column alignment.

export type Inline =
  | { t: "text"; v: string }
  | { t: "code"; v: string }
  | { t: "strong"; c: Inline[] }
  | { t: "em"; c: Inline[] }
  | { t: "del"; c: Inline[] }
  | { t: "link"; href: string; c: Inline[] }
  | { t: "br" };

export type Align = "left" | "center" | "right" | null;

export interface ListItem {
  blocks: Block[];
  /** Task list item: checked state. */
  checked?: boolean;
}

export type Block =
  | { t: "p"; c: Inline[] }
  | { t: "h"; level: 1 | 2 | 3; c: Inline[] }
  | { t: "code"; lang: string; v: string }
  | { t: "list"; ordered: boolean; start: number; tight: boolean; items: ListItem[] }
  | { t: "quote"; c: Block[] }
  | { t: "hr" }
  | { t: "table"; head: Inline[][]; align: Align[]; rows: Inline[][][] };

const MAX_DEPTH = 8;
const ESCAPABLE = "\\`*_{}[]()#+-.!~|<>";
const ENTITIES: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'", "&nbsp;": " " };

const decodeEntities = (s: string) => s.replace(/&(?:amp|lt|gt|quot|#39|nbsp);/g, (m) => ENTITIES[m]);
const isAlnum = (c: string | undefined) => !!c && /[\p{L}\p{N}]/u.test(c);
const isSpace = (c: string | undefined) => c === undefined || /\s/.test(c);

/** Only web and mail links become links. */
export function safeHref(url: string): string | null {
  const u = url.trim();
  return /^(?:https?:\/\/|mailto:)[^\s<>"']+$/i.test(u) ? u : null;
}

// ------------------------------------------------------------------ inline

function findBacktickClose(s: string, from: number, run: number): number {
  let j = from;
  while (j < s.length) {
    if (s[j] !== "`") {
      j++;
      continue;
    }
    let r = 1;
    while (s[j + r] === "`") r++;
    if (r === run) return j;
    j += r;
  }
  return -1;
}

/** Index of the `]` that closes the `[` at `open`, skipping escapes, code spans and nested brackets. */
function findLabelEnd(s: string, open: number): number {
  let depth = 0;
  for (let j = open; j < s.length; j++) {
    const c = s[j];
    if (c === "\\") j++;
    else if (c === "`") {
      let r = 1;
      while (s[j + r] === "`") r++;
      const close = findBacktickClose(s, j + r, r);
      j = close >= 0 ? close + r - 1 : j + r - 1;
    } else if (c === "[") depth++;
    else if (c === "]" && --depth === 0) return j;
  }
  return -1;
}

function parseLink(s: string, open: number): { label: string; url: string; end: number } | null {
  const close = findLabelEnd(s, open);
  if (close < 0 || s[close + 1] !== "(") return null;
  let j = close + 2;
  while (s[j] === " ") j++;
  let url = "";
  if (s[j] === "<") {
    const gt = s.indexOf(">", j);
    if (gt < 0) return null;
    url = s.slice(j + 1, gt);
    j = gt + 1;
  } else {
    let depth = 0;
    const start = j;
    while (j < s.length && !/\s/.test(s[j])) {
      if (s[j] === "(") depth++;
      else if (s[j] === ")") {
        if (depth === 0) break;
        depth--;
      }
      j++;
    }
    url = s.slice(start, j);
  }
  while (s[j] === " ") j++;
  const q = s[j];
  if (q === '"' || q === "'") {
    const end = s.indexOf(q, j + 1);
    if (end < 0) return null;
    j = end + 1;
    while (s[j] === " ") j++;
  }
  if (s[j] !== ")") return null;
  return { label: s.slice(open + 1, close), url, end: j + 1 };
}

const AUTOLINK = /<((?:https?:\/\/|mailto:)[^\s<>]+)>/iy;
const BARE_URL = /https?:\/\/[^\s<>]+/iy;

function trimUrl(url: string): string {
  let u = url;
  for (;;) {
    const last = u[u.length - 1];
    if (last && ".,;:!?'\"*_~".includes(last)) u = u.slice(0, -1);
    else if (last === ")" && (u.match(/\)/g)?.length ?? 0) > (u.match(/\(/g)?.length ?? 0)) u = u.slice(0, -1);
    else return u;
  }
}

/** Closing run of `marker` (exactly `n` long) for an emphasis opened before `from`; -1 when none. */
function findEmphasisClose(s: string, from: number, marker: string, n: number): number {
  let j = from;
  while (j < s.length) {
    const c = s[j];
    if (c === "\\") {
      j += 2;
      continue;
    }
    if (c === "`") {
      let r = 1;
      while (s[j + r] === "`") r++;
      const close = findBacktickClose(s, j + r, r);
      j = close >= 0 ? close + r : j + r;
      continue;
    }
    if (c !== marker) {
      j++;
      continue;
    }
    let r = 1;
    while (s[j + r] === marker) r++;
    const before = s[j - 1];
    const after = s[j + r];
    if (r === n && !isSpace(before) && (marker !== "_" || !isAlnum(after))) return j;
    j += r;
  }
  return -1;
}

function parseEmphasis(s: string, i: number, depth: number): { nodes: Inline[]; end: number } | null {
  const marker = s[i];
  let run = 1;
  while (s[i + run] === marker) run++;
  if (marker === "~" ? run !== 2 : run > 3) return null;
  if (isSpace(s[i + run])) return null;
  if (marker === "_" && isAlnum(s[i - 1])) return null;
  const close = findEmphasisClose(s, i + run, marker, run);
  if (close < 0 || close === i + run) return null;
  const inner = parseInline(s.slice(i + run, close), depth + 1);
  const end = close + run;
  if (marker === "~") return { nodes: [{ t: "del", c: inner }], end };
  if (run === 1) return { nodes: [{ t: "em", c: inner }], end };
  if (run === 2) return { nodes: [{ t: "strong", c: inner }], end };
  return { nodes: [{ t: "strong", c: [{ t: "em", c: inner }] }], end };
}

/** Inline formatting is not looked for in a single run of text longer than this (unmatched markers would make the scan quadratic). */
const MAX_INLINE_CHARS = 8000;

function plainInline(src: string): Inline[] {
  const out: Inline[] = [];
  src.split("\n").forEach((line, i) => {
    if (i > 0) out.push({ t: "br" });
    if (line) out.push({ t: "text", v: decodeEntities(line) });
  });
  return out;
}

export function parseInline(src: string, depth = 0): Inline[] {
  if (depth > 6 || src.length > MAX_INLINE_CHARS) return plainInline(src);
  const out: Inline[] = [];
  let buf = "";
  const flush = () => {
    if (buf) {
      out.push({ t: "text", v: decodeEntities(buf) });
      buf = "";
    }
  };
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === "\\" && i + 1 < src.length) {
      const d = src[i + 1];
      if (d === "\n") {
        flush();
        out.push({ t: "br" });
        i += 2;
        continue;
      }
      if (ESCAPABLE.includes(d)) {
        buf += d;
        i += 2;
        continue;
      }
    }
    if (c === "\n") {
      buf = buf.replace(/ +$/, "");
      flush();
      out.push({ t: "br" });
      i++;
      continue;
    }
    if (c === "`") {
      let run = 1;
      while (src[i + run] === "`") run++;
      const close = findBacktickClose(src, i + run, run);
      if (close >= 0) {
        flush();
        let v = src.slice(i + run, close).replace(/\n/g, " ");
        if (v.length > 2 && v.startsWith(" ") && v.endsWith(" ") && v.trim() !== "") v = v.slice(1, -1);
        out.push({ t: "code", v });
        i = close + run;
        continue;
      }
      buf += "`".repeat(run);
      i += run;
      continue;
    }
    if (c === "[" || (c === "!" && src[i + 1] === "[")) {
      const image = c === "!";
      const link = parseLink(src, image ? i + 1 : i);
      if (link) {
        flush();
        const href = safeHref(link.url);
        const label: Inline[] = image ? [{ t: "text", v: link.label.trim() || link.url }] : parseInline(link.label, depth + 1);
        if (href) out.push({ t: "link", href, c: label.length > 0 ? label : [{ t: "text", v: href }] });
        else out.push(...label);
        i = link.end;
        continue;
      }
      buf += c;
      i++;
      continue;
    }
    if (c === "<") {
      AUTOLINK.lastIndex = i;
      const m = AUTOLINK.exec(src);
      if (m) {
        flush();
        out.push({ t: "link", href: m[1], c: [{ t: "text", v: m[1] }] });
        i += m[0].length;
        continue;
      }
    }
    if ((c === "h" || c === "H") && !isAlnum(src[i - 1])) {
      BARE_URL.lastIndex = i;
      const m = BARE_URL.exec(src);
      if (m) {
        const url = trimUrl(m[0]);
        const href = safeHref(url);
        if (href && url.length > 8) {
          flush();
          out.push({ t: "link", href, c: [{ t: "text", v: url }] });
          i += url.length;
          continue;
        }
      }
    }
    if (c === "*" || c === "_" || c === "~") {
      const em = parseEmphasis(src, i, depth);
      if (em) {
        flush();
        out.push(...em.nodes);
        i = em.end;
        continue;
      }
      let run = 1;
      while (src[i + run] === c) run++;
      buf += c.repeat(run);
      i += run;
      continue;
    }
    buf += c;
    i++;
  }
  flush();
  return out;
}

// ------------------------------------------------------------------ blocks

const FENCE_OPEN = /^( {0,3})(`{3,}|~{3,})[ \t]*([^\s`]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$/;
const HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}>[ ]?(.*)$/;
const BULLET = /^( *)([-*+])( +)(.*)$/;
const ORDERED = /^( *)(\d{1,9})([.)])( +)(.*)$/;
const TABLE_SEP = /^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;

const isBlank = (l: string) => l.trim() === "";
const indentOf = (l: string) => l.length - l.trimStart().length;

interface Marker {
  indent: number;
  ordered: boolean;
  number: number;
  /** Column where the item's content starts (what continuation lines are measured against). */
  offset: number;
  text: string;
}

function marker(line: string): Marker | null {
  const b = BULLET.exec(line);
  if (b) return { indent: b[1].length, ordered: false, number: 1, offset: b[1].length + 1 + Math.min(b[3].length, 4), text: b[4] };
  const o = ORDERED.exec(line);
  if (o) return { indent: o[1].length, ordered: true, number: Number(o[2]), offset: o[1].length + o[2].length + 1 + Math.min(o[4].length, 4), text: o[5] };
  return null;
}

function splitRow(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let code = 0;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === "\\" && line[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (c === "`") {
      let r = 1;
      while (line[i + r] === "`") r++;
      code = code === r ? 0 : code === 0 ? r : code;
      cur += "`".repeat(r);
      i += r - 1;
    } else if (c === "|" && code === 0) {
      cells.push(cur);
      cur = "";
    } else cur += c;
  }
  cells.push(cur);
  const t = line.trim();
  if (t.startsWith("|")) cells.shift();
  if (t.endsWith("|") && !t.endsWith("\\|") && cells.length > 0) cells.pop();
  return cells.map((c) => c.trim());
}

function isTableStart(lines: string[], i: number): boolean {
  const head = lines[i];
  const sep = lines[i + 1];
  return sep !== undefined && head.includes("|") && sep.includes("|") && TABLE_SEP.test(sep) && splitRow(head).length === splitRow(sep).length;
}

/** Does this line begin a block that ends the paragraph above it? */
function startsBlock(lines: string[], i: number): boolean {
  const l = lines[i];
  return FENCE_OPEN.test(l) || HEADING.test(l) || HR.test(l) || QUOTE.test(l) || marker(l) !== null || isTableStart(lines, i);
}

function parseList(lines: string[], start: number, depth: number): { block: Block; next: number } {
  const first = marker(lines[start])!;
  const base = first.indent;
  const items: ListItem[] = [];
  let loose = false;
  let i = start;
  for (;;) {
    const m = marker(lines[i])!;
    const body: string[] = [m.text];
    let j = i + 1;
    while (j < lines.length) {
      const l = lines[j];
      if (isBlank(l)) {
        let k = j + 1;
        while (k < lines.length && isBlank(lines[k])) k++;
        if (k >= lines.length) {
          j = k;
          break;
        }
        const nm = marker(lines[k]);
        if (indentOf(lines[k]) > base) {
          for (let b = j; b < k; b++) body.push("");
          loose = true;
          j = k;
          continue;
        }
        if (nm && nm.ordered === first.ordered && nm.indent <= base) loose = true;
        break;
      }
      const ind = indentOf(l);
      if (ind > base) {
        body.push(l.slice(Math.min(ind, m.offset)));
        j++;
        continue;
      }
      if (marker(l) || startsBlock(lines, j)) break;
      if (!isBlank(body[body.length - 1])) {
        body.push(l.trimStart());
        j++;
        continue;
      }
      break;
    }
    let checked: boolean | undefined;
    const task = /^\[( |x|X)\][ \t]+/.exec(body[0]);
    if (task) {
      checked = task[1] !== " ";
      body[0] = body[0].slice(task[0].length);
    }
    items.push({ blocks: parseBlockLines(body, depth + 1), ...(checked === undefined ? {} : { checked }) });
    // Next sibling (a blank line may separate items), else the list ends here.
    let k = j;
    while (k < lines.length && isBlank(lines[k])) k++;
    const nm = k < lines.length ? marker(lines[k]) : null;
    if (nm && nm.ordered === first.ordered && nm.indent <= base && !HR.test(lines[k])) {
      i = k;
      continue;
    }
    i = j;
    break;
  }
  return { block: { t: "list", ordered: first.ordered, start: first.number, tight: !loose, items }, next: i };
}

function parseBlockLines(lines: string[], depth: number): Block[] {
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      i++;
      continue;
    }
    if (depth < MAX_DEPTH) {
      const fence = FENCE_OPEN.exec(line);
      if (fence) {
        const [, indent, ticks, info] = fence;
        const buf: string[] = [];
        i++;
        const closer = new RegExp(`^ {0,3}${ticks[0] === "`" ? "`" : "~"}{${ticks.length},}[ \\t]*$`);
        while (i < lines.length && !closer.test(lines[i])) {
          buf.push(lines[i].replace(new RegExp(`^ {0,${indent.length}}`), ""));
          i++;
        }
        i++; // closing fence (an unclosed fence runs to the end: streaming)
        out.push({ t: "code", lang: info.replace(/[^\w+#.-]/g, "").slice(0, 24), v: buf.join("\n") });
        continue;
      }
      const h = HEADING.exec(line);
      if (h) {
        out.push({ t: "h", level: Math.min(h[1].length, 3) as 1 | 2 | 3, c: parseInline(h[2]) });
        i++;
        continue;
      }
      if (HR.test(line)) {
        out.push({ t: "hr" });
        i++;
        continue;
      }
      if (QUOTE.test(line)) {
        const inner: string[] = [];
        while (i < lines.length && QUOTE.test(lines[i])) {
          inner.push(QUOTE.exec(lines[i])![1]);
          i++;
        }
        out.push({ t: "quote", c: parseBlockLines(inner, depth + 1) });
        continue;
      }
      if (marker(line)) {
        const { block, next } = parseList(lines, i, depth);
        out.push(block);
        i = next;
        continue;
      }
      if (isTableStart(lines, i)) {
        const head = splitRow(line);
        const align = splitRow(lines[i + 1]).map<Align>((c) => (c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : c.startsWith(":") ? "left" : null));
        const rows: Inline[][][] = [];
        i += 2;
        while (i < lines.length && !isBlank(lines[i]) && lines[i].includes("|")) {
          const cells = splitRow(lines[i]);
          rows.push(head.map((_, c) => parseInline(cells[c] ?? "")));
          i++;
        }
        out.push({ t: "table", head: head.map((c) => parseInline(c)), align, rows });
        continue;
      }
    }
    const para: string[] = [line.trimStart()];
    i++;
    while (i < lines.length && !isBlank(lines[i]) && (depth >= MAX_DEPTH || !startsBlock(lines, i))) para.push(lines[i++].trimStart());
    out.push({ t: "p", c: parseInline(para.join("\n").trimEnd()) });
  }
  return out;
}

/** Parses a whole document (no caching). */
export function parseBlocks(text: string): Block[] {
  return parseBlockLines(text.replace(/\r\n?/g, "\n").replace(/^\t+/gm, (t) => "    ".repeat(t.length)).split("\n"), 0);
}

// ------------------------------------------------------------------ caching for streaming

/**
 * Start of the last top-level block when it begins after a blank line (outside code fences, not
 * indented, not a list item): everything before it cannot change when more text is appended, so it
 * is parsed once and reused while the tail streams.
 */
export function stableCut(text: string): number {
  const lines = text.split("\n");
  let pos = 0;
  let cut = 0;
  let fence: string | null = null;
  let prevBlank = false;
  for (const line of lines) {
    const f = FENCE_OPEN.exec(line);
    if (fence) {
      if (new RegExp(`^ {0,3}${fence[0] === "`" ? "`" : "~"}{${fence.length},}[ \\t]*$`).test(line)) fence = null;
    } else if (f) {
      if (prevBlank && pos > 0 && indentOf(line) === 0) cut = pos;
      fence = f[2];
    } else if (prevBlank && pos > 0 && !isBlank(line) && indentOf(line) === 0 && marker(line) === null) {
      cut = pos;
    }
    prevBlank = isBlank(line);
    pos += line.length + 1;
  }
  return cut;
}

const cache = new Map<string, Block[]>();
const CACHE_MAX = 300;
const CACHE_MAX_CHARS = 100_000;

/** Cached, incremental parse: unchanged text is a lookup, a growing message re-parses only its tail. */
export function parseMarkdown(text: string): Block[] {
  const hit = cache.get(text);
  if (hit) {
    cache.delete(text);
    cache.set(text, hit);
    return hit;
  }
  const cut = text.length > 200 ? stableCut(text) : 0;
  const blocks = cut > 0 ? [...parseMarkdown(text.slice(0, cut)), ...parseBlocks(text.slice(cut))] : parseBlocks(text);
  if (text.length <= CACHE_MAX_CHARS) {
    cache.set(text, blocks);
    if (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value as string);
  }
  return blocks;
}

/** Test hook. */
export const markdownCacheSize = () => cache.size;
