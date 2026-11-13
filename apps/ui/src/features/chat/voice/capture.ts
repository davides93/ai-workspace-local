/**
 * Microphone capture in the web view: frames go through the voice-activity detector (`vad.ts`)
 * and the trimmed speech comes out as a 16 kHz mono WAV (`wav.ts`).
 *
 * Capture uses a ScriptProcessorNode: deprecated but available in every web view we ship on, and
 * unlike an AudioWorklet it needs no extra script URL (the app's CSP allows none).
 */
import { errorMessage, errorStatus } from "../../../shared/lib/errors";
import { Vad, rms, type VadOptions } from "./vad";
import { TARGET_RATE, concat, encodeWav, resample } from "./wav";

import { t as tr, t } from "../../../shared/i18n";
export interface Recording {
  /** The trimmed speech as WAV, or null when nothing was said (or it was cancelled). */
  result: Promise<Blob | null>;
  /** Ends the recording now; `result` still resolves with what was captured. */
  stop(): void;
  /** Drops the recording: `result` resolves to null. */
  cancel(): void;
}

export interface RecordOptions extends VadOptions {
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

export async function record(opts: RecordOptions = {}): Promise<Recording> {
  if (!micSupported()) throw Object.assign(new Error(tr("chat.microfono_non_supportato_in_questa")), { name: "NotFoundError" });
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const Ctx = audioContextCtor()!;
  const ctx = new Ctx();
  if (ctx.state === "suspended") await ctx.resume().catch(() => undefined);
  const source = ctx.createMediaStreamSource(stream);
  const proc = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  const vad = new Vad(opts);
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
    const range = chunks.length ? vad.range((chunks[0].length / rate) * 1000) : null;
    if (cancelled || !range) return resolve(null);
    resolve(encodeWav(resample(concat(chunks.slice(range[0], range[1])), rate, TARGET_RATE), TARGET_RATE));
  };

  proc.onaudioprocess = (e) => {
    if (done) return;
    const frame = new Float32Array(e.inputBuffer.getChannelData(0));
    chunks.push(frame);
    const step = vad.push(rms(frame), (frame.length / ctx.sampleRate) * 1000);
    if (step.speechStarted) opts.onSpeech?.();
    opts.onLevel?.(step.level);
    if (step.verdict === "end") finish(false);
  };
  source.connect(proc);
  // A ScriptProcessor only runs when connected to the output; it writes nothing, so it is silent.
  proc.connect(ctx.destination);
  return { result, stop: () => finish(false), cancel: () => finish(true) };
}

/** A readable reason for a failed microphone start. */
export function micError(e: unknown): string {
  if (typeof window !== "undefined" && window.isSecureContext === false) {
    return t("chat.il_microfono_funziona_solo_in");
  }
  const name = (e as { name?: string } | null)?.name;
  if (name === "NotAllowedError" || name === "SecurityError") return t("chat.permesso_del_microfono_negato_abilitalo");
  if (name === "NotFoundError" || name === "OverconstrainedError") return t("chat.nessun_microfono_disponibile_su_questo");
  if (name === "NotReadableError" || name === "AbortError") return t("chat.il_microfono_e_occupato_da");
  return String((e as Error)?.message ?? e);
}

/** What to tell the user about a failure of the voice flow: the backend's own sentence, or the microphone's. */
export function voiceError(e: unknown): string {
  return errorStatus(e) !== undefined ? errorMessage(e) : micError(e);
}
