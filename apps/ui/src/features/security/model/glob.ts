import { t as tr } from "../../../shared/i18n";
// Port of the glob dialect of the Rust crate `globset` (0.4.21) with the options the backend uses:
// `Glob::new(pattern)` — `*` and `?` also match `/`, `\` escapes, `{a,b}` alternates, `[a-z]` classes,
// `**` as a whole path component. See crates/aiw-agent/src/policy.rs and hooks.rs.
//
// The backend refuses to save a rule or a hook whose glob the crate rejects (`validated`, 400), so the
// editors treat `globProblem` as an error. For what was stored before that check the policy engine still
// falls back to comparing the pattern with the value as plain text; `globMatches` does the same.

type Token =
  | { t: "lit"; c: string }
  | { t: "any" }
  | { t: "star" }
  | { t: "rprefix" }
  | { t: "rsuffix" }
  | { t: "rzero" }
  | { t: "class"; negated: boolean; ranges: [string, string][] }
  | { t: "alt"; branches: Token[][] };

export class GlobSyntaxError extends Error {}

const isSep = (c: string | undefined): boolean => c === "/";

class Parser {
  private chars: string[];
  private pos = 0;
  private prev: string | undefined;
  private cur: string | undefined;
  private branches: Token[][] = [[]];
  private altStack: number[] = [];

  constructor(glob: string) {
    this.chars = [...glob];
  }

  private bump(): string | undefined {
    this.prev = this.cur;
    this.cur = this.chars[this.pos++];
    return this.cur;
  }

  private peek(): string | undefined {
    return this.chars[this.pos];
  }

  private last(): Token[] {
    return this.branches[this.branches.length - 1];
  }

  private push(tok: Token): void {
    this.last().push(tok);
  }

  parse(): Token[] {
    for (let c = this.bump(); c !== undefined; c = this.bump()) {
      switch (c) {
        case "?": this.push({ t: "any" }); break;
        case "*": this.star(); break;
        case "[": this.cls(); break;
        case "{": this.altStack.push(this.branches.length); this.branches.push([]); break;
        case "}": this.closeAlt(); break;
        case ",":
          if (this.altStack.length === 0) this.push({ t: "lit", c: "," });
          else this.branches.push([]);
          break;
        case "\\": {
          const n = this.bump();
          if (n === undefined) throw new GlobSyntaxError(tr("security.il_pattern_finisce_con_una"));
          this.push({ t: "lit", c: n });
          break;
        }
        default: this.push({ t: "lit", c });
      }
    }
    if (this.branches.length > 1) throw new GlobSyntaxError(tr("security.unclosed_brace"));
    return this.branches[0];
  }

  private closeAlt(): void {
    const start = this.altStack.pop();
    if (start === undefined) throw new GlobSyntaxError(tr("security.unopened_brace"));
    const branches = this.branches.splice(start);
    this.push({ t: "alt", branches });
  }

  private star(): void {
    const prev = this.prev;
    if (this.peek() !== "*") {
      this.push({ t: "star" });
      return;
    }
    this.bump();
    if (this.last().length === 0) {
      const next = this.peek();
      if (next !== undefined && !isSep(next)) {
        this.push({ t: "star" });
        this.push({ t: "star" });
      } else {
        this.push({ t: "rprefix" });
        this.bump();
      }
      return;
    }
    if (!isSep(prev)) {
      if (this.branches.length <= 1 || (prev !== "," && prev !== "{")) {
        this.push({ t: "star" });
        this.push({ t: "star" });
        return;
      }
    }
    let isSuffix: boolean;
    const next = this.peek();
    if (next === undefined) {
      this.bump();
      isSuffix = true;
    } else if ((next === "," || next === "}") && this.branches.length >= 2) {
      isSuffix = true;
    } else if (isSep(next)) {
      this.bump();
      isSuffix = false;
    } else {
      this.push({ t: "star" });
      this.push({ t: "star" });
      return;
    }
    const popped = this.last().pop();
    if (popped?.t === "rprefix" || popped?.t === "rsuffix") this.push(popped);
    else this.push({ t: isSuffix ? "rsuffix" : "rzero" });
  }

  private cls(): void {
    const ranges: [string, string][] = [];
    let negated = false;
    if (this.peek() === "!" || this.peek() === "^") {
      this.bump();
      negated = true;
    }
    let first = true;
    let inRange = false;
    const extend = (r: [string, string], to: string) => {
      r[1] = to;
      if (to < r[0]) throw new GlobSyntaxError(tr("security.intervallo_non_valido", { from: r[0], to }));
    };
    for (;;) {
      const c = this.bump();
      if (c === undefined) throw new GlobSyntaxError(tr("security.classe_non_chiusa"));
      if (c === "]") {
        if (first) ranges.push(["]", "]"]);
        else break;
      } else if (c === "-") {
        if (first) ranges.push(["-", "-"]);
        else if (inRange) {
          extend(ranges[ranges.length - 1], "-");
          inRange = false;
        } else inRange = true;
      } else {
        if (inRange) extend(ranges[ranges.length - 1], c);
        else ranges.push([c, c]);
        inRange = false;
      }
      first = false;
    }
    if (inRange) ranges.push(["-", "-"]);
    this.push({ t: "class", negated, ranges });
  }
}

const REGEX_SPECIAL = /[\\^$.*+?()[\]{}|/-]/g;
const esc = (c: string): string => c.replace(REGEX_SPECIAL, "\\$&");

function toRegexSource(tokens: Token[]): string {
  let re = "";
  for (const tok of tokens) {
    switch (tok.t) {
      case "lit": re += esc(tok.c); break;
      case "any": re += "."; break;
      case "star": re += ".*"; break;
      case "rprefix": re += "(?:/?|.*/)"; break;
      case "rsuffix": re += "/.*"; break;
      case "rzero": re += "(?:/|/.*/)"; break;
      case "class":
        re += `[${tok.negated ? "^" : ""}${tok.ranges.map(([a, b]) => (a === b ? esc(a) : `${esc(a)}-${esc(b)}`)).join("")}]`;
        break;
      case "alt": {
        const parts = tok.branches.map(toRegexSource).filter((p) => p !== "");
        if (parts.length > 0) re += `(?:${parts.join("|")})`;
        break;
      }
    }
  }
  return re;
}

const cache = new Map<string, RegExp | GlobSyntaxError>();

/** Compiles a glob; a pattern `globset` would reject returns the reason instead. */
export function compileGlob(pattern: string): RegExp | GlobSyntaxError {
  const hit = cache.get(pattern);
  if (hit) return hit;
  let out: RegExp | GlobSyntaxError;
  try {
    const tokens = new Parser(pattern).parse();
    const source = tokens.length === 1 && tokens[0].t === "rprefix" ? ".*" : toRegexSource(tokens);
    out = new RegExp(`^${source}$`, "s");
  } catch (e) {
    out = e instanceof GlobSyntaxError ? e : new GlobSyntaxError(tr("security.pattern_non_valido"));
  }
  if (cache.size > 500) cache.clear();
  cache.set(pattern, out);
  return out;
}

/** Why the backend cannot read `pattern` as a glob (it then compares it as plain text), or null. */
export function globProblem(pattern: string): string | null {
  const g = compileGlob(pattern);
  return g instanceof GlobSyntaxError ? g.message : null;
}

/** `Glob::new(pattern).is_match(value)` with the backend's fallback to equality for invalid globs. */
export function globMatches(pattern: string, value: string): boolean {
  const g = compileGlob(pattern);
  return g instanceof RegExp ? g.test(value) : pattern === value;
}
