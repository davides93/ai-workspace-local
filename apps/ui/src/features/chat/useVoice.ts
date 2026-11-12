import { useCallback, useEffect, useRef, useState } from "react";
import { useHotkey } from "../../shared/lib/useHotkey";
import { toast } from "../../shared/lib/toast";
import type { ApprovalItem, Item } from "./model/items";
import { approvalAnnouncement, replyToSpeak, type Convo } from "./model/voiceReply";
import { useTranscribe, useVoiceStatus } from "./api";
import { micSupported, record, voiceError, type Recording } from "./voice/capture";
import { useReader } from "./useReader";

export type Dictation = "off" | "rec" | "busy";

export interface VoiceInput {
  /** Open session (voice needs one to talk to). */
  sessionId: string | null;
  /** The conversation can take a message. */
  ready: boolean;
  running: boolean;
  items: Item[];
  /** Confirmations the agent is waiting for. */
  pending: ApprovalItem[];
  /** Sends a message; `onRejected` runs when the run could not start. */
  send: (text: string, onRejected?: () => void) => void;
  /** Adds dictated text to the message box. */
  insertText: (text: string) => void;
}

/**
 * Voice in the chat: dictation into the message box, and the hands-free conversation (listen →
 * send → read the reply aloud with the neural voice or a local system voice → listen again). Esc, or leaving the
 * session, ends the conversation. Audio capture, speech and the reply text live in `voice/` and
 * `model/voiceReply.ts`; this hook only sequences them.
 */
export function useVoice(input: VoiceInput) {
  const status = useVoiceStatus();
  const transcribe = useTranscribe();
  const reader = useReader();
  const [dictation, setDictation] = useState<Dictation>("off");
  const [convo, setConvo] = useState<Convo>("off");
  const [level, setLevel] = useState(0);
  const recording = useRef<Recording | null>(null);
  const speaking = useRef<AbortController | null>(null);
  // The async loop reads the state from here: it changes together with `setConvo`, not one render later.
  const convoRef = useRef<Convo>("off");
  // "sent": the message was handed to the conversation; "running": its run has started (so its end is the reply).
  const turn = useRef<"idle" | "sent" | "running">("idle");
  const latest = useRef(input);
  latest.current = input;

  const available = status.data?.available ?? true;
  const language = status.data?.language && status.data.language !== "auto" ? status.data.language : undefined;

  const goto = useCallback((c: Convo) => {
    convoRef.current = c;
    setConvo(c);
  }, []);

  const unavailable = () => {
    if (status.data && !status.data.available) {
      toast(`Voce non disponibile: ${status.data.detail ?? "nessun motore di trascrizione"}`, "crit");
      return true;
    }
    return false;
  };

  const stopConvo = useCallback(() => {
    goto("off");
    turn.current = "idle";
    recording.current?.cancel();
    recording.current = null;
    speaking.current?.abort();
    speaking.current = null;
    setLevel(0);
  }, [goto]);

  const fail = useCallback(
    (e: unknown) => {
      stopConvo();
      toast(voiceError(e), "crit");
    },
    [stopConvo],
  );

  /** Reads `text` aloud with the engine's neural voice, else the preferred local system voice, if there is one. */
  const say = useCallback((text: string, abort?: AbortController) => reader.read(text, { own: abort ?? new AbortController(), quiet: true }), [reader.read]);

  const dictate = async () => {
    if (dictation === "rec") return recording.current?.stop();
    if (dictation === "busy" || unavailable()) return;
    try {
      const rec = await record({ silenceMs: 2500, onLevel: setLevel });
      recording.current = rec;
      setDictation("rec");
      const wav = await rec.result;
      recording.current = null;
      if (!wav) return toast("Non ho sentito nulla: riprova più vicino al microfono");
      setDictation("busy");
      const t = await transcribe.mutateAsync({ wav, language });
      if (!t.text) return toast("Nessuna parola riconosciuta");
      latest.current.insertText(t.text);
    } catch (e) {
      toast(voiceError(e), "crit");
    } finally {
      recording.current = null;
      setDictation("off");
      setLevel(0);
    }
  };

  /** One conversation turn: listen until a pause, transcribe, send. The reply is read in an effect. */
  const listen = async () => {
    // A function, so TypeScript does not narrow the ref across the awaits below.
    const stopped = () => convoRef.current === "off";
    try {
      goto("listening");
      const rec = await record({ silenceMs: 1100, noSpeechMs: 9000, maxMs: 60_000, onLevel: setLevel });
      if (stopped()) return rec.cancel();
      recording.current = rec;
      const wav = await rec.result;
      recording.current = null;
      if (stopped()) return;
      if (!wav) {
        stopConvo();
        return toast("Conversazione a voce in pausa: non ho sentito nulla");
      }
      goto("transcribing");
      const t = await transcribe.mutateAsync({ wav, language });
      if (stopped()) return;
      if (!t.text) return void listen();
      goto("thinking");
      turn.current = "sent";
      latest.current.send(t.text, stopConvo);
    } catch (e) {
      fail(e);
    }
  };

  const toggleConvo = () => {
    if (convo !== "off") return stopConvo();
    if (!input.sessionId || !input.ready) return toast("Apri o crea una sessione per parlare con l'agente");
    if (unavailable() || input.running) return;
    void listen();
  };

  // The voice turn's run ended: read the reply aloud, then listen again.
  useEffect(() => {
    if (convo !== "thinking") return;
    if (input.running) {
      if (turn.current === "sent") turn.current = "running";
      return;
    }
    if (turn.current !== "running") return;
    turn.current = "idle";
    const text = replyToSpeak(latest.current.items);
    goto("speaking");
    void (async () => {
      const ac = new AbortController();
      speaking.current = ac;
      await say(text, ac);
      speaking.current = null;
      if (convoRef.current === "speaking") void listen();
    })();
    // `listen` and `say` read everything through refs: only the run and the conversation state matter here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [input.running, convo]);

  // Hands-free: say that the agent waits for a confirmation (answering it stays a deliberate tap).
  const announced = useRef<string | null>(null);
  const firstPending = input.pending[0]?.req;
  useEffect(() => {
    if (convo !== "thinking" || !firstPending || announced.current === firstPending.id) return;
    announced.current = firstPending.id;
    void say(approvalAnnouncement(firstPending));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [convo, firstPending?.id]);

  // Leaving the session (or the page) ends the conversation.
  useEffect(() => stopConvo, [input.sessionId, stopConvo]);
  useHotkey({ key: "Escape" }, () => {
    if (convoRef.current !== "off") stopConvo();
  });

  return {
    status: status.data,
    supported: micSupported(),
    available,
    dictation,
    convo,
    level,
    waitingApproval: convo === "thinking" && input.pending.length > 0,
    toggleDictation: () => void dictate(),
    toggleConvo,
    stopConvo,
  };
}

export type VoiceController = ReturnType<typeof useVoice>;
