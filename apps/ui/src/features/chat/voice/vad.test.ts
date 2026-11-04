import { describe, expect, it } from "vitest";
import { Vad, rms } from "./vad";

const FRAME = 100; // ms per frame in these tests
const run = (vad: Vad, energies: number[]) => energies.map((e) => vad.push(e, FRAME));

describe("rms", () => {
  it("is the root mean square, and 0 for an empty frame", () => {
    expect(rms([])).toBe(0);
    expect(rms([1, -1, 1, -1])).toBe(1);
    expect(rms(new Float32Array([0.5, 0.5]))).toBeCloseTo(0.5);
  });
});

describe("Vad", () => {
  const quiet = Array(5).fill(0.002);
  const speech = Array(6).fill(0.2);

  it("ends after the silence that follows the speech", () => {
    const vad = new Vad({ silenceMs: 500 });
    const steps = run(vad, [...quiet, ...speech, ...Array(8).fill(0.002)]);
    expect(steps[5].speechStarted).toBe(true);
    expect(steps.filter((s) => s.speechStarted)).toHaveLength(1);
    const end = steps.findIndex((s) => s.verdict === "end");
    expect(end).toBeGreaterThan(10);
    // 500 ms of silence = 5 frames after the last voiced one (index 10), the end comes on the sixth.
    expect(end).toBe(16);
  });

  it("gives up when nobody speaks", () => {
    const vad = new Vad({ noSpeechMs: 1000 });
    const steps = run(vad, Array(15).fill(0.002));
    expect(steps.findIndex((s) => s.verdict === "end")).toBe(10);
    expect(vad.speechStarted).toBe(false);
    expect(vad.range(FRAME)).toBeNull();
  });

  it("never ends by silence with manual stop only, but obeys the hard cap", () => {
    const vad = new Vad({ maxMs: 2000 });
    const steps = run(vad, [...quiet, ...speech, ...Array(30).fill(0.002)]);
    expect(steps.findIndex((s) => s.verdict === "end")).toBe(20);
  });

  it("calls a steady hum background noise, not speech", () => {
    const vad = new Vad({ noSpeechMs: 3000 });
    const steps = run(vad, Array(20).fill(0.02));
    expect(steps.some((s) => s.voiced)).toBe(false);
  });

  it("reports a level between 0 and 1", () => {
    const vad = new Vad();
    for (const s of run(vad, [0, 0.002, 0.2, 5])) {
      expect(s.level).toBeGreaterThanOrEqual(0);
      expect(s.level).toBeLessThanOrEqual(1);
    }
  });

  it("keeps the speech plus 300 ms around it", () => {
    const vad = new Vad({ silenceMs: 0 });
    run(vad, [...Array(10).fill(0.002), ...speech, ...Array(10).fill(0.002)]);
    // Voiced frames 10..15; 300 ms = 3 frames of padding on each side.
    expect(vad.range(FRAME)).toEqual([7, 19]);
  });

  it("does not trim past the ends of the recording", () => {
    const vad = new Vad();
    run(vad, [...Array(5).fill(0.002), 0.3, 0.3]);
    expect(vad.range(FRAME)).toEqual([2, 7]);
  });
});
