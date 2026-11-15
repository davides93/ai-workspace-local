import { describe, expect, it } from "vitest";
import { enqueueBrowserInput, flushBrowserInput, onFlushBrowserInput, toFrame } from "./api";

describe("toFrame", () => {
  it("takes a frame of the contract as it is", () => {
    expect(toFrame({ image: "data:image/jpeg;base64,/9j/4AAQ", width: 1000, height: 500 })).toEqual({ image: "data:image/jpeg;base64,/9j/4AAQ", width: 1000, height: 500 });
  });
  it("only an image data URL is ever put in `src`", () => {
    for (const image of ["javascript:alert(1)", "https://example.com/x.png", "data:text/html;base64,PGI+", "", "//example.com/x.png"]) {
      expect(toFrame({ image, width: 1000, height: 500 }), image).toBeNull();
    }
    expect(toFrame({ image: 42 as unknown as string, width: 1, height: 1 })).toBeNull();
  });
  it("a size that is not a size becomes the usual one", () => {
    expect(toFrame({ image: "data:image/png;base64,AAAA", width: 0, height: Number.NaN })).toMatchObject({ width: 1000, height: 500 });
    expect(toFrame({ image: "data:image/png;base64,AAAA", width: -5, height: Infinity })).toMatchObject({ width: 1000, height: 500 });
    expect(toFrame({ image: "data:image/png;base64,AAAA", width: 640.4, height: 480.6 })).toMatchObject({ width: 640, height: 481 });
  });
});

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("the queue of the person's input", () => {
  it("sends one request at a time, in the order of the keys, whatever each takes", async () => {
    const log: string[] = [];
    enqueueBrowserInput(async () => { await sleep(30); log.push("a"); });
    enqueueBrowserInput(async () => { await sleep(5); log.push("b"); });
    enqueueBrowserInput(async () => { log.push("c"); });
    await flushBrowserInput();
    expect(log).toEqual(["a", "b", "c"]);
  });

  it("«Restituisci» and «Ho finito» wait for it: flushing resolves only after every key queued before is sent", async () => {
    const log: string[] = [];
    for (const k of ["x", "y", "z"]) enqueueBrowserInput(async () => { await sleep(20); log.push(k); });
    await flushBrowserInput();
    log.push("give back");
    expect(log).toEqual(["x", "y", "z", "give back"]);
  });

  it("what a view still gathers (a scroll) is queued by the flush, before it waits", async () => {
    const log: string[] = [];
    const off = onFlushBrowserInput(() => enqueueBrowserInput(async () => { log.push("scroll"); }));
    enqueueBrowserInput(async () => { await sleep(10); log.push("key"); });
    await flushBrowserInput();
    off();
    expect(log).toEqual(["key", "scroll"]);
    // A view that went away is not asked any more.
    log.length = 0;
    await flushBrowserInput();
    expect(log).toEqual([]);
  });

  it("a request that fails does not stop the ones behind it (it handles its own failure)", async () => {
    const log: string[] = [];
    enqueueBrowserInput(async () => { try { throw new Error("409"); } catch { log.push("refused"); } });
    enqueueBrowserInput(async () => { log.push("next"); });
    await flushBrowserInput();
    expect(log).toEqual(["refused", "next"]);
  });
});
