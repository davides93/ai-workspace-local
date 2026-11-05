import type { PromptOptimization, PromptVariant } from "../types";
import { agents } from "./fixtures";

// Demo data and rules of the prompt optimization API, as the engine applies them (crates/aiw-engine/src/optimize.rs).

const iso = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();

/** Unified diff of two texts, line by line: the demo's stand-in for the one the engine writes. */
export function unifiedDiff(from: string, to: string): string {
  if (from === to) return "";
  const a = from.split("\n");
  const b = to.split("\n");
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  const ctxBefore = a.slice(Math.max(0, head - 3), head);
  const ctxAfter = a.slice(a.length - tail, Math.min(a.length, a.length - tail + 3));
  const removed = a.slice(head, a.length - tail);
  const added = b.slice(head, b.length - tail);
  const start = head - ctxBefore.length + 1;
  return [
    "--- attuale", "+++ variante",
    `@@ -${start},${ctxBefore.length + removed.length + ctxAfter.length} +${start},${ctxBefore.length + added.length + ctxAfter.length} @@`,
    ...ctxBefore.map((l) => ` ${l}`), ...removed.map((l) => `-${l}`), ...added.map((l) => `+${l}`), ...ctxAfter.map((l) => ` ${l}`),
  ].join("\n") + "\n";
}

const promptOf = (agentId: string): string => agents.find((a) => a.id === agentId)?.system_prompt ?? "";

export const variant = (base: string, prompt: string, rationale: string, score: number, pass_rate: number, report_id: string): PromptVariant => ({
  prompt, rationale, score, pass_rate, report_id, diff: unifiedDiff(base, prompt),
});

export const optimizations: PromptOptimization[] = [
  {
    id: "opt_demo_win", suite_id: "esempio", agent_id: "operator", status: "done", started_at: iso(300), finished_at: iso(270), base_prompt: promptOf("operator"),
    baseline_score: 0.62, baseline_pass_rate: 0.33, baseline_report_id: "evr_demo_a", best: 1,
    variants: [
      variant(promptOf("operator"), `${promptOf("operator")}\nControlla il risultato prima di rispondere.`, "Aggiunge una verifica finale", 0.7, 0.33, "evr_demo_b"),
      variant(promptOf("operator"), `${promptOf("operator")}\nCrea sempre i file richiesti e cita il loro nome nella risposta.`, "Rende esplicita la richiesta dei file", 0.95, 1, "evr_demo_b"),
    ],
  },
  {
    id: "opt_demo_none", suite_id: "cortesia", agent_id: "planner", status: "done", started_at: iso(2000), finished_at: iso(1970), base_prompt: promptOf("planner"),
    baseline_score: 0.9, baseline_pass_rate: 1, baseline_report_id: "evr_demo_b",
    variants: [variant(promptOf("planner"), `${promptOf("planner")}\nUsa un tono formale.`, "Più formale", 0.8, 1, "evr_demo_a")],
  },
];

/** Scores the demo gives to the variants of a new optimization: the second one wins. */
export const DEMO_SCORES: [number, number][] = [[0.55, 0.33], [0.9, 1], [0.6, 0.5], [0.5, 0.33], [0.45, 0.33]];

/** Prompts the demo "model" writes for a new optimization. */
export function demoPrompts(base: string, n: number): { prompt: string; rationale: string }[] {
  const hints = [
    ["Controlla il risultato prima di rispondere.", "Aggiunge una verifica finale"],
    ["Fai solo quello che è richiesto e riporta cosa hai fatto davvero.", "Riduce le azioni non richieste"],
    ["Se mancano informazioni, chiedile prima di agire.", "Evita di indovinare"],
    ["Elenca i passi prima di iniziare.", "Rende il piano esplicito"],
    ["Rispondi in due righe al massimo.", "Risposte più brevi"],
  ];
  return hints.slice(0, n).map(([line, rationale]) => ({ prompt: `${base.trim()}\n${line}`, rationale }));
}
