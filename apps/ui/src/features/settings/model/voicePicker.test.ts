import { describe, expect, it } from "vitest";
import type { TtsStatus } from "../../../shared/api/types";
import { engineLine, nextPicks, pickerLanguages, savedPick, voicesFor } from "./voicePicker";

const status = (over: Partial<TtsStatus> = {}): TtsStatus => ({
  setting: "auto", engine: "piper", neural: true, backend: "Piper · it_it-paola-medium", detail: null,
  voices: [
    { id: "it_it-paola-medium", name: "Voce paola", language: "it", engine: "piper" },
    { id: "it_it-riccardo-x_low", name: "Voce riccardo", language: "it", engine: "piper" },
    { id: "de_de-thorsten-medium", name: "Voce thorsten", language: "de", engine: "piper" },
  ],
  languages: ["de", "it"], any_language: false, picks: { it: "it_it-paola-medium" }, speed: 1, max_chars: 4000, ...over,
});

describe("voice picker model", () => {
  it("offers Italian and English plus what the engine speaks", () => {
    expect(pickerLanguages(status())).toEqual(["it", "en", "de"]);
    expect(pickerLanguages(status({ voices: [], languages: [] }))).toEqual(["it", "en"]);
  });
  it("lists the voices of a language", () => {
    expect(voicesFor(status(), "it").map((v) => v.id)).toEqual(["it_it-paola-medium", "it_it-riccardo-x_low"]);
    expect(voicesFor(status(), "en")).toEqual([]);
  });
  it("reads and writes the per-language choice, keeping the other languages", () => {
    const saved = { it: "it_it-riccardo-x_low", en: "en_us-amy-medium" };
    expect(savedPick(saved, "it")).toBe("it_it-riccardo-x_low");
    expect(savedPick(saved, "de")).toBeUndefined();
    expect(savedPick(undefined, "it")).toBeUndefined();
    expect(nextPicks(saved, "it", "it_it-paola-medium")).toEqual({ it: "it_it-paola-medium", en: "en_us-amy-medium" });
    // Back to automatic: the language is dropped, and when nothing is left the key is removed.
    expect(nextPicks(saved, "it", "")).toEqual({ en: "en_us-amy-medium" });
    expect(nextPicks({ it: "x" }, "it", "")).toBeNull();
    expect(nextPicks(undefined, "en", "en_us-amy-medium")).toEqual({ en: "en_us-amy-medium" });
    expect(nextPicks({ it: 5, en: "" }, "de", "d")).toEqual({ de: "d" });
  });
  it("says what speaks and why a neural voice is missing", () => {
    expect(engineLine(status())).toEqual({ tone: "ok", text: "Voce neurale: Piper · it_it-paola-medium" });
    expect(engineLine(status({ neural: false, engine: "system", backend: null, detail: "Piper non è installato" }))).toEqual({ tone: "info", text: "Piper non è installato" });
    expect(engineLine(status({ neural: false, engine: "system", backend: null })).text).toContain("voci del sistema");
  });
});
