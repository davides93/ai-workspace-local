import { describe, expect, it } from "vitest";
import { base64ToBytes, blobToBase64, bytesToBase64 } from "./base64";

describe("base64", () => {
  it("encodes long data without overflowing the call stack", () => {
    const bytes = new Uint8Array(300_000).map((_, i) => i % 251);
    const back = Uint8Array.from(atob(bytesToBase64(bytes)), (c) => c.charCodeAt(0));
    expect(back.length).toBe(bytes.length);
    expect(back[299_999]).toBe(bytes[299_999]);
    expect(bytesToBase64(new TextEncoder().encode("RIFF"))).toBe("UklGRg==");
  });

  it("encodes a Blob", async () => {
    expect(await blobToBase64(new Blob(["RIFF"], { type: "audio/wav" }))).toBe("UklGRg==");
    expect(await blobToBase64(new Blob([]))).toBe("");
  });

  it("decodes back to the same bytes", () => {
    const bytes = new Uint8Array(1000).map((_, i) => (i * 7) % 256);
    expect(Array.from(base64ToBytes(bytesToBase64(bytes)))).toEqual(Array.from(bytes));
    expect(base64ToBytes("").length).toBe(0);
  });
});
