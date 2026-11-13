// Demo of the neural speech: a Piper with four voices "installed" and a tone in place of the speech.

import type { SpeakRequest, Speech, TtsStatus, TtsVoice } from "../types";
import { ApiError } from "../http";

export const MOCK_MAX_CHARS = 4000;

const PIPER_VOICES: TtsVoice[] = [
  { id: "it_it-paola-medium", name: "Voce paola (it_IT, medium)", language: "it", engine: "piper" },
  { id: "it_it-riccardo-x_low", name: "Voce riccardo (it_IT, x_low)", language: "it", engine: "piper" },
  { id: "en_us-lessac-medium", name: "Voce lessac (en_US, medium)", language: "en", engine: "piper" },
  { id: "en_us-amy-medium", name: "Voce amy (en_US, medium)", language: "en", engine: "piper" },
];

const SERVER_VOICES: TtsVoice[] = [
  { id: "if_sara", name: "Sara (Kokoro)", language: "it", engine: "openai" },
  { id: "im_nicola", name: "Nicola (Kokoro)", language: "it", engine: "openai" },
  { id: "af_heart", name: "Heart (Kokoro)", language: "en", engine: "openai" },
  { id: "am_michael", name: "Michael (Kokoro)", language: "en", engine: "openai" },
];

/** What the engine would report with Piper installed (the demo has no server unless an address is set). */
export function mockTtsStatus(eff: Record<string, unknown>): TtsStatus {
  const setting = (["auto", "system", "piper", "openai"].includes(String(eff["voice.tts.engine"])) ? eff["voice.tts.engine"] : "auto") as TtsStatus["setting"];
  const server = String(eff["voice.tts.server_url"] ?? "").trim() !== "";
  const engine: TtsStatus["engine"] = setting === "system" ? "system" : setting === "openai" ? (server ? "openai" : "system") : setting === "auto" || setting === "piper" ? "piper" : "system";
  const voices = engine === "piper" ? PIPER_VOICES : engine === "openai" ? SERVER_VOICES : [];
  const chosen = (eff["voice.tts.voice"] ?? {}) as Record<string, string>;
  const picks: Record<string, string> = {};
  for (const lang of ["it", "en"]) {
    const v = voices.find((x) => x.language === lang && x.id === chosen[lang]) ?? voices.find((x) => x.language === lang);
    if (v) picks[lang] = v.id;
  }
  return {
    setting,
    engine,
    neural: engine !== "system",
    backend: engine === "piper" ? `Piper · ${picks.it ?? "it_it-paola-medium"}` : engine === "openai" ? `${String(eff["voice.tts.server_url"])} · ${picks.it ?? "if_sara"}` : null,
    detail: engine === "system" ? (setting === "openai" ? "indica l'indirizzo del server della voce (per esempio http://localhost:8880)" : null) : null,
    voices,
    languages: engine === "piper" ? ["en", "it"] : [],
    any_language: engine === "openai",
    picks,
    speed: Number(eff["voice.tts.speed"] ?? 1),
    max_chars: MOCK_MAX_CHARS,
  };
}

/** A short quiet tone as a 16-bit mono WAV: the length follows the text, so the pacing looks real. */
export function toneWav(ms: number, rate = 16000): Uint8Array {
  const samples = Math.round((rate * ms) / 1000);
  const out = new Uint8Array(44 + samples * 2);
  const v = new DataView(out.buffer);
  const tag = (at: number, s: string) => [...s].forEach((c, i) => out.set([c.charCodeAt(0)], at + i));
  tag(0, "RIFF");
  v.setUint32(4, 36 + samples * 2, true);
  tag(8, "WAVEfmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  tag(36, "data");
  v.setUint32(40, samples * 2, true);
  for (let i = 0; i < samples; i++) {
    // A soft 330 Hz tone with a fade at both ends.
    const fade = Math.min(1, i / 800, (samples - i) / 800);
    v.setInt16(44 + i * 2, Math.round(Math.sin((2 * Math.PI * 330 * i) / rate) * 2200 * fade), true);
  }
  return out;
}

export function base64Of(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** The demo of `POST /api/voice/speak`: refuses like the engine does (empty, too long, no neural engine). */
export function mockSpeak(req: SpeakRequest, eff: Record<string, unknown>): Speech {
  const status = mockTtsStatus(eff);
  const text = req.text.trim();
  if (!text) throw new ApiError(400, "nessun testo da leggere");
  if ([...text].length > MOCK_MAX_CHARS) throw new ApiError(400, `testo troppo lungo: massimo ${MOCK_MAX_CHARS} caratteri`);
  if (!status.neural) throw new ApiError(422, "la voce neurale non è attiva: nessun motore installato, si usano le voci del sistema");
  const ms = Math.min(1800, Math.max(250, text.length * 20));
  return { mime: "audio/wav", audio: base64Of(toneWav(ms)), backend: status.backend ?? "demo", elapsed_ms: 40 };
}
