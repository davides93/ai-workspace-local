/**
 * Voice I/O in the web view: microphone capture with a small voice-activity detector, 16 kHz mono
 * WAV for whisper.cpp, and text to speech with the system's *local* voices only (a cloud voice
 * would send the reply off the device).
 *
 * Capture uses a ScriptProcessorNode: deprecated but available in every web view we ship on, and
 * unlike an AudioWorklet it needs no extra script URL (the app's CSP allows none).
 */

export const TARGET_RATE = 16_000;

export interface Recording {
  /** The trimmed speech as WAV, or null when nothing was said (or it was cancelled). */
  result: Promise<Blob | null>;
  /** Ends the recording now; `result` still resolves with what was captured. */
  stop(): void;
  /** Drops the recording: `result` resolves to null. */
  cancel(): void;
}

export interface RecordOptions {
  /** Stop after this much silence once speech has started (ms); 0 = manual stop only. */
  silenceMs?: number;
  /** Give up when no speech starts within this time (ms). */
  noSpeechMs?: number;
  /** Hard cap (ms). */
  maxMs?: number;
  /** Input level 0..1, about 10 times a second. */
  onLevel?: (level: number) => void;
  onSpeech?: () => void;
}

type AudioContextCtor = typeof AudioContext;

function audioContextCtor(): AudioContextCtor | undefined {
  if (typeof window === "undefined") return undefined;
  return window.AudioContext ?? (window as unknown as { webkitAudioContext?: AudioContextCtor }).webkitAudioContext;
}

export function micSupported(): boolean {
  return typeof navigator !== "undefined" && !!navigator.mediaDevices?.getUserMedia && !!audioContextCtor();
}

/** A readable reason for a failed microphone start. */
export function micError(e: unknown): string {
  if (typeof window !== "undefined" && window.isSecureContext === false) {
    return "Il microfono funziona solo in un contesto sicuro: usa l'app, oppure apri l'interfaccia da http://127.0.0.1 o in HTTPS.";
  }
  const name = (e as { name?: string } | null)?.name;
  if (name === "NotAllowedError" || name === "SecurityError") return "Permesso del microfono negato: abilitalo nelle impostazioni del sistema o del browser.";
  if (name === "NotFoundError" || name === "OverconstrainedError") return "Nessun microfono disponibile su questo dispositivo.";
  if (name === "NotReadableError" || name === "AbortError") return "Il microfono è occupato da un'altra applicazione.";
  return String((e as Error)?.message ?? e);
}

export async function record(opts: RecordOptions = {}): Promise<Recording> {
  if (!micSupported()) throw Object.assign(new Error("Microfono non supportato in questa vista"), { name: "NotFoundError" });
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const Ctx = audioContextCtor()!;
  const ctx = new Ctx();
  if (ctx.state === "suspended") await ctx.resume().catch(() => undefined);
  const source = ctx.createMediaStreamSource(stream);
  const proc = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  const voiced: boolean[] = [];
  let noise = 0;
  let elapsed = 0;
  let speechAt = -1;
  let lastVoice = -1;
  let done = false;
  let resolve!: (b: Blob | null) => void;
  const result = new Promise<Blob | null>((r) => (resolve = r));

  const finish = (cancelled: boolean) => {
    if (done) return;
    done = true;
    proc.onaudioprocess = null;
    try {
      source.disconnect();
      proc.disconnect();
    } catch {
      /* already disconnected */
    }
    stream.getTracks().forEach((t) => t.stop());
    const rate = ctx.sampleRate;
    void ctx.close().catch(() => undefined);
    opts.onLevel?.(0);
    if (cancelled || speechAt < 0 || chunks.length === 0) return resolve(null);
    // Keep the speech plus ~300 ms around it: whisper tends to invent words in long silences.
    const frameMs = (chunks[0].length / rate) * 1000;
    const pad = Math.ceil(300 / frameMs);
    const first = Math.max(0, voiced.indexOf(true) - pad);
    const last = Math.min(chunks.length, voiced.lastIndexOf(true) + 1 + pad);
    resolve(encodeWav(resample(concat(chunks.slice(first, last)), rate, TARGET_RATE), TARGET_RATE));
  };

  proc.onaudioprocess = (e) => {
    if (done) return;
    const frame = new Float32Array(e.inputBuffer.getChannelData(0));
    chunks.push(frame);
    let sum = 0;
    for (let i = 0; i < frame.length; i++) sum += frame[i] * frame[i];
    const rms = Math.sqrt(sum / frame.length);
    elapsed += (frame.length / ctx.sampleRate) * 1000;
    // Noise floor: average of the first 400 ms, then slow tracking while nobody speaks.
    if (elapsed <= 400) noise = noise === 0 ? rms : (noise + rms) / 2;
    const threshold = Math.max(0.012, noise * 2.5);
    const isVoice = rms > threshold;
    voiced.push(isVoice);
    if (isVoice) {
      lastVoice = elapsed;
      if (speechAt < 0) {
        speechAt = elapsed;
        opts.onSpeech?.();
      }
    } else if (speechAt < 0 && elapsed > 400) {
      noise = noise * 0.95 + rms * 0.05;
    }
    opts.onLevel?.(Math.min(1, rms / Math.max(threshold * 4, 0.04)));
    if (opts.silenceMs && speechAt >= 0 && elapsed - lastVoice > opts.silenceMs) finish(false);
    else if (opts.noSpeechMs && speechAt < 0 && elapsed > opts.noSpeechMs) finish(false);
    else if (elapsed > (opts.maxMs ?? 120_000)) finish(false);
  };
  source.connect(proc);
  // A ScriptProcessor only runs when connected to the output; it writes nothing, so it is silent.
  proc.connect(ctx.destination);
  return { result, stop: () => finish(false), cancel: () => finish(true) };
}

