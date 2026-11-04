/**
 * Voice-activity detector for the microphone capture. Pure: it is fed the frames and tells what
 * to do, so the thresholds and the trimming are testable without an audio device.
 */

export interface VadOptions {
  /** Stop after this much silence once speech has started (ms); 0 = manual stop only. */
  silenceMs?: number;
  /** Give up when no speech starts within this time (ms). */
  noSpeechMs?: number;
  /** Hard cap (ms). */
  maxMs?: number;
}

export type VadVerdict = "continue" | "end";

export interface VadStep {
  /** Input level 0..1, for the meter. */
  level: number;
  voiced: boolean;
  /** `true` on the frame where speech starts. */
  speechStarted: boolean;
  verdict: VadVerdict;
}

export const DEFAULT_MAX_MS = 120_000;
/** Silence kept around the speech when trimming (whisper tends to invent words in long silences). */
export const PAD_MS = 300;
const CALIBRATION_MS = 400;
const MIN_THRESHOLD = 0.012;

export function rms(frame: ArrayLike<number>): number {
  if (frame.length === 0) return 0;
  let sum = 0;
  for (let i = 0; i < frame.length; i++) sum += frame[i] * frame[i];
  return Math.sqrt(sum / frame.length);
}

export class Vad {
  private noise = 0;
  private elapsed = 0;
  private lastVoice = -1;
  private speechAt = -1;
  readonly voiced: boolean[] = [];

  constructor(private readonly opts: VadOptions = {}) {}

  get speechStarted() {
    return this.speechAt >= 0;
  }

  /** One frame of `frameMs` milliseconds with the given RMS energy. */
  push(energy: number, frameMs: number): VadStep {
    this.elapsed += frameMs;
    // Noise floor: average of the first 400 ms, then slow tracking while nobody speaks.
    if (this.elapsed <= CALIBRATION_MS) this.noise = this.noise === 0 ? energy : (this.noise + energy) / 2;
    const threshold = Math.max(MIN_THRESHOLD, this.noise * 2.5);
    const voiced = energy > threshold;
    this.voiced.push(voiced);
    let speechStarted = false;
    if (voiced) {
      this.lastVoice = this.elapsed;
      if (this.speechAt < 0) {
        this.speechAt = this.elapsed;
        speechStarted = true;
      }
    } else if (this.speechAt < 0 && this.elapsed > CALIBRATION_MS) {
      this.noise = this.noise * 0.95 + energy * 0.05;
    }
    const level = Math.min(1, energy / Math.max(threshold * 4, 0.04));
    const { silenceMs, noSpeechMs, maxMs = DEFAULT_MAX_MS } = this.opts;
    const end =
      (!!silenceMs && this.speechAt >= 0 && this.elapsed - this.lastVoice > silenceMs) ||
      (!!noSpeechMs && this.speechAt < 0 && this.elapsed > noSpeechMs) ||
      this.elapsed > maxMs;
    return { level, voiced, speechStarted, verdict: end ? "end" : "continue" };
  }

  /**
   * Frames to keep: the speech plus ~300 ms around it. `null` when nothing was said.
   * Indexes are `[first, last)` over the frames pushed so far.
   */
  range(frameMs: number): [number, number] | null {
    const first = this.voiced.indexOf(true);
    if (first < 0 || frameMs <= 0) return null;
    const pad = Math.ceil(PAD_MS / frameMs);
    return [Math.max(0, first - pad), Math.min(this.voiced.length, this.voiced.lastIndexOf(true) + 1 + pad)];
  }
}
