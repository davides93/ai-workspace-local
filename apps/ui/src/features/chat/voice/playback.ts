import type { Speech } from "../../../shared/api/types";

/** The WAV of a synthesis, as a Blob. */
export function speechBlob(s: Speech): Blob {
  const bin = atob(s.audio);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type: s.mime || "audio/wav" });
}

/** Plays a synthesis; resolves at the end or when `signal` aborts, rejects when the browser cannot play it. */
export function playSpeech(s: Speech, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return resolve();
    const url = URL.createObjectURL(speechBlob(s));
    const audio = new Audio(url);
    const done = (err?: unknown) => {
      signal.removeEventListener("abort", onAbort);
      audio.onended = audio.onerror = null;
      audio.pause();
      URL.revokeObjectURL(url);
      if (err) reject(err);
      else resolve();
    };
    const onAbort = () => done();
    signal.addEventListener("abort", onAbort, { once: true });
    audio.onended = () => done();
    audio.onerror = () => done(new Error("il browser non riesce a riprodurre l'audio"));
    audio.play().catch((e) => done(e));
  });
}
