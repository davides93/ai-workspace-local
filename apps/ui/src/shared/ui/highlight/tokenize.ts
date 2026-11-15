// A small syntax highlighter for the code blocks of the chat: a tokenizer, not a parser. It turns code into runs of text with a
// kind (`Token[]`); the view renders each run as a <span>, so there is no HTML string and nothing to escape. Properties the
// tests rely on:
//  - the runs, joined, are exactly the input (copying, selecting and screen readers see the same text);
//  - it never throws and never loops: every step consumes at least one character, and an unfinished string or comment (a block
//    that is still streaming) runs to its end;
//  - no rule backtracks over the whole input: a pathological block costs a few milliseconds, and a block over
//    `MAX_HIGHLIGHT_CHARS` is not highlighted at all.
// This file holds every grammar, so it is its own chunk (`useHighlight.ts` imports it on demand).
import type { GrammarId } from "./languages";

/** `com` comment · `str` string · `num` number · `kw` keyword · `lit` true/false/null · `fn` function or macro · `ty` type, class, tag · `attr` property, key, variable, flag · `ins`/`del` added/removed line of a diff. */
export type Kind = "com" | "str" | "num" | "kw" | "lit" | "fn" | "ty" | "attr" | "ins" | "del";
export interface Token {
  /** `null`: plain text. */
  k: Kind | null;
  v: string;
}

/** Longer blocks are shown plain: the cost of highlighting grows with the length, and nobody reads 30 000 characters as colours. */
export const MAX_HIGHLIGHT_CHARS = 30_000;

type Scanner = (code: string, i: number) => Token[] | null;
type Rule = [Kind, RegExp] | [null, Scanner];

interface Grammar {
  /** Tried in order at each position; the first that matches wins. A regex rule is sticky. */
  rules: Rule[];
  /** Pattern of a word (identifier, keyword); the sets below classify it. */
  word?: RegExp;
  kw?: ReadonlySet<string>;
  lit?: ReadonlySet<string>;
  ty?: ReadonlySet<string>;
  fn?: ReadonlySet<string>;
  /** Keywords are matched ignoring case (SQL). */
  ci?: boolean;
  /** A word followed by `(` is a function (`"tight"`: only when the parenthesis follows at once, as in `count(*)`). */
  calls?: boolean | "tight";
  /** A word that starts with a capital and has a lowercase letter is a type. */
  caps?: boolean;
}

const set = (words: string): ReadonlySet<string> => new Set(words.split(/\s+/).filter(Boolean));
const sticky = (re: RegExp): RegExp => new RegExp(re.source, re.flags.replace("y", "") + "y");
const r = (kind: Kind, re: RegExp): Rule => [kind, sticky(re)];

// Pieces shared by the grammars. Every quantified piece excludes what ends it, so none can backtrack across the input.
const LINE_COMMENT = (marker: string) => new RegExp(`${marker}[^\\n]*`);
const BLOCK_COMMENT = /\/\*[\s\S]*?(?:\*\/|$)/;
const DQ = /"(?:\\[\s\S]|[^"\\\n])*"?/;
const SQ = /'(?:\\[\s\S]|[^'\\\n])*'?/;
const DQ_MULTI = /"(?:\\[\s\S]|[^"\\])*"?/;
const NUM = /(?<![\w$.])(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)n?(?![\w$])/;
const LINE_START = "(?<![^\\n])";

const JS_KW = "break case catch class const continue debugger default delete do else export extends finally for from function if import in instanceof let new of return super switch this throw try typeof var void while with yield async await static get set as";
const TS_KW = `${JS_KW} interface type enum implements namespace declare abstract readonly is keyof infer satisfies public private protected override module`;

