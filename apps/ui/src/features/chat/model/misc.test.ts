import { beforeEach, describe, expect, it } from "vitest";
import { MODE_INFO, modeInfo, sandboxInfo } from "./labels";
import { cachedImageChars, forgetSessionImages, imagesFor, rememberImage } from "./imageCache";
import type { Item } from "./items";
import { describeRunError, isStandardErrorMessage, lastRunNotice } from "./runNotice";
import { distanceFromBottom, isNearBottom } from "./scroll";

describe("run errors", () => {
  it("does not present a stop as an error", () => {
    const c = describeRunError("cancelled");
    expect(c.tone).toBe("neutral");
    expect(c.title).toBe("Run interrotto");
    expect(c.link).toBeUndefined();
  });
  it("sends a runtime failure to the runtimes and a blocked prompt to the hooks", () => {
    expect(describeRunError("runtime")).toMatchObject({ tone: "crit", link: { to: "/runtimes" } });
    expect(describeRunError("hook_blocked")).toMatchObject({ tone: "warn", link: { to: "/security/hooks" } });
  });
  it("has a generic failure for any other code", () => {
    expect(describeRunError(undefined)).toMatchObject({ tone: "crit", title: "Run non riuscito" });
    expect(describeRunError("boh").title).toBe("Run non riuscito");
  });
  it("recognizes the standard messages that the title already says", () => {
    expect(isStandardErrorMessage("cancelled", "Run annullato")).toBe(true);
    expect(isStandardErrorMessage("cancelled", "Run annullato dall'utente")).toBe(true);
    expect(isStandardErrorMessage("runtime", "Errore del runtime: boom")).toBe(false);
    expect(isStandardErrorMessage("runtime", " ")).toBe(true);
  });
});

describe("lastRunNotice", () => {
  const items: Item[] = [{ kind: "user", id: "u", text: "ciao" }];
  it("explains a history that ends with a stopped or failed run", () => {
    expect(lastRunNotice("cancelled", items, false)).toMatchObject({ tone: "neutral" });
    expect(lastRunNotice("failed", items, false)).toMatchObject({ tone: "crit" });
  });
  it("says nothing for a good run, a running one, an empty history or one that already ends with the error", () => {
    expect(lastRunNotice("succeeded", items, false)).toBeNull();
    expect(lastRunNotice(undefined, items, false)).toBeNull();
    expect(lastRunNotice("cancelled", items, true)).toBeNull();
    expect(lastRunNotice("cancelled", [], false)).toBeNull();
    expect(lastRunNotice("cancelled", [...items, { kind: "error", id: "e", message: "x", code: "cancelled" }], false)).toBeNull();
  });
});

describe("scroll", () => {
  it("measures the distance from the bottom", () => {
    expect(distanceFromBottom({ scrollTop: 600, scrollHeight: 1000, clientHeight: 400 })).toBe(0);
    expect(distanceFromBottom({ scrollTop: 0, scrollHeight: 1000, clientHeight: 400 })).toBe(600);
    expect(distanceFromBottom({ scrollTop: 700, scrollHeight: 1000, clientHeight: 400 })).toBe(0);
  });
  it("follows only readers who are at the bottom", () => {
    expect(isNearBottom({ scrollTop: 580, scrollHeight: 1000, clientHeight: 400 })).toBe(true);
    expect(isNearBottom({ scrollTop: 500, scrollHeight: 1000, clientHeight: 400 })).toBe(false);
    expect(isNearBottom({ scrollTop: 500, scrollHeight: 1000, clientHeight: 400 }, 120)).toBe(true);
  });
});

describe("labels", () => {
  it("describes every approval mode and sandbox in Italian", () => {
    expect(MODE_INFO.yolo.tone).toBe("crit");
    expect(modeInfo("ask").label).toBe("Chiedi");
    expect(modeInfo("boh").label).toBe("boh");
    expect(modeInfo(undefined).label).toBe("—");
    expect(sandboxInfo("read-only").label).toBe("Sola lettura");
    expect(sandboxInfo("off").tone).toBe("crit");
  });
});

describe("image cache", () => {
  beforeEach(() => {
    for (const s of ["a", "b", "c"]) forgetSessionImages(s);
  });
  it("remembers images by session and tool call", () => {
    rememberImage("a", "c1", { mime: "image/png", data: "AAAA" });
    rememberImage("a", "c1", { mime: "image/png", data: "AAAA" });
    rememberImage("a", "c1", { mime: "image/png", data: "BBBB" });
    expect(imagesFor("a")("c1")).toHaveLength(2);
    expect(imagesFor("a")("c2")).toBeUndefined();
    expect(imagesFor("b")("c1")).toBeUndefined();
    expect(cachedImageChars()).toBe(8);
  });
  it("forgets a session", () => {
    rememberImage("b", "c1", { mime: "image/png", data: "AAAA" });
    forgetSessionImages("b");
    expect(imagesFor("b")("c1")).toBeUndefined();
    expect(cachedImageChars()).toBe(0);
  });
  it("drops the least recently used session when over budget, never the current one", () => {
    const big = "A".repeat(15_000_000);
    rememberImage("a", "c1", { mime: "image/png", data: big });
    rememberImage("b", "c1", { mime: "image/png", data: `B${big}` });
    expect(imagesFor("a")("c1")).toBeUndefined();
    expect(imagesFor("b")("c1")).toHaveLength(1);
  });
});
