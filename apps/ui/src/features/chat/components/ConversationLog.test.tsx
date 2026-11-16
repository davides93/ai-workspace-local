import { describe, expect, it, vi } from "vitest";
import type { Item } from "../model/items";
import { renderIn } from "../../../test-utils/render";
import { ConversationLog } from "./ConversationLog";

// Which replies are marked cold (chat.css skips their layout) depends on the conversation as it was when it opened, not on how long
// it is now: a row that turned cold while the reader was at the end would change height above the screen and the view would
// stop following the end.
const items = (n: number, from = 0): Item[] => Array.from({ length: n }, (_, i): Item => ({ kind: "text", id: `a${from + i}`, text: `risposta ${from + i}`, streaming: false }));
const props = { state: { status: "ready" as const }, onRetry: vi.fn(), busy: false, onRespond: vi.fn(), onAction: vi.fn(), notice: null, empty: <p>vuota</p> };
const cold = (c: HTMLElement) => [...c.querySelectorAll(".ws-bubble")].map((b) => b.hasAttribute("data-cold"));

describe("ConversationLog: rows far from the end", () => {
  it("marks the replies before the last 40 of an opened conversation, and none of a short one", () => {
    const long = renderIn(<ConversationLog {...props} items={items(100)} sessionKey="s1" />);
    const flags = cold(long.container);
    expect(flags).toHaveLength(100);
    expect(flags.slice(0, 60).every(Boolean)).toBe(true);
    expect(flags.slice(60).some(Boolean)).toBe(false);
    long.unmount();
    const short = renderIn(<ConversationLog {...props} items={items(40)} sessionKey="s2" />);
    expect(cold(short.container).some(Boolean)).toBe(false);
  });

  it("does not move the boundary when rows arrive: the new ones, and the 40 before them, stay as they are", () => {
    const { container, rerender } = renderIn(<ConversationLog {...props} items={items(100)} sessionKey="s1" />);
    const before = cold(container);
    rerender(<ConversationLog {...props} items={items(130)} sessionKey="s1" />);
    const after = cold(container);
    expect(after).toHaveLength(130);
    expect(after.slice(0, 100)).toEqual(before);
    expect(after.slice(100).some(Boolean)).toBe(false);
  });

  it("a conversation that opens with nothing and grows has no cold rows however long it gets", () => {
    const { container, rerender } = renderIn(<ConversationLog {...props} items={[]} sessionKey="s1" />);
    rerender(<ConversationLog {...props} items={items(200)} sessionKey="s1" />);
    expect(cold(container).some(Boolean)).toBe(false);
  });

  it("another conversation takes its own boundary, and a conversation that is still loading marks nothing", () => {
    const { container, rerender } = renderIn(<ConversationLog {...props} items={items(100)} sessionKey="s1" />);
    rerender(<ConversationLog {...props} items={items(60)} sessionKey="s2" />);
    expect(cold(container).filter(Boolean)).toHaveLength(20);
    rerender(<ConversationLog {...props} state={{ status: "loading" }} items={[]} sessionKey="s3" />);
    expect(container.querySelectorAll(".ws-bubble")).toHaveLength(0);
    rerender(<ConversationLog {...props} items={items(50)} sessionKey="s3" />);
    expect(cold(container).filter(Boolean)).toHaveLength(10);
  });

  it("the log keeps its name and its keyboard stop whatever the length", () => {
    const { getByRole } = renderIn(<ConversationLog {...props} items={items(100)} sessionKey="s1" />);
    const log = getByRole("log", { name: "Conversazione" });
    expect(log).toHaveAttribute("tabindex", "0");
    expect(log).toHaveAttribute("aria-live", "off");
  });
});
