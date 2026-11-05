import { describe, expect, it } from "vitest";
import type { AgEvent, ChatMessage } from "../../../shared/api/types";
import { getPath, setPath } from "../a2ui/protocol";
import { historyToItems } from "./history";
import { lastRunNotice } from "./runNotice";
import type { Item, ToolItem } from "./items";
import { parseImageEvent, reduce, settle } from "./reduce";

const fold = (events: AgEvent[], from: Item[] = []): Item[] => events.reduce<Item[]>((items, e) => reduce(items, e), from);

describe("reduce", () => {
  it("streams text into one item and closes it", () => {
    const items = fold([
      { type: "TEXT_MESSAGE_START", messageId: "m1", role: "assistant" },
      { type: "TEXT_MESSAGE_CONTENT", messageId: "m1", delta: "Ciao " },
      { type: "TEXT_MESSAGE_CONTENT", messageId: "m1", delta: "mondo" },
      { type: "TEXT_MESSAGE_END", messageId: "m1" },
    ]);
    expect(items).toEqual([{ kind: "text", id: "m1", text: "Ciao mondo", streaming: false }]);
  });
  it("does not mutate the previous array or its rows", () => {
    const a = reduce([], { type: "TEXT_MESSAGE_START", messageId: "m", role: "assistant" });
    const b = reduce(a, { type: "TEXT_MESSAGE_CONTENT", messageId: "m", delta: "x" });
    expect(a).not.toBe(b);
    expect((a[0] as Extract<Item, { kind: "text" }>).text).toBe("");
  });
  it("keeps the rows that did not change (same references)", () => {
    const a = fold([
      { type: "TEXT_MESSAGE_START", messageId: "m1", role: "assistant" },
      { type: "TEXT_MESSAGE_END", messageId: "m1" },
      { type: "TEXT_MESSAGE_START", messageId: "m2", role: "assistant" },
    ]);
    const b = reduce(a, { type: "TEXT_MESSAGE_CONTENT", messageId: "m2", delta: "x" });
    expect(b[0]).toBe(a[0]);
    expect(b[1]).not.toBe(a[1]);
  });
  it("ignores events it does not show and unknown ids", () => {
    const a = fold([{ type: "TEXT_MESSAGE_START", messageId: "m1", role: "assistant" }]);
    expect(reduce(a, { type: "STEP_STARTED", stepName: "x" })).toBe(a);
    expect(reduce(a, { type: "TOOL_CALL_ARGS", toolCallId: "nope", delta: "{}" })).toBe(a);
    expect(reduce(a, { type: "CUSTOM", name: "aiw.compacted", value: {} })).toBe(a);
  });
  it("builds a tool card from start, args and result, with its timing", () => {
    const items = fold([
      { type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "shell", timestamp: 1000 },
      { type: "TOOL_CALL_ARGS", toolCallId: "c1", delta: '{"command":' },
      { type: "TOOL_CALL_ARGS", toolCallId: "c1", delta: '"ls"}' },
      { type: "TOOL_CALL_RESULT", messageId: "x", toolCallId: "c1", content: "a.txt", timestamp: 3500 },
    ]);
    expect(items[0]).toMatchObject({ kind: "tool", name: "shell", args: '{"command":"ls"}', status: "done", startedAt: 1000, endedAt: 3500 });
  });
  it("tells a denied call from a failed one", () => {
    const run = (content: string) => (fold([
      { type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "shell" },
      { type: "TOOL_CALL_RESULT", messageId: "x", toolCallId: "c1", content },
    ])[0] as ToolItem).status;
    expect(run("Negato dall'utente.")).toBe("denied");
    expect(run("ERRORE: comando non trovato")).toBe("failed");
    expect(run("ERRORE: strumento non eseguito, il run è stato interrotto prima.")).toBe("interrupted");
    expect(run("ok")).toBe("done");
  });
  it("tracks approvals", () => {
    const req = { id: "a1", run_id: "r", tool: "shell", args: {}, reason: "x", risk: "medium" as const };
    const items = fold([
      { type: "CUSTOM", name: "aiw.approval_request", value: req },
      { type: "CUSTOM", name: "aiw.approval_resolved", value: { id: "a1", allowed: true } },
    ]);
    expect(items[0]).toMatchObject({ kind: "approval", resolved: true });
  });
});

