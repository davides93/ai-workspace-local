/**
 * Reading a reply aloud with the engine's neural voice (Piper, a speech server), sentence by
 * sentence: while one piece plays the next one is already being synthesised. Pure sequencing: the
 * request and the playback are passed in, so the logic is testable without audio.
 */
import type { Speech, TtsStatus } from "../../../shared/api/types";
import { chunks, plainText } from "./speech";

/** Characters of one request: a sentence or two, so the first sound comes quickly. */
export const PIECE_CHARS = 220;

/** Can the engine speak `language` (a BCP-47 tag or ISO code; undefined = whatever the text is in) with a neural voice? */
export function neuralFor(tts: TtsStatus | undefined, language?: string): boolean {
  if (!tts?.neural) return false;
  if (tts.any_language || !language) return true;
  return tts.languages.includes(language.toLowerCase().slice(0, 2));
}

/** The text split into the pieces sent to the engine, one request each. */
export function neuralPieces(markdown: string, max = PIECE_CHARS): string[] {
  return chunks(plainText(markdown), max);
}

const IT = new Set(["il", "lo", "la", "gli", "le", "che", "di", "è", "un", "una", "per", "non", "sono", "con", "come", "del", "della", "anche", "questo", "questa", "ma", "più", "ho", "hai", "nel", "alla", "ciao", "grazie"]);
const EN = new Set(["the", "and", "is", "are", "of", "to", "that", "with", "for", "this", "it", "you", "have", "not", "was", "be", "on", "hello", "what", "can", "will"]);

/** Italian or English from common words (undefined when unsure): picks the voice when the language setting is "auto". */
export function guessLanguage(text: string): "it" | "en" | undefined {
  let it = 0;
  let en = 0;
  for (const w of text.toLowerCase().split(/[^\p{L}]+/u)) {
    if (IT.has(w)) it++;
    if (EN.has(w)) en++;
  }
  it += Math.min(3, (text.match(/[àèéìòù]/gi) ?? []).length);
  if (it >= 2 && it > en) return "it";
  if (en >= 2 && en > it) return "en";
  return undefined;
}

export interface NeuralDeps {
  /** Synthesises one piece (rejects when the engine cannot). */
  synth: (text: string, language: string) => Promise<Speech>;
  /** Plays it; resolves when it ends or `signal` aborts. */
  play: (speech: Speech, signal: AbortSignal) => Promise<void>;
}

export interface PiecesResult {
  /** Pieces fully spoken. */
  spoken: number;
  /** Why it stopped before the end (not set when it was stopped on purpose). */
  error?: unknown;
}

/** Speaks the pieces in order, requesting the next while the current one plays. Stops at the first error. */
export async function speakPieces(pieces: readonly string[], deps: NeuralDeps, signal: AbortSignal, language = ""): Promise<PiecesResult> {
  const request = (i: number) => {
    const p = deps.synth(pieces[i], language);
    p.catch(() => undefined); // a rejection nobody awaits (we stopped first) is not an error
    return p;
  };
  let pending = pieces.length ? request(0) : undefined;
  for (let i = 0; pending; i++) {
    if (signal.aborted) return { spoken: i };
    let speech: Speech;
    try {
      speech = await pending;
    } catch (error) {
      return signal.aborted ? { spoken: i } : { spoken: i, error };
    }
    if (signal.aborted) return { spoken: i };
    pending = i + 1 < pieces.length ? request(i + 1) : undefined;
    try {
      await deps.play(speech, signal);
    } catch (error) {
      return signal.aborted ? { spoken: i } : { spoken: i, error };
    }
    if (signal.aborted) return { spoken: i + 1 };
  }
  return { spoken: pieces.length };
}
