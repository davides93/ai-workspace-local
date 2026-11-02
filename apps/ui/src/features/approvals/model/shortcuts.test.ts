import { describe, expect, it } from "vitest";
import { approvalShortcut, isTypingTarget, moveSelection, pendingLabel } from "./shortcuts";

const key = (k: string, extra: Partial<Parameters<typeof approvalShortcut>[0]> = {}) => ({ key: k, ctrlKey: false, metaKey: false, altKey: false, ...extra });

describe("approval shortcuts", () => {
  it("maps A to allow and N to deny, in either case", () => {
    expect(approvalShortcut(key("a"), { count: 1, typing: false })).toBe("allow");
    expect(approvalShortcut(key("A"), { count: 1, typing: false })).toBe("allow");
    expect(approvalShortcut(key("n"), { count: 1, typing: false })).toBe("deny");
    expect(approvalShortcut(key("N"), { count: 2, typing: false })).toBe("deny");
  });
  it("ignores keys while typing, with modifiers, or already handled", () => {
    expect(approvalShortcut(key("a"), { count: 1, typing: true })).toBeNull();
    expect(approvalShortcut(key("a", { ctrlKey: true }), { count: 1, typing: false })).toBeNull();
    expect(approvalShortcut(key("n", { metaKey: true }), { count: 1, typing: false })).toBeNull();
    expect(approvalShortcut(key("a", { defaultPrevented: true }), { count: 1, typing: false })).toBeNull();
    expect(approvalShortcut(key("x"), { count: 1, typing: false })).toBeNull();
  });
  it("uses the arrows only when there is something to move to", () => {
    expect(approvalShortcut(key("ArrowDown"), { count: 1, typing: false })).toBeNull();
    expect(approvalShortcut(key("ArrowDown"), { count: 3, typing: false })).toBe("next");
    expect(approvalShortcut(key("ArrowUp"), { count: 3, typing: false })).toBe("prev");
  });
  it("wraps the selection", () => {
    expect(moveSelection(0, 3, "prev")).toBe(2);
    expect(moveSelection(2, 3, "next")).toBe(0);
    expect(moveSelection(1, 3, "next")).toBe(2);
    expect(moveSelection(0, 0, "next")).toBe(0);
  });
  it("recognizes fields where letters are text", () => {
    expect(isTypingTarget({ tagName: "INPUT" } as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget({ tagName: "TEXTAREA" } as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget({ tagName: "DIV", isContentEditable: true } as unknown as EventTarget)).toBe(true);
    expect(isTypingTarget({ tagName: "BUTTON" } as unknown as EventTarget)).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
  it("words the pending count in Italian", () => {
    expect(pendingLabel(1)).toBe("1 conferma in attesa");
    expect(pendingLabel(4)).toBe("4 conferme in attesa");
  });
});
