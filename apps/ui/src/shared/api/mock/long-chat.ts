import type { ChatMessage } from "../types";

/** The session of the `long-chat` scenario. */
export const LONG_CHAT_ID = "ses_long";
/** How many question-and-answer turns it holds: about four hundred rows once the tool cards are counted. */
export const LONG_CHAT_TURNS = 120;
/** Words that appear once, in a known turn: the end-to-end tests look for them (find in page, selection). */
export const LONG_CHAT_FIRST = "Domanda numero 1";
export const LONG_CHAT_NEEDLE = "calibro-quarantadue";
export const LONG_CHAT_LAST = "Fine della conversazione";

interface Snippet {
  lang: string;
  topic: string;
  code: (n: number) => string;
}

// One snippet per grammar of the highlighter, so the scenario also shows every colour.
const SNIPPETS: Snippet[] = [
  { lang: "ts", topic: "un hook che memorizza l'ultimo valore", code: (n) => `export function usePrevious<T>(value: T): T | undefined {\n  const ref = useRef<T>();\n  useEffect(() => { ref.current = value; }, [value]);\n  return ref.current; // turno ${n}\n}` },
  { lang: "python", topic: "un conteggio delle parole", code: (n) => `from collections import Counter\n\ndef top_words(text: str, k: int = ${n % 7 + 3}) -> list[tuple[str, int]]:\n    """Le k parole più frequenti."""\n    return Counter(text.lower().split()).most_common(k)` },
  { lang: "rust", topic: "una funzione che somma i byte", code: (n) => `fn total(sizes: &[u64]) -> u64 {\n    // somma saturante, turno ${n}\n    sizes.iter().fold(0u64, |acc, s| acc.saturating_add(*s))\n}` },
  { lang: "bash", topic: "un ciclo sui file di log", code: (n) => `for f in logs/*.log; do\n  echo "$f: $(wc -l < "$f") righe" # turno ${n}\ndone` },
  { lang: "json", topic: "la configurazione di un runtime", code: (n) => `{\n  "runtime": "llama.cpp",\n  "context": ${2048 * (n % 4 + 1)},\n  "gpu_layers": null,\n  "stream": true\n}` },
  { lang: "sql", topic: "i download più pesanti", code: (n) => `SELECT name, size_mb\nFROM models\nWHERE size_mb > ${n * 10} -- soglia del turno ${n}\nORDER BY size_mb DESC\nLIMIT 5;` },
  { lang: "css", topic: "una scheda con ombra leggera", code: (n) => `.card {\n  padding: ${n % 5 + 1}rem;\n  border: 1px solid var(--line);\n  box-shadow: 0 1px 2px #0002;\n}` },
  { lang: "diff", topic: "una correzione di una riga", code: (n) => `--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -${n},3 +${n},3 @@\n fn limit() -> usize {\n-    ${n}\n+    ${n + 1}\n }` },
  { lang: "yaml", topic: "un job di integrazione continua", code: (n) => `jobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npm test # turno ${n}` },
  { lang: "go", topic: "un server minimo", code: (n) => `func main() {\n\thttp.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {\n\t\tfmt.Fprintln(w, "ciao ${n}")\n\t})\n\tlog.Fatal(http.ListenAndServe(":8080", nil))\n}` },
];

const TOOLS = [
  { name: "read_file", args: (n: number) => ({ path: `src/modulo-${n}.ts` }), result: (n: number) => `export const modulo${n} = ${n};` },
  { name: "grep", args: (n: number) => ({ pattern: `TODO\\(${n}\\)`, path: "src" }), result: (n: number) => `src/modulo-${n}.ts:12: // TODO(${n}) rivedere` },
  { name: "list_dir", args: () => ({ path: "~/Documenti" }), result: () => "12 file · 3 cartelle" },
];

/**
 * A long conversation, built the same way every time: question, a tool call with its result, answer with a code block (every
 * ninth also a table and a list). Used to look at what a conversation of hundreds of rows costs and how it behaves (scrolling,
 * find in page, selection, keyboard); the first and the last turn and one in the middle carry known words.
 */
export function longChatMessages(turns = LONG_CHAT_TURNS, at = () => new Date().toISOString()): ChatMessage[] {
  const out: ChatMessage[] = [];
  for (let n = 1; n <= turns; n++) {
    const s = SNIPPETS[n % SNIPPETS.length];
    const tool = TOOLS[n % TOOLS.length];
    const callId = `c_long_${n}`;
    const needle = n === 42 ? ` Il codice ha un ${LONG_CHAT_NEEDLE}.` : "";
    const last = n === turns ? ` ${LONG_CHAT_LAST} (turno ${turns}).` : "";
    out.push({ id: `l${n}u`, role: "user", content: `Domanda numero ${n}: mostrami ${s.topic}.`, created_at: at() });
    out.push({ id: `l${n}a`, role: "assistant", content: "", tool_calls: [{ id: callId, name: tool.name, arguments: JSON.stringify(tool.args(n)) }], created_at: at() });
    out.push({ id: `l${n}t`, role: "tool", tool_call_id: callId, name: tool.name, content: tool.result(n), created_at: at() });
    let answer = `Ecco ${s.topic}:\n\n\`\`\`${s.lang}\n${s.code(n)}\n\`\`\`\n\nLa risposta al turno ${n} usa **${s.lang}**; il comando \`${tool.name}\` ha confermato il resto.${needle}${last}`;
    if (n % 9 === 0) answer += `\n\n| Voce | Valore |\n|---|---|\n| turno | ${n} |\n| linguaggio | ${s.lang} |\n\n- primo punto\n- secondo punto\n- terzo punto`;
    out.push({ id: `l${n}r`, role: "assistant", content: answer, created_at: at() });
  }
  return out;
}