function concat(parts: Float32Array[]): Float32Array {
  const out = new Float32Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/** Box-filter resampling: averaging each output window also removes most aliasing. */
export function resample(input: Float32Array, from: number, to: number): Float32Array {
  if (from === to) return input;
  const ratio = from / to;
  const out = new Float32Array(Math.floor(input.length / ratio));
  for (let i = 0; i < out.length; i++) {
    const start = Math.floor(i * ratio);
    const end = Math.min(input.length, Math.max(start + 1, Math.floor((i + 1) * ratio)));
    let sum = 0;
    for (let j = start; j < end; j++) sum += input[j];
    out[i] = sum / (end - start);
  }
  return out;
}

/** 16-bit PCM mono WAV. */
export function encodeWav(samples: Float32Array, rate: number): Blob {
  const buf = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(buf);
  const ascii = (at: number, s: string) => [...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
  ascii(0, "RIFF");
  v.setUint32(4, 36 + samples.length * 2, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); // PCM
  v.setUint16(22, 1, true); // mono
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  ascii(36, "data");
  v.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const x = Math.max(-1, Math.min(1, samples[i]));
    v.setInt16(44 + i * 2, x < 0 ? x * 0x8000 : x * 0x7fff, true);
  }
  return new Blob([buf], { type: "audio/wav" });
}

// ---------------------------------------------------------------- speech synthesis

export function ttsSupported(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window && typeof SpeechSynthesisUtterance !== "undefined";
}

/** Voices that run on this device (voices are listed asynchronously by some engines). */
export async function localVoices(): Promise<SpeechSynthesisVoice[]> {
  if (!ttsSupported()) return [];
  const get = () => window.speechSynthesis.getVoices().filter((v) => v.localService);
  if (get().length) return get();
  await new Promise<void>((done) => {
    const t = window.setTimeout(done, 1500);
    window.speechSynthesis.addEventListener("voiceschanged", () => { window.clearTimeout(t); done(); }, { once: true });
  });
  return get();
}

/** The preferred voice by name, else the default (or first) local voice for the language. */
export function pickVoice(voices: SpeechSynthesisVoice[], language: string, preferred = ""): SpeechSynthesisVoice | null {
  const byName = preferred && voices.find((v) => v.name === preferred);
  if (byName) return byName;
  const lang = language.toLowerCase().slice(0, 2);
  const same = voices.filter((v) => v.lang.toLowerCase().replace("_", "-").startsWith(lang));
  return same.find((v) => v.default) ?? same[0] ?? null;
}

/** Markdown → text that reads well aloud (tables become "cell, cell." lines). */
export function plainText(md: string): string {
  const lines = md
    .replace(/```[\s\S]*?```/g, "\n(codice omesso)\n")
    .split("\n")
    .flatMap((line) => {
      const t = line.trim();
      if (t.includes("-") && /^\|?[\s:|-]+\|?$/.test(t)) return []; // table separator, rule
      if (/^([*_])(\s*\1){2,}$/.test(t)) return []; // *** or ___ rule
      if (t.split("|").length > 2) {
        const cells = t.replace(/^\||\|$/g, "").split("|").map((c) => c.trim()).filter(Boolean);
        return cells.length ? [`${cells.join(", ")}.`] : [];
      }
      return [line];
    });
  return lines
    .join("\n")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+(.*)$/gm, "$1.")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*>\s?/gm, "")
    .replace(/(\*\*|__|~~|\*|_)(\S(?:.*?\S)?)\1/g, "$2")
    .replace(/https?:\/\/\S+/g, "link")
    .replace(/([.!?;:])\.+/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/** Sentence-sized pieces (long utterances get cut off by some engines). Splits only where the
 *  punctuation is followed by a space, so "INDEX.md" or "3.5" stay whole. */
export function chunks(text: string, max = 200): string[] {
  const out: string[] = [];
  let cur = "";
  for (const s of text.replace(/([.!?;])\s+/g, "$1\u0000").split("\u0000")) {
    let piece = s.trim();
    while (piece.length > max) {
      const cut = piece.lastIndexOf(" ", max);
      const at = cut > 40 ? cut : max;
      if (cur) out.push(cur), (cur = "");
      out.push(piece.slice(0, at));
      piece = piece.slice(at).trim();
    }
    if (!piece) continue;
    if (cur && cur.length + piece.length + 1 > max) out.push(cur), (cur = "");
    cur = cur ? `${cur} ${piece}` : piece;
  }
  if (cur) out.push(cur);
  return out;
}

/** Reads `markdown` aloud; resolves when done or aborted. Never hangs on engines that skip `end`. */
export function speak(markdown: string, voice: SpeechSynthesisVoice, signal?: AbortSignal): Promise<void> {
  const parts = chunks(plainText(markdown));
  const synth = window.speechSynthesis;
  return new Promise((resolve) => {
    let i = 0;
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      resolve();
    };
    signal?.addEventListener("abort", () => { synth.cancel(); finish(); }, { once: true });
    const next = () => {
      if (signal?.aborted || i >= parts.length) return finish();
      const u = new SpeechSynthesisUtterance(parts[i++]);
      u.voice = voice;
      u.lang = voice.lang;
      let advanced = false;
      const go = () => {
        if (advanced) return;
        advanced = true;
        window.clearTimeout(timer);
        next();
      };
      const timer = window.setTimeout(go, 3000 + u.text.length * 95);
      u.onend = go;
      u.onerror = go;
      synth.speak(u);
    };
    synth.cancel();
    next();
  });
}
