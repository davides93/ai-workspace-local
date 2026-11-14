import { beforeEach, describe, expect, it } from "vitest";
import { MODE_INFO, modeInfo, sandboxInfo } from "./labels";
import { cachedImageChars, forgetSessionImages, imagesFor, rememberImage, rememberSessionImages } from "./imageCache";
import type { Item } from "./items";
import { describeRunError, errorDetail, isStandardErrorMessage, lastRunNotice, runErrorCode } from "./runNotice";
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

describe("stored run errors", () => {
  it("reads the code back from the message of the runner", () => {
    expect(runErrorCode("Run annullato")).toBe("cancelled");
    expect(runErrorCode("Errore del runtime: il runtime si è chiuso")).toBe("runtime");
    expect(runErrorCode("Runtime non disponibile: connessione rifiutata")).toBe("runtime");
    expect(runErrorCode("Prompt bloccato da un hook: no")).toBe("hook_blocked");
    expect(runErrorCode("boom")).toBeUndefined();
    expect(runErrorCode("Il runtime non risponde")).toBeUndefined();
  });
  it("drops the words that only repeat the title, keeps the reason as a sentence", () => {
    expect(errorDetail("runtime", "Errore del runtime: il runtime si è chiuso")).toBe("Il runtime si è chiuso");
    expect(errorDetail("runtime", "Runtime non disponibile: connessione rifiutata")).toBe("Runtime non disponibile: connessione rifiutata");
    expect(errorDetail("hook_blocked", "Prompt bloccato da un hook: parola vietata")).toBe("Parola vietata");
    expect(errorDetail("cancelled", "Run annullato")).toBeUndefined();
    expect(errorDetail("runtime", "Errore del runtime: ")).toBeUndefined();
    expect(errorDetail(undefined, "boom")).toBe("boom");
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

describe("images replayed by the backend", () => {
  beforeEach(() => {
    for (const s of ["a", "b"]) forgetSessionImages(s);
  });
  const entry = (toolCallId: string, data: string, extra: Record<string, unknown> = {}) => ({ run_id: "run1", toolCallId, mime: "image/png", data, ...extra });
  it("remembers the well-formed entries by tool call, in order", () => {
    rememberSessionImages("a", [entry("c1", "AAAA"), entry("c1", "BBBB"), entry("c2", "CCCC")]);
    expect(imagesFor("a")("c1")).toEqual([
      { mime: "image/png", data: "AAAA" },
      { mime: "image/png", data: "BBBB" },
    ]);
    expect(imagesFor("a")("c2")).toEqual([{ mime: "image/png", data: "CCCC" }]);
    expect(imagesFor("b")("c1")).toBeUndefined();
  });
  it("skips malformed entries and keeps the good ones", () => {
    rememberSessionImages("a", [
      null,
      "testo",
      42,
      { run_id: "r", mime: "image/png", data: "AAAA" },
      entry("c1", "AAAA", { mime: "text/html" }),
      entry("c1", ""),
      entry("c1", "non base64 !!"),
      entry("c1", 7 as unknown as string),
      entry("c2", "DDDD"),
    ]);
    expect(imagesFor("a")("c1")).toBeUndefined();
    expect(imagesFor("a")("c2")).toEqual([{ mime: "image/png", data: "DDDD" }]);
    expect(cachedImageChars()).toBe(4);
  });
  it("ignores an answer that is not a list", () => {
    rememberSessionImages("a", { error: "boom" });
    rememberSessionImages("a", undefined);
    expect(cachedImageChars()).toBe(0);
  });
  it("does not double an image already seen live, nor one replayed twice", () => {
    rememberImage("a", "c1", { mime: "image/png", data: "AAAA" });
    rememberSessionImages("a", [entry("c1", "AAAA")]);
    rememberSessionImages("a", [entry("c1", "AAAA")]);
    expect(imagesFor("a")("c1")).toHaveLength(1);
    expect(cachedImageChars()).toBe(4);
  });
  it("keeps only the payload of a data URL", () => {
    rememberSessionImages("a", [entry("c1", "data:image/png;base64,AAAA")]);
    expect(imagesFor("a")("c1")).toEqual([{ mime: "image/png", data: "AAAA" }]);
  });
});

describe("model roles in the conversation", () => {
  it("labels each change of model once per turn, and shows the check and the collapse as notes", async () => {
    const { reduce: reduceEvent } = await import("./reduce");
    const { modelLabel } = await import("./items");
    const ev = (name: string, value: unknown) => ({ type: "CUSTOM" as const, name, value });
    let items: import("./items").Item[] = [{ kind: "user", id: "u", text: "ciao" }];
    for (const e of [
      ev("aiw.model", { role: "plan", model: "grande" }),
      ev("aiw.model", { role: "plan", model: "grande" }),
      ev("aiw.model", { role: "act", model: "veloce" }),
      ev("aiw.model", { role: "act", model: "veloce" }),
    ]) items = reduceEvent(items, e);
    expect(items.filter((i) => i.kind === "model").map((i) => (i.kind === "model" ? i.label : ""))).toEqual(["Pianifica · grande", "Esegue · veloce"]);
    expect(modelLabel(null, "m")).toBe("m");
    items = reduceEvent(items, ev("aiw.check", { passed: false, note: "manca il file" }));
    items = reduceEvent(items, ev("aiw.roles", { notes: ["Un solo modello."] }));
    items = reduceEvent(items, ev("aiw.roles", { notes: [] }));
    const notes = items.filter((i) => i.kind === "note").map((i) => (i.kind === "note" ? i.text : ""));
    expect(notes).toEqual(["Verifica finale: manca il file. L'agente corregge prima di concludere.", "Un solo modello."]);
  });
});