function js(ts: boolean): Grammar {
  return {
    rules: [
      r("com", LINE_COMMENT("//")), r("com", BLOCK_COMMENT),
      r("str", DQ), r("str", SQ), r("str", /`(?:\\[\s\S]|[^`\\])*`?/),
      r("num", NUM), r("attr", /@[A-Za-z_]\w*/), r("fn", /(?<=\bfunction\*?[ \t]+)[A-Za-z_$][\w$]*/),
    ],
    kw: set(ts ? TS_KW : JS_KW),
    lit: set("true false null undefined NaN Infinity"),
    ty: set(ts ? "string number boolean any unknown never object bigint symbol Array Promise Record Partial Map Set Date Error" : "Array Promise Map Set Date Error Object String Number Boolean Symbol"),
    calls: true,
    caps: true,
  };
}

const JSON_GRAMMAR: Grammar = {
  rules: [
    r("com", LINE_COMMENT("//")), r("com", BLOCK_COMMENT),
    r("attr", /"(?:\\[\s\S]|[^"\\\n])*"(?=[ \t]*:)/), r("str", DQ),
    r("num", /(?<![\w.])-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?(?![\w.])/),
  ],
  lit: set("true false null"),
};

const PY_PREFIX = "(?<![\\w])[rRbBfFuU]{0,2}";
const PYTHON: Grammar = {
  rules: [
    r("com", LINE_COMMENT("#")),
    r("str", new RegExp(`${PY_PREFIX}(?:"""[\\s\\S]*?(?:"""|$)|'''[\\s\\S]*?(?:'''|$))`)),
    r("str", new RegExp(`${PY_PREFIX}(?:"(?:\\\\.|[^"\\\\\\n])*"?|'(?:\\\\.|[^'\\\\\\n])*'?)`)),
    r("num", NUM), r("attr", new RegExp(`${LINE_START}[ \\t]*@[\\w.]+`)),
    r("fn", /(?<=\bdef[ \t]+)[A-Za-z_]\w*/), r("ty", /(?<=\bclass[ \t]+)[A-Za-z_]\w*/),
  ],
  kw: set("and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case"),
  lit: set("True False None"),
  ty: set("int str float bool list dict set tuple bytes object type"),
  fn: set("print len range open isinstance super self cls"),
  calls: true,
  caps: true,
};