describe("run end", () => {
  const running = (): Item[] => fold([
    { type: "TEXT_MESSAGE_START", messageId: "m1", role: "assistant" },
    { type: "TEXT_MESSAGE_CONTENT", messageId: "m1", delta: "Sto" },
    { type: "REASONING_START", messageId: "r1" },
    { type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "shell" },
    { type: "SUBAGENT_STARTED", subagentId: "s1", name: "Lettore" },
    { type: "CUSTOM", name: "aiw.approval_request", value: { id: "a1", run_id: "r", tool: "shell", args: {}, reason: "x", risk: "high" } },
  ]);

  it("keeps the error code and message of RUN_ERROR", () => {
    const items = reduce([], { type: "RUN_ERROR", message: "Run annullato", code: "cancelled" });
    expect(items).toEqual([{ kind: "error", id: "err-0", message: "Run annullato", code: "cancelled" }]);
    expect(reduce([], { type: "RUN_ERROR", message: "boom" })[0]).toMatchObject({ kind: "error", message: "boom", code: undefined });
  });
  it("settles whatever was in flight when the run fails or is stopped", () => {
    const items = reduce(running(), { type: "RUN_ERROR", message: "Run annullato", code: "cancelled" });
    expect(items.find((i) => i.kind === "text")).toMatchObject({ streaming: false, text: "Sto" });
    expect(items.find((i) => i.kind === "reasoning")).toMatchObject({ streaming: false });
    expect(items.find((i) => i.kind === "tool")).toMatchObject({ status: "interrupted" });
    expect(items.find((i) => i.kind === "subagent")).toMatchObject({ interrupted: true });
    expect(items.find((i) => i.kind === "approval")).toMatchObject({ resolved: false });
    expect(items[items.length - 1]).toMatchObject({ kind: "error", code: "cancelled" });
  });
  it("settles on RUN_FINISHED too, and returns the same array when there is nothing to settle", () => {
    const done = reduce(running(), { type: "RUN_FINISHED", threadId: "t", runId: "r" });
    expect(done.find((i) => i.kind === "tool")).toMatchObject({ status: "interrupted" });
    expect(settle(done)).toBe(done);
  });
});

describe("structured answers", () => {
  it("shows the format check, live and from the stored history, as a note", () => {
    const live = fold([{ type: "CUSTOM", name: "aiw.format_check", value: { errors: ["$.totale: manca il campo obbligatorio"] } }]);
    expect(live[0]).toMatchObject({ kind: "note", text: expect.stringContaining("$.totale: manca il campo obbligatorio") });
    const stored = historyToItems([
      { id: "1", role: "user", name: "aiw.format_check", content: "[Controllo del formato] La risposta non rispetta lo schema richiesto:\n- $.totale: manca il campo obbligatorio\nRispondi di nuovo.", created_at: "2026-11-02T10:00:00Z" },
    ]);
    expect(stored).toEqual([{ kind: "note", id: "1", text: live[0].kind === "note" ? live[0].text : "" }]);
    expect(fold([{ type: "CUSTOM", name: "aiw.format_error", value: { errors: ["x"] } }])[0]).toMatchObject({ kind: "note" });
  });
});

