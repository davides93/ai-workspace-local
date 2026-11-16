import { useEffect, useRef, useState } from "react";
import { toast } from "../../../shared/lib/toast";
import { useTranscribe, useVoiceStatus } from "../../chat/api";
import { micSupported, record, voiceError, type Recording } from "../../chat/voice/capture";

import { t } from "../../../shared/i18n";
export type Dictation = "off" | "rec" | "busy";

/**
 * Dictation into a box of this page: records until a pause (or the second tap), transcribes with the engine's local
 * speech to text and hands the words over. They are never sent from here: the person reads them in the box first. It is
 * the dictation of the chat (`useVoice`) without its conversation loop, built on the same capture and transcription.
 */
export function useDictation(onText: (text: string) => void) {
  const status = useVoiceStatus();
  const transcribe = useTranscribe();
  const [state, setState] = useState<Dictation>("off");
  const recording = useRef<Recording | null>(null);
  const alive = useRef(true);
  const latest = useRef(onText);
  latest.current = onText;
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      recording.current?.cancel();
    };
  }, []);

  const available = status.data?.available ?? true;
  const language = status.data?.language && status.data.language !== "auto" ? status.data.language : undefined;

  const toggle = async () => {
    if (state === "rec") return recording.current?.stop();
    if (state === "busy") return;
    if (status.data && !status.data.available) return toast(t("chat.voce_non_disponibile", { p: status.data.detail ?? "" }), "crit");
    try {
      const rec = await record({ silenceMs: 2500 });
      recording.current = rec;
      setState("rec");
      const wav = await rec.result;
      recording.current = null;
      if (!alive.current) return;
      if (!wav) return toast(t("chat.non_ho_sentito_nulla_riprova"));
      setState("busy");
      const heard = await transcribe.mutateAsync({ wav, language });
      if (!alive.current) return;
      if (!heard.text) return toast(t("chat.nessuna_parola_riconosciuta"));
      latest.current(heard.text);
    } catch (e) {
      if (alive.current) toast(voiceError(e), "crit");
    } finally {
      recording.current = null;
      if (alive.current) setState("off");
    }
  };

  return { state, supported: micSupported(), available, detail: status.data?.detail ?? undefined, toggle: () => void toggle() };
}
