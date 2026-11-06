import type { EvalComparison, EvalReport, EvalSide, EvalSuite, EvalTaskResult } from "../types";

// Demo data and rules of the evaluation API, as the engine applies them (crates/aiw-eval).

const iso = (msAgo: number) => new Date(Date.now() - msAgo).toISOString();

export const evalSuites: EvalSuite[] = [
  {
    id: "esempio", name: "Suite di esempio", description: "Cinque task che mostrano i grader a disposizione.", agent: "operator",
    tasks: [
      { id: "crea-nota", prompt: "Crea note.md con scritto ciao", files: { "input.txt": "contenuto iniziale" }, timeout_s: 300,
        graders: [{ kind: "file_exists", path: "note.md" }, { kind: "file_contains", path: "note.md", pattern: "(?i)ciao" }, { kind: "output_matches", pattern: "note\\.md" }] },
      { id: "riassunto", prompt: "Riassumi input.txt in una frase", files: { "input.txt": "Il report trimestrale mostra ricavi in crescita del 12% e costi stabili." },
        graders: [{ kind: "judge", criteria: "Il riassunto è una sola frase e cita la crescita dei ricavi.", threshold: 0.7 }] },
      { id: "json", prompt: "Rispondi con un oggetto JSON con il campo ok", graders: [{ kind: "json_schema", schema: { type: "object", required: ["ok"] } }] },
    ],
  },
  {
    id: "cortesia", name: "Tono delle risposte", agent: "planner",
    tasks: [{ id: "mail", prompt: "Scrivi una breve mail di scuse a un cliente", graders: [{ kind: "judge", criteria: "Il tono è cortese e la mail è breve." }] }],
  },
];

const result = (task_id: string, passed: boolean, score: number, tokens: number, duration_ms: number, detail: string): EvalTaskResult => ({
  task_id, attempt: 1, passed, score, duration_ms, tokens, turns: 3, run_id: `run_demo_${task_id}`,
  graders: [{ kind: passed ? "file_exists" : "output_matches", passed, score: passed ? 1 : 0, detail }],
});

const summarize = (r: EvalReport): EvalReport => {
  const total = r.tasks.length;
  const passed = r.tasks.filter((t) => t.passed).length;
  r.summary = { passed, total, score: total ? r.tasks.reduce((s, t) => s + t.score, 0) / total : 0, pass_rate: total ? passed / total : 0 };
  return r;
};

export const evalReports: EvalReport[] = [
  summarize({
    id: "evr_demo_b", suite_id: "esempio", suite_name: "Suite di esempio", status: "completed", started_at: iso(3_600e3), finished_at: iso(3_500e3),
    profile: "default", repeat: 1, summary: { passed: 0, total: 0, score: 0, pass_rate: 0 },
    tasks: [result("crea-nota", true, 1, 1800, 21_000, "note.md esiste"), result("riassunto", true, 0.85, 2400, 30_000, "0.85 (soglia 0.70): una frase con i ricavi"), result("json", false, 0, 900, 9_000, "la risposta non è JSON")],
  }),
  summarize({
    id: "evr_demo_a", suite_id: "esempio", suite_name: "Suite di esempio", status: "completed", started_at: iso(7_200e3), finished_at: iso(7_100e3),
    profile: "fast-local", repeat: 1, summary: { passed: 0, total: 0, score: 0, pass_rate: 0 },
    tasks: [result("crea-nota", true, 1, 1200, 12_000, "note.md esiste"), result("riassunto", false, 0.4, 1500, 15_000, "0.40 (soglia 0.70): due frasi"), result("json", false, 0, 700, 7_000, "la risposta non è JSON")],
  }),
];

const GRADER_KINDS = ["file_exists", "file_contains", "output_matches", "json_schema", "command", "judge"];

/** The checks the engine makes before storing a suite; the first problem is the message. */
export function suiteProblem(s: EvalSuite): string | null {
  if (!s.id?.trim()) return "la suite deve avere un id";
  if (!s.name?.trim()) return "la suite deve avere un nome";
  if (!Array.isArray(s.tasks) || s.tasks.length === 0) return "la suite deve avere almeno un task";
  const seen = new Set<string>();
  for (const t of s.tasks) {
    if (!t.id?.trim()) return "ogni task deve avere un id";
    if (seen.has(t.id)) return `il task \`${t.id}\` è ripetuto`;
    seen.add(t.id);
    if (!t.prompt?.trim()) return `il task \`${t.id}\` non ha un prompt`;
    if (!Array.isArray(t.graders) || t.graders.length === 0) return `il task \`${t.id}\` non ha grader: non si saprebbe se è riuscito`;
    for (const g of t.graders) {
      if (!GRADER_KINDS.includes(g.kind)) return `suite non valida: grader \`${g.kind}\` sconosciuto`;
      if (g.kind === "command") return "i grader `command` eseguono comandi con i tuoi permessi: si scrivono solo in un file in ~/.aiw/evals o si lanciano dalla CLI";
      if (g.kind === "judge") {
        if (typeof g.criteria !== "string" || !g.criteria.trim()) return "servono i criteri di valutazione (`criteria`)";
        if (g.threshold !== undefined && (typeof g.threshold !== "number" || g.threshold < 0 || g.threshold > 1)) return "threshold deve essere tra 0 e 1";
      }
    }
  }
  return null;
}

function side(rows: EvalTaskResult[]): EvalSide {
  const n = rows.length || 1;
  const passed = rows.filter((r) => r.passed).length;
  return {
    attempts: rows.length, passed, pass_rate: rows.length ? passed / rows.length : 0,
    score: rows.reduce((s, r) => s + r.score, 0) / n, avg_tokens: rows.reduce((s, r) => s + r.tokens, 0) / n, avg_duration_ms: rows.reduce((s, r) => s + r.duration_ms, 0) / n,
  };
}

/** B against A: the deltas are `b - a`; tasks in the order of A, then the ones only B has. */
export function compareReports(a: EvalReport, b: EvalReport): EvalComparison {
  const ids = [...new Set([...a.tasks, ...b.tasks].map((t) => t.task_id))];
  const tasks = ids.map((id) => {
    const ra = a.tasks.filter((t) => t.task_id === id);
    const rb = b.tasks.filter((t) => t.task_id === id);
    const sa = ra.length ? side(ra) : null;
    const sb = rb.length ? side(rb) : null;
    return { task_id: id, a: sa, b: sb, pass_rate_delta: sa && sb ? sb.pass_rate - sa.pass_rate : null, score_delta: sa && sb ? sb.score - sa.score : null };
  });
  const total_a = side(a.tasks);
  const total_b = side(b.tasks);
  const ref = (r: EvalReport) => ({ id: r.id, suite_id: r.suite_id, agent: r.agent ?? null, profile: r.profile ?? null });
  return { a: ref(a), b: ref(b), tasks, total_a, total_b, pass_rate_delta: total_b.pass_rate - total_a.pass_rate, score_delta: total_b.score - total_a.score };
}

export { summarize as refreshSummary };