describe("aiw.image", () => {
  const png = { toolCallId: "c1", mime: "image/png", data: "iVBORw0KGgo=" };
  const tool = (): Item[] => fold([{ type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "computer_screenshot" }]);

  it("attaches the screenshot (base64 without prefix) to the card of its tool call", () => {
    const items = fold([{ type: "CUSTOM", name: "aiw.image", value: png }], tool());
    expect(items[0]).toMatchObject({ kind: "tool", images: [{ mime: "image/png", data: "iVBORw0KGgo=" }] });
  });
  it("keeps several screenshots of one call and skips an identical one", () => {
    const items = fold([
      { type: "CUSTOM", name: "aiw.image", value: png },
      { type: "CUSTOM", name: "aiw.image", value: png },
      { type: "CUSTOM", name: "aiw.image", value: { ...png, mime: "image/jpeg", data: "/9j/4AAQ" } },
    ], tool());
    expect((items[0] as ToolItem).images).toHaveLength(2);
  });
  it("tolerates a data URL and drops malformed payloads", () => {
    expect(parseImageEvent({ ...png, data: `data:image/png;base64,${png.data}` })?.image.data).toBe(png.data);
    expect(parseImageEvent({ ...png, mime: "text/html" })).toBeNull();
    expect(parseImageEvent({ ...png, mime: "image/png;x" })).toBeNull();
    expect(parseImageEvent({ ...png, data: "" })).toBeNull();
    expect(parseImageEvent({ ...png, data: "<svg onload=alert(1)>" })).toBeNull();
    expect(parseImageEvent({ toolCallId: "c1", mime: "image/png" })).toBeNull();
    expect(parseImageEvent(null)).toBeNull();
    const a = tool();
    expect(reduce(a, { type: "CUSTOM", name: "aiw.image", value: { ...png, data: "" } })).toBe(a);
  });
  it("ignores an image for an unknown tool call", () => {
    const a = tool();
    expect(reduce(a, { type: "CUSTOM", name: "aiw.image", value: { ...png, toolCallId: "zz" } })).toBe(a);
  });
});

