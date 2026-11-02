import { describe, expect, it } from "vitest";
import type { AgEvent, ChatMessage } from "../../../shared/api/types";
import { getPath, setPath } from "../a2ui/protocol";
import { historyToItems } from "./history";
import type { Item } from "./items";
import { reduce } from "./reduce";

const fold = (events: AgEvent[]): Item[] => events.reduce<Item[]>((items, e) => reduce(items, e), []);

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
  it("does not mutate the previous array", () => {
    const a = reduce([], { type: "TEXT_MESSAGE_START", messageId: "m", role: "assistant" });
    const b = reduce(a, { type: "TEXT_MESSAGE_CONTENT", messageId: "m", delta: "x" });
    expect(a).not.toBe(b);
    expect((a[0] as Extract<Item, { kind: "text" }>).text).toBe("");
  });
  it("builds a tool card from start, args and result, and flags denials", () => {
    const items = fold([
      { type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "shell" },
      { type: "TOOL_CALL_ARGS", toolCallId: "c1", delta: '{"command":' },
      { type: "TOOL_CALL_ARGS", toolCallId: "c1", delta: '"ls"}' },
      { type: "TOOL_CALL_RESULT", messageId: "x", toolCallId: "c1", content: "Negato dall'utente." },
    ]);
    expect(items[0]).toMatchObject({ kind: "tool", name: "shell", args: '{"command":"ls"}', status: "denied" });
  });
  it("attaches aiw.image screenshots to the tool card", () => {
    const items = fold([
      { type: "TOOL_CALL_START", toolCallId: "c1", toolCallName: "computer_screenshot" },
      { type: "CUSTOM", name: "aiw.image", value: { toolCallId: "c1", mime: "image/png", data: "AAAA" } },
    ]);
    expect(items[0]).toMatchObject({ kind: "tool", images: [{ mime: "image/png", data: "AAAA" }] });
  });
  it("tracks approvals and errors", () => {
    const req = { id: "a1", run_id: "r", tool: "shell", args: {}, reason: "x", risk: "medium" as const };
    const items = fold([
      { type: "CUSTOM", name: "aiw.approval_request", value: req },
      { type: "CUSTOM", name: "aiw.approval_resolved", value: { id: "a1", allowed: true } },
      { type: "RUN_ERROR", message: "boom" },
    ]);
    expect(items[0]).toMatchObject({ kind: "approval", resolved: true });
    expect(items[1]).toEqual({ kind: "error", message: "boom" });
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
  it("turns a render_ui call into a surface", () => {
    const args = JSON.stringify({ surfaceId: "s1", components: [{ id: "root", component: "Text", text: "ciao" }] });
    const items = historyToItems([
      { id: "2", role: "assistant", content: "", tool_calls: [{ id: "c", name: "render_ui", arguments: args }], created_at: at },
      { id: "3", role: "tool", tool_call_id: "c", content: "ok", created_at: at },
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "surface", id: "s1" });
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
