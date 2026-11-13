import { describe, expect, it } from "vitest";
import type { Speech, TtsStatus } from "../../../shared/api/types";
import { guessLanguage, neuralFor, neuralPieces, speakPieces, type NeuralDeps } from "./neural";
import { readAloud, type ReaderDeps } from "./reader";

const tts = (over: Partial<TtsStatus> = {}): TtsStatus => ({
  setting: "auto", engine: "piper", neural: true, backend: "Piper · it_it-paola-medium", detail: null, voices: [], languages: ["it"],
  any_language: false, picks: {}, speed: 1, max_chars: 4000, ...over,
});
const speech = (n: number): Speech => ({ mime: "audio/wav", audio: "AAAA", backend: "Piper", elapsed_ms: n });

describe("neuralFor", () => {
  it("needs a neural engine that speaks the language", () => {
    expect(neuralFor(undefined, "it")).toBe(false);
    expect(neuralFor(tts({ neural: false }), "it")).toBe(false);
    expect(neuralFor(tts(), "it-IT")).toBe(true);
    expect(neuralFor(tts(), "en")).toBe(false);
    expect(neuralFor(tts(), undefined)).toBe(true);
    expect(neuralFor(tts({ languages: [], any_language: true }), "de")).toBe(true);
  });
});

describe("neuralPieces and guessLanguage", () => {
  it("cuts the reply into sentence-sized pieces without markup", () => {
    const pieces = neuralPieces("## Titolo\n\nCiao **a tutti**. Ecco `codice` e [un link](https://x.y).\n\n```js\nfoo()\n```");
    expect(pieces.join(" ")).toContain("Ciao a tutti.");
    expect(pieces.join(" ")).not.toMatch(/[*`#]|https/);
    expect(pieces.every((p) => p.length <= 220)).toBe(true);
  });
  it("guesses Italian or English from the words", () => {
    expect(guessLanguage("Il gatto è sul tavolo e non vuole scendere")).toBe("it");
    expect(guessLanguage("The cat is on the table and it will not come down")).toBe("en");
    expect(guessLanguage("42")).toBeUndefined();
  });
});

describe("speakPieces", () => {
  const tick = () => new Promise((r) => setTimeout(r, 0));

  it("requests the next piece while the current one plays", async () => {
    const log: string[] = [];
    const deps: NeuralDeps = {
      synth: async (text, lang) => {
        log.push(`synth:${text}:${lang}`);
        return speech(text.length);
      },
      play: async (s) => {
        log.push(`play:${s.elapsed_ms}`);
        await tick();
        log.push(`end:${s.elapsed_ms}`);
      },
    };
    const out = await speakPieces(["uno", "due", "tre"], deps, new AbortController().signal, "it");
    expect(out).toEqual({ spoken: 3 });
    // The second piece is requested as soon as the first one starts, never before the first is ready.
    expect(log).toEqual([
      "synth:uno:it", "synth:due:it", "play:3", "end:3", "synth:tre:it", "play:3", "end:3", "play:3", "end:3",
    ]);
  });

  it("stops on the first failure and says how far it got", async () => {
    const deps: NeuralDeps = {
      synth: async (text) => {
        if (text === "due") throw new Error("422");
        return speech(1);
      },
      play: async () => undefined,
    };
    const out = await speakPieces(["uno", "due", "tre"], deps, new AbortController().signal);
    expect(out.spoken).toBe(1);
    expect((out.error as Error).message).toBe("422");
  });

  it("stops without an error when aborted, even with a request in flight", async () => {
    const ac = new AbortController();
    let release: () => void = () => undefined;
    const deps: NeuralDeps = {
      synth: (text) => (text === "due" ? new Promise<Speech>((_, reject) => { release = () => reject(new Error("late")); }) : Promise.resolve(speech(1))),
      play: async () => {
        ac.abort();
      },
    };
    const out = await speakPieces(["uno", "due"], deps, ac.signal);
    expect(out).toEqual({ spoken: 1 });
    release();
    await tick(); // the late rejection is swallowed
  });

  it("does nothing for no pieces", async () => {
    expect(await speakPieces([], { synth: async () => speech(1), play: async () => undefined }, new AbortController().signal)).toEqual({ spoken: 0 });
  });
});

describe("readAloud", () => {
  const base = { voice: "", fallbackLanguage: "it" };
  const make = (over: Partial<ReaderDeps> = {}) => {
    const said: string[] = [];
    const played: number[] = [];
    const deps: ReaderDeps = {
      synth: async (text) => speech(text.length),
      play: async (s) => void played.push(s.elapsed_ms),
      system: async (text) => void said.push(text),
      ...over,
    };
    return { deps, said, played };
  };

  it("uses the neural voice when it speaks the language", async () => {
    const { deps, said, played } = make();
    const r = await readAloud("Ciao a tutti. Come stai?", { ...base, tts: tts(), signal: new AbortController().signal }, deps);
    expect(r).toEqual({ engine: "neural", fellBack: false });
    expect(played.length).toBeGreaterThan(0);
    expect(said).toEqual([]);
  });

  it("uses the system voice without a neural engine or for another language", async () => {
    const a = make();
    expect(await readAloud("Ciao.", { ...base, tts: tts({ neural: false }), signal: new AbortController().signal }, a.deps)).toEqual({ engine: "system", fellBack: false });
    expect(a.said).toEqual(["Ciao."]);
    const b = make();
    await readAloud("The weather is nice and you can go out.", { ...base, tts: tts(), signal: new AbortController().signal }, b.deps);
    expect(b.said).toHaveLength(1);
    expect(b.played).toEqual([]);
  });

  it("falls back to the system voice for what is left when the engine fails", async () => {
    let n = 0;
    const { deps, said, played } = make({
      synth: async (text) => {
        if (++n === 2) throw new Error("server giù");
        return speech(text.length);
      },
    });
    const reasons: string[] = [];
    const long = Array.from({ length: 20 }, (_, i) => `Frase numero ${i + 1} con alcune parole.`).join(" ");
    const r = await readAloud(long, { ...base, tts: tts(), language: "it", signal: new AbortController().signal, onFallback: (w) => reasons.push(w) }, deps);
    expect(r).toEqual({ engine: "neural", fellBack: true });
    expect(played).toHaveLength(1);
    expect(reasons).toEqual(["server giù"]);
    expect(said[0]).toContain("Frase numero 20");
    expect(said[0]).not.toContain("Frase numero 1 ");
  });

  it("does not fall back when stopped on purpose", async () => {
    const ac = new AbortController();
    const { deps, said } = make({ play: async () => void ac.abort() });
    const r = await readAloud("Ciao a tutti. Come stai oggi?", { ...base, tts: tts(), language: "it", signal: ac.signal }, deps);
    expect(r.fellBack).toBe(false);
    expect(said).toEqual([]);
  });
});
