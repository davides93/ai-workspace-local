import { describe, expect, it } from "vitest";
import { reduceRuntime } from "./reduce";
import type { Item } from "./items";

const first = {
  requested: "auto", runtime: "llama-server", name: "llama.cpp server", model: "qwen", context: 8192,
  reason: "tutto il modello su CUDA; già in esecuzione; contesto 8192",
  alternatives: [{ runtime: "ollama", name: "Ollama", reason: "sulla CPU (alternativa)" }],
  excluded: [{ runtime: "vllm", name: "vLLM", reason: "non installato" }], fallback: null,
};

describe("aiw.runtime", () => {
  it("the choice is a row for Developer mode, with the reasons and the alternatives", () => {
    const items = reduceRuntime([], first);
    expect(items).toHaveLength(1);
    const row = items[0] as Extract<Item, { kind: "route" }>;
    expect(row.kind).toBe("route");
    expect(row.label).toBe("Runtime automatico: llama.cpp server");
    expect(row.lines).toEqual([
      "Perché: tutto il modello su CUDA; già in esecuzione; contesto 8192",
      "Modello: qwen · contesto 8192",
      "Alternativa · Ollama: sulla CPU (alternativa)",
      "Escluso · vLLM: non installato",
    ]);
  });

  it("a fallback is a note for everybody, with the plain sentence", () => {
    const items = reduceRuntime(reduceRuntime([], first), { ...first, runtime: "llama-embedded", name: "motore interno", fallback: { from: "llama-server", note: "llama-server non è partito: uso motore interno" } });
    expect(items.map((i) => i.kind)).toEqual(["route", "note", "route"]);
    expect((items[1] as Extract<Item, { kind: "note" }>).text).toBe("llama-server non è partito: uso motore interno.");
    expect((items[2] as Extract<Item, { kind: "route" }>).label).toBe("Runtime: motore interno");
  });

  it("an event without details still gives a row", () => {
    expect(reduceRuntime([], {})).toHaveLength(1);
  });
});