const SHELL: Grammar = {
  rules: [
    r("com", /(?<![^\s;|&(])#[^\n]*/),
    r("str", DQ_MULTI), r("str", /'[^']*'?/),
    r("attr", /\$(?:\{[^}\n]*\}?|\([^)\n]*\)?|[A-Za-z_]\w*|[0-9@#?$!*-])/),
    r("lit", /(?<![\w-])--?[A-Za-z][\w-]*/),
    r("num", NUM),
  ],
  word: /[A-Za-z_][\w.-]*/y,
  kw: set("if then else elif fi for while until do done case esac in function select time"),
  fn: set("cd echo export source alias unset exit return set eval exec test read printf pwd ls cat grep sed awk curl wget git npm npx cargo sudo mkdir rm cp mv chmod chown kill docker"),
};

const RUST: Grammar = {
  rules: [
    r("com", LINE_COMMENT("//")), r("com", BLOCK_COMMENT),
    r("attr", /#!?\[[^\]\n]*\]?/),
    r("str", /b?r#+"[\s\S]*?(?:"#+|$)/), r("str", /b?r"[^"]*"?/), r("str", new RegExp(`b?${DQ_MULTI.source}`)),
    r("str", /b?'(?:\\.|[^\\'\n])'/), r("attr", /'[A-Za-z_]\w*/),
    r("num", /(?<![\w.])(?:0x[\da-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)(?:[iuf](?:8|16|32|64|128|size))?(?![\w])/),
    r("fn", /[A-Za-z_]\w*!(?=[([{])/), r("fn", /(?<=\bfn[ \t]+)[A-Za-z_]\w*/),
  ],
  kw: set("as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self static struct super trait type union unsafe use where while"),
  lit: set("true false"),
  ty: set("i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box Self Some None Ok Err"),
  calls: true,
  caps: true,
};

function clike(kw: string, ty: string, lit: string, extra: Rule[] = []): Grammar {
  return {
    rules: [r("com", LINE_COMMENT("//")), r("com", BLOCK_COMMENT), ...extra, r("str", DQ), r("str", SQ), r("num", NUM)],
    kw: set(kw), ty: set(ty), lit: set(lit), calls: true, caps: true,
  };
}
const C = clike(
  "auto break case const continue default do else enum extern for goto if inline register restrict return sizeof static struct switch typedef union volatile while class namespace template typename public private protected virtual override new delete this using try catch throw operator constexpr noexcept",
  "int char short long float double void unsigned signed bool size_t ssize_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t string vector",
  "true false NULL nullptr",
  [r("kw", /(?<![^\n])[ \t]*#[ \t]*(?:include|define|undef|ifdef|ifndef|if|elif|else|endif|pragma|error)\b/)],
);
const JAVA = clike(
  "abstract assert break case catch class const continue default do else enum extends final finally for goto if implements import instanceof interface native new package private protected public return static strictfp super switch synchronized this throw throws transient try volatile while var record sealed permits yield",
  "int long short byte char float double boolean void String Object List Map Set",
  "true false null",
  [r("attr", /@[A-Za-z_]\w*/)],
);
const GO = clike(
  "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var",
  "int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr float32 float64 complex64 complex128 string bool byte rune error any",
  "true false nil iota",
  [r("str", /`[^`]*`?/), r("fn", /(?<=\bfunc[ \t]+)[A-Za-z_]\w*/)],
);
const CSHARP = clike(
  "abstract as base break case catch checked class const continue default delegate do else enum event explicit extern finally fixed for foreach goto if implicit in interface internal is lock namespace new operator out override params private protected public readonly ref return sealed sizeof stackalloc static struct switch this throw try typeof unchecked unsafe using virtual volatile while async await var record get set init",
  "int long short byte char float double decimal bool void string object",
  "true false null",
  [r("attr", /(?<![\w\]])\[[A-Z]\w*(?:\([^)\n]*\))?\]/)],
);

const CSS: Grammar = {
  rules: [
    r("com", BLOCK_COMMENT), r("str", DQ), r("str", SQ),
    r("kw", /@[\w-]+/), r("kw", /!important\b/),
    r("num", /#[\da-fA-F]{3,8}\b/),
    r("num", /(?<![\w#-])-?(?:\d+\.?\d*|\.\d+)(?:%|[a-zA-Z]+)?/),
    r("attr", /--[\w-]+/), r("attr", /[\w-]+(?=[ \t]*:[ \t\n])/),
    r("ty", /[.#][A-Za-z_-][\w-]*/), r("lit", /::?[A-Za-z-]+/),
    r("fn", /[A-Za-z-]+(?=\()/),
  ],
};

/** A whole tag, split into its name, attribute names and quoted values. */
const TAG = /<\/?[A-Za-z][\w:.-]*(?:\s+[^\s=<>"'/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'<>]+))?)*\s*\/?>?/y;
const TAG_PARTS = /(<\/?[A-Za-z][\w:.-]*)|("[^"]*"?|'[^']*'?)|([^\s=<>"'/]+)(?=\s*=)/g;
const htmlTag: Scanner = (code, i) => {
  TAG.lastIndex = i;
  const m = TAG.exec(code);
  if (!m) return null;
  const out: Token[] = [];
  let at = 0;
  for (const p of m[0].matchAll(TAG_PARTS)) {
    const idx = p.index ?? 0;
    if (idx > at) out.push({ k: null, v: m[0].slice(at, idx) });
    out.push({ k: p[1] ? "ty" : p[2] ? "str" : "attr", v: p[0] });
    at = idx + p[0].length;
  }
  if (at < m[0].length) out.push({ k: null, v: m[0].slice(at) });
  return out;
};
const HTML: Grammar = {
  rules: [r("com", /<!--[\s\S]*?(?:-->|$)/), r("com", /<[!?][^>]*>?/), [null, htmlTag], r("lit", /&#?\w+;/)],
  word: /[^\s<&]+/y,
};

const SQL: Grammar = {
  rules: [r("com", LINE_COMMENT("--")), r("com", BLOCK_COMMENT), r("str", /'(?:''|[^'])*'?/), r("attr", /"[^"\n]*"?/), r("attr", /`[^`\n]*`?/), r("num", NUM)],
  kw: set("select insert update delete from where join left right inner outer full cross on as and or not in is like between group by order having limit offset union all distinct create table alter drop index view primary key foreign references default values set into returning with case when then else end exists asc desc add column constraint unique check begin commit rollback"),
  lit: set("null true false"),
  ty: set("int integer bigint smallint text varchar char boolean bool date time timestamp numeric decimal float real serial uuid json jsonb blob"),
  ci: true,
  calls: "tight",
};

const YAML: Grammar = {
  rules: [
    r("com", /(?<![^\s])#[^\n]*/), r("str", DQ), r("str", SQ),
    r("attr", /[\w.@/-]+(?=[ \t]*:(?:[ \t]|\n|$))/),
    r("ty", /[&*][\w-]+/), r("num", NUM),
  ],
  lit: set("true false null yes no on off ~ True False Null"),
};

const TOML: Grammar = {
  rules: [
    r("com", /#[^\n]*/),
    r("str", /"""[\s\S]*?(?:"""|$)/), r("str", /'''[\s\S]*?(?:'''|$)/), r("str", DQ), r("str", /'[^'\n]*'?/),
    r("ty", new RegExp(`${LINE_START}\\[\\[?[^\\]\\n]*\\]?\\]?`)),
    r("attr", /[\w.-]+(?=[ \t]*=)/),
    r("num", /\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?/), r("num", NUM),
  ],
  lit: set("true false inf nan"),
};

const DIFF: Grammar = {
  rules: [
    r("com", new RegExp(`${LINE_START}(?:diff |index |\\+\\+\\+ |--- |new file|deleted file|similarity|rename )[^\\n]*`)),
    r("kw", new RegExp(`${LINE_START}@@[^\\n]*`)),
    r("ins", new RegExp(`${LINE_START}\\+[^\\n]*`)),
    r("del", new RegExp(`${LINE_START}-[^\\n]*`)),
  ],
  // A line of context is plain, whatever words it holds.
  word: /[^\n]+/y,
};

const GRAMMAR_TABLE: Record<GrammarId, Grammar> = {
  js: js(false), ts: js(true), json: JSON_GRAMMAR, python: PYTHON, shell: SHELL, rust: RUST, c: C, java: JAVA, go: GO, csharp: CSHARP,
  css: CSS, html: HTML, sql: SQL, yaml: YAML, toml: TOML, diff: DIFF,
};

const WORD = /[\p{L}_$][\p{L}\p{N}_$]*/uy;
/** Blanks inside a line, or a line break: the unit the loop steps over when no rule starts there. */
const BLANKS = /[^\S\n]+|\n/y;
const CAPITAL = /^[A-Z][A-Za-z0-9_]*$/;

function classify(g: Grammar, word: string, code: string, end: number): Kind | null {
  const w = g.ci ? word.toLowerCase() : word;
  if (g.kw?.has(w)) return "kw";
  if (g.lit?.has(w)) return "lit";
  if (g.ty?.has(w)) return "ty";
  if (g.fn?.has(w)) return "fn";
  if (g.calls) {
    let j = end;
    if (g.calls !== "tight") while (code[j] === " " || code[j] === "\t") j++;
    if (code[j] === "(") return "fn";
  }
  if (g.caps && CAPITAL.test(word) && /[a-z]/.test(word)) return "ty";
  return null;
}

/** Splits `code` into runs of the given grammar. Adjacent runs of the same kind are merged. */
export function tokenize(code: string, grammar: GrammarId): Token[] {
  const g = GRAMMAR_TABLE[grammar];
  if (!g || code.length === 0 || code.length > MAX_HIGHLIGHT_CHARS) return [{ k: null, v: code }];
  const out: Token[] = [];
  const push = (k: Kind | null, v: string) => {
    if (!v) return;
    const last = out[out.length - 1];
    if (last && last.k === k) last.v += v;
    else out.push({ k, v });
  };
  const word = g.word ? sticky(g.word) : WORD;
  let i = 0;
  scan: while (i < code.length) {
    // Blanks in the middle of a line are never the start of anything: step over them without trying the rules. At the start of a
    // line they are not skipped here, so the rules that anchor to a line start (`#include`, a decorator, a diff line) still see it.
    if (i > 0 && code[i - 1] !== "\n" && (code[i] === " " || code[i] === "\t")) {
      BLANKS.lastIndex = i;
      const sp = BLANKS.exec(code);
      if (sp) {
        push(null, sp[0]);
        i += sp[0].length;
        continue;
      }
    }
    for (const [kind, rule] of g.rules) {
      if (typeof rule === "function") {
        const toks = rule(code, i);
        if (toks && toks.length > 0) {
          let n = 0;
          for (const t of toks) {
            push(t.k, t.v);
            n += t.v.length;
          }
          if (n > 0) {
            i += n;
            continue scan;
          }
        }
        continue;
      }
      rule.lastIndex = i;
      const m = rule.exec(code);
      if (kind && m && m[0].length > 0) {
        push(kind, m[0]);
        i += m[0].length;
        continue scan;
      }
    }
    word.lastIndex = i;
    const w = word.exec(code);
    if (w && w[0].length > 0) {
      push(classify(g, w[0], code, i + w[0].length), w[0]);
      i += w[0].length;
      continue;
    }
    BLANKS.lastIndex = i;
    const sp = BLANKS.exec(code);
    const len = sp ? sp[0].length : 1;
    push(null, code.slice(i, i + len));
    i += len;
  }
  return out;
}
