/**
 * Reads a reply aloud: with the engine's neural voice when one speaks the language, falling back
 * to the system's local voices for what is left when the engine fails. Nothing here touches the DOM
 * directly: the engine call, the neural playback and the system voice come in as `deps`.
 */
import type { TtsStatus } from "../../../shared/api/types";
import { guessLanguage, neuralFor, neuralPieces, speakPieces, type NeuralDeps } from "./neural";

export interface ReaderDeps extends NeuralDeps {
  /** Reads `text` with the preferred local system voice; resolves when done or aborted. Does nothing without a voice. */
  system: (text: string, language: string, voice: string, signal: AbortSignal) => Promise<void>;
}

export interface ReadOptions {
  tts?: TtsStatus;
  /** Language of the voice settings; undefined (auto) = guessed from the reply. */
  language?: string;
  /** Preferred system voice (`voice.tts_voice`). */
  voice: string;
  /** Language of the interface, when nothing else says. */
  fallbackLanguage: string;
  signal: AbortSignal;
  /** The neural voice failed: the rest is read with the system voice. */
  onFallback?: (reason: string) => void;
}

export type ReadResult = { engine: "neural" | "system"; fellBack: boolean };

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export async function readAloud(markdown: string, o: ReadOptions, deps: ReaderDeps): Promise<ReadResult> {
  const language = o.language ?? guessLanguage(markdown) ?? o.fallbackLanguage;
  if (!neuralFor(o.tts, language)) {
    await deps.system(markdown, language, o.voice, o.signal);
    return { engine: "system", fellBack: false };
  }
  const pieces = neuralPieces(markdown);
  const out = await speakPieces(pieces, deps, o.signal, language);
  if (out.error === undefined || o.signal.aborted) return { engine: "neural", fellBack: false };
  o.onFallback?.(message(out.error));
  await deps.system(pieces.slice(out.spoken).join(" "), language, o.voice, o.signal);
  return { engine: "neural", fellBack: true };
}
