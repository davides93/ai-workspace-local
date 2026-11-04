import { describe, expect, it } from "vitest";
import { TARGET_RATE, concat, encodeWav, resample, wavBytes, wavDurationMs } from "./wav";

const ascii = (b: Uint8Array, at: number, n: number) => String.fromCharCode(...b.subarray(at, at + n));

describe("wav", () => {
  it("writes a 16 kHz mono 16-bit PCM header the backend reads back", () => {
    const b = wavBytes(new Float32Array([0, 1, -1, 0.5]), TARGET_RATE);
    const v = new DataView(b.buffer);
    expect([ascii(b, 0, 4), ascii(b, 8, 4), ascii(b, 12, 4), ascii(b, 36, 4)]).toEqual(["RIFF", "WAVE", "fmt ", "data"]);
    expect(v.getUint32(4, true)).toBe(36 + 8);
    expect([v.getUint16(20, true), v.getUint16(22, true), v.getUint32(24, true), v.getUint32(28, true), v.getUint16(34, true)]).toEqual([1, 1, 16_000, 32_000, 16]);
    expect(v.getUint32(40, true)).toBe(8);
    expect([v.getInt16(44, true), v.getInt16(46, true), v.getInt16(48, true)]).toEqual([0, 0x7fff, -0x8000]);
    expect(b.length).toBe(52);
  });

  it("clips samples outside -1..1", () => {
    const v = new DataView(wavBytes(new Float32Array([3, -3]), TARGET_RATE).buffer);
    expect([v.getInt16(44, true), v.getInt16(46, true)]).toEqual([0x7fff, -0x8000]);
  });

  it("encodes a Blob of type audio/wav", () => {
    const blob = encodeWav(new Float32Array(160), TARGET_RATE);
    expect(blob.type).toBe("audio/wav");
    expect(blob.size).toBe(44 + 320);
  });

  it("measures the duration from the size", () => {
    expect(wavDurationMs(44 + 32_000)).toBe(1000);
    expect(wavDurationMs(10)).toBe(0);
  });

  it("concatenates frames in order", () => {
    expect([...concat([new Float32Array([1, 2]), new Float32Array([]), new Float32Array([3])])]).toEqual([1, 2, 3]);
  });

  it("resamples by averaging windows, and leaves the same rate alone", () => {
    const x = new Float32Array([0, 1, 2, 3, 4, 5]);
    expect(resample(x, 16_000, 16_000)).toBe(x);
    expect([...resample(x, 48_000, 16_000)]).toEqual([1, 4]);
    expect(resample(new Float32Array(480), 48_000, 16_000).length).toBe(160);
    // Upsampling never reads outside the input.
    expect(resample(new Float32Array([1, 2]), 8_000, 16_000).length).toBe(4);
  });
});