describe("historyToItems", () => {
  const at = "2026-11-02T10:00:00Z";
  it("pairs tool calls with their results", () => {
    const msgs: ChatMessage[] = [
      { id: "1", role: "user", content: "ciao", created_at: at },
      { id: "2", role: "assistant", content: "", tool_calls: [{ id: "c", name: "list_dir", arguments: "{}" }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c", content: "7 file", created_at: at },
      { id: "4", role: "assistant", content: "fatto", created_at: at },
    ];
    expect(historyToItems(msgs).map((i) => i.kind)).toEqual(["user", "tool", "text"]);
    expect(historyToItems(msgs)[1]).toMatchObject({ kind: "tool", status: "done", result: "7 file" });
  });
  it("shows a denied or failed call as such, and a call without answer as interrupted", () => {
    const msgs: ChatMessage[] = [
      { id: "2", role: "assistant", content: "provo", tool_calls: [{ id: "a", name: "write_file", arguments: "{}" }, { id: "b", name: "shell", arguments: "{}" }, { id: "c", name: "grep", arguments: "{}" }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "a", content: "Negato dall'utente.", created_at: at },
      { id: "4", role: "tool", tool_call_id: "b", content: "ERRORE: exit 1", created_at: at },
    ];
    expect((historyToItems(msgs).filter((i) => i.kind === "tool") as ToolItem[]).map((t) => t.status)).toEqual(["denied", "failed", "interrupted"]);
  });
  it("keeps the partial messages of a run that was stopped (text, then a denied tool)", () => {
    const msgs: ChatMessage[] = [
      { id: "1", role: "user", content: "Crea una nota", created_at: at },
      { id: "2", role: "assistant", content: "Scrivo la nota.", tool_calls: [{ id: "c1", name: "write_file", arguments: '{"path":"nota.md"}' }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c1", content: "Negato dall'utente.", created_at: at },
    ];
    expect(historyToItems(msgs).map((i) => i.kind)).toEqual(["user", "text", "tool"]);
  });
  it("puts the screenshots seen live back on their cards", () => {
    const msgs: ChatMessage[] = [
      { id: "2", role: "assistant", content: "", tool_calls: [{ id: "c", name: "browser_screenshot", arguments: "{}" }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c", content: "Screenshot allegato.", created_at: at },
    ];
    const items = historyToItems(msgs, { images: (id) => (id === "c" ? [{ mime: "image/png", data: "AAAA" }] : undefined) });
    expect(items[0]).toMatchObject({ kind: "tool", images: [{ mime: "image/png", data: "AAAA" }] });
  });
  it("turns a render_ui call into a surface", () => {
    const args = JSON.stringify({ surfaceId: "s1", components: [{ id: "root", component: "Text", text: "ciao" }] });
    const items = historyToItems([
      { id: "2", role: "assistant", content: "", tool_calls: [{ id: "c", name: "render_ui", arguments: args }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c", content: "ok", created_at: at },
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "surface", id: "s1" });
  });
  describe("run errors (system message aiw.run_error)", () => {
    const run = (extra: ChatMessage[]): ChatMessage[] => [{ id: "1", role: "user", content: "ciao", created_at: at }, ...extra];
    const err = (content: string, id = "e1"): ChatMessage => ({ id, role: "system", name: "aiw.run_error", content, created_at: at });
    it("shows the stored error as the same error row the live RUN_ERROR makes, with the code read back from the wording", () => {
      const cases: [string, string | undefined][] = [
        ["Errore del runtime: il runtime si è chiuso", "runtime"],
        ["Runtime non disponibile: connessione rifiutata", "runtime"],
        ["Prompt bloccato da un hook: parola vietata", "hook_blocked"],
        ["Run annullato", "cancelled"],
        ["qualcosa di nuovo", undefined],
      ];
      for (const [content, code] of cases) {
        expect(historyToItems(run([err(content)])).at(-1), content).toMatchObject({ kind: "error", id: "e1", message: content, code, stored: true });
      }
    });
    it("keeps the error where it happened, after what the run had produced", () => {
      const items = historyToItems(run([
        { id: "2", role: "assistant", content: "provo", created_at: at },
        err("Errore del runtime: boom"),
      ]));
      expect(items.map((i) => i.kind)).toEqual(["user", "text", "error"]);
    });
    it("skips a system message that is not a run error", () => {
      expect(historyToItems(run([{ id: "s", role: "system", content: "Sei un assistente", created_at: at }])).map((i) => i.kind)).toEqual(["user"]);
      expect(historyToItems(run([{ id: "s", role: "system", name: "altro", content: "x", created_at: at }])).map((i) => i.kind)).toEqual(["user"]);
    });
    it("leaves the note of the last run out, so the same error is not told twice", () => {
      const items = historyToItems(run([err("Errore del runtime: boom")]));
      expect(lastRunNotice("failed", items, false)).toBeNull();
      // Without the stored error (an older backend) the note still explains how the run ended.
      expect(lastRunNotice("failed", historyToItems(run([])), false)).not.toBeNull();
    });
  });
  it("keeps the card of a render_ui call that failed", () => {
    const items = historyToItems([
      { id: "2", role: "assistant", content: "", tool_calls: [{ id: "c", name: "render_ui", arguments: "{}" }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c", content: "ERRORE: componenti mancanti", created_at: at },
    ]);
    expect(items[0]).toMatchObject({ kind: "tool", status: "failed" });
  });
});

describe("A2UI JSON Pointer", () => {
  it("reads and writes nested paths immutably", () => {
    const data = { a: { b: [1, 2] } };
    expect(getPath(data, "/a/b/1")).toBe(2);
    const next = setPath(data, "/a/b/1", 9);
    expect(getPath(next, "/a/b/1")).toBe(9);
    expect(getPath(data, "/a/b/1")).toBe(2);
    expect(getPath(setPath({}, "/x/y", "z"), "/x/y")).toBe("z");
  });
});
