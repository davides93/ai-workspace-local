// Which fenced-code languages have a grammar. This file is tiny and always loaded: the grammars (`tokenize.ts`) are a separate chunk
// that is fetched the first time a block in one of these languages is shown.

export const GRAMMARS = ["js", "ts", "json", "python", "shell", "rust", "c", "java", "go", "csharp", "css", "html", "sql", "yaml", "toml", "diff"] as const;
export type GrammarId = (typeof GRAMMARS)[number];

const ALIASES: Record<string, GrammarId> = {
  js: "js", javascript: "js", jsx: "js", mjs: "js", cjs: "js", node: "js",
  ts: "ts", typescript: "ts", tsx: "ts", mts: "ts",
  json: "json", jsonc: "json", json5: "json", jsonl: "json",
  py: "python", python: "python", python3: "python",
  sh: "shell", bash: "shell", shell: "shell", zsh: "shell", console: "shell", "shell-session": "shell", shellsession: "shell", curl: "shell",
  rs: "rust", rust: "rust",
  c: "c", h: "c", cpp: "c", "c++": "c", cc: "c", cxx: "c", hpp: "c",
  java: "java",
  go: "go", golang: "go",
  cs: "csharp", csharp: "csharp", "c#": "csharp",
  css: "css", scss: "css", less: "css",
  html: "html", htm: "html", xml: "html", svg: "html", xhtml: "html", plist: "html",
  sql: "sql", psql: "sql", sqlite: "sql", mysql: "sql", postgres: "sql", postgresql: "sql",
  yaml: "yaml", yml: "yaml",
  toml: "toml",
  diff: "diff", patch: "diff",
};

/** The grammar for the language of a code fence (`ts`, `Python`, `c++`…), `undefined` when there is none: such a block stays plain. */
export function grammarFor(lang: string | undefined): GrammarId | undefined {
  if (!lang) return undefined;
  const key = lang.trim().toLowerCase();
  return Object.hasOwn(ALIASES, key) ? ALIASES[key] : undefined;
}
