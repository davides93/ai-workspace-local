/**
 * Reading replies aloud with the system's *local* voices only (a cloud voice would send the reply
 * off the device).
 */
import { chunks, pickVoice, plainText } from "./speech";

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

/** The local voice for `language` (the preferred one by name wins); null when there is none or no synthesis. */
export async function voiceFor(language: string, preferred = ""): Promise<SpeechSynthesisVoice | null> {
  return ttsSupported() ? pickVoice(await localVoices(), language, preferred) : null;
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
