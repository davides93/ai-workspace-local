import { useCallback, useRef, useState } from "react";
import { useApi } from "../../shared/api/context";
import { useHotkey } from "../../shared/lib/useHotkey";
import { toast } from "../../shared/lib/toast";
import { useVoiceStatus } from "./api";
import { playSpeech } from "./voice/playback";
import { readAloud, type ReaderDeps } from "./voice/reader";
import { speak as speakSystem, voiceFor } from "./voice/tts";

// One reading at a time in the whole app: starting another stops the one that speaks.
let active: AbortController | null = null;

export interface ReadAloudOptions {
  /** A controller the caller keeps, to stop the reading (the voice conversation). */
  own?: AbortController;
  /** No notice when the device has no voice at all. */
  quiet?: boolean;
  /** Language of the text, when the caller knows it (otherwise the setting, then a guess). */
  language?: string;
}

/**
 * Reads text aloud: with the engine's neural voice when it speaks the language (sentence by
 * sentence, the next one requested while the current one plays), with the system's local voices
 * otherwise or when the engine fails. `read` resolves when done or stopped. Esc stops a reading started here.
 */
export function useReader() {
  const api = useApi();
  const status = useVoiceStatus();
  const latest = useRef(status.data);
  latest.current = status.data;
  const mine = useRef<AbortController | null>(null);
  const [speaking, setSpeaking] = useState(false);

  const stop = useCallback(() => {
    mine.current?.abort();
  }, []);

  const read = useCallback(
    async (markdown: string, { own, quiet = false, language: forced }: ReadAloudOptions = {}): Promise<void> => {
      const st = latest.current;
      if (!markdown.trim()) return;
      active?.abort();
      const ac = own ?? new AbortController();
      active = ac;
      const ours = !own;
      if (ours) {
        mine.current = ac;
        setSpeaking(true);
      }
      let heard = false;
      const deps: ReaderDeps = {
        synth: (text, language) => api.speak({ text, language: language || undefined }, ac.signal),
        play: (s, signal) => {
          heard = true;
          return playSpeech(s, signal);
        },
        system: async (text, language, voice, signal) => {
          const v = await voiceFor(language, voice);
          if (!text || !v || signal.aborted) return;
          heard = true;
          await speakSystem(text, v, signal);
        },
      };
      try {
        await readAloud(
          markdown,
          {
            tts: st?.tts,
            language: forced ?? (st?.language && st.language !== "auto" ? st.language : undefined),
            voice: st?.tts_voice ?? "",
            fallbackLanguage: navigator.language || "it",
            signal: ac.signal,
            onFallback: (why) => toast(`Voce neurale non disponibile (${why}): leggo con la voce del sistema`),
          },
          deps,
        );
        if (!heard && !quiet && !ac.signal.aborted) toast("Nessuna voce disponibile su questo dispositivo: installa Piper o una voce di sistema");
      } finally {
        if (active === ac) active = null;
        if (ours) {
          mine.current = null;
          setSpeaking(false);
        }
      }
    },
    [api],
  );

  useHotkey({ key: "Escape" }, () => {
    if (mine.current) stop();
  });

  return { read, stop, speaking };
}

export type Reader = ReturnType<typeof useReader>;
