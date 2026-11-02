// Keyboard shortcuts of the confirmation drawer: A allows, N denies, ↑/↓ move between requests.

export type ApprovalCommand = "allow" | "deny" | "prev" | "next";

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey?: boolean;
  defaultPrevented?: boolean;
}

/** The command a key press stands for, or null (typing in a field, a modifier held, one request only for the arrows). */
export function approvalShortcut(e: KeyLike, opts: { count: number; typing: boolean }): ApprovalCommand | null {
  if (opts.typing || e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return null;
  switch (e.key) {
    case "a":
    case "A":
      return "allow";
    case "n":
    case "N":
      return "deny";
    case "ArrowUp":
      return opts.count > 1 ? "prev" : null;
    case "ArrowDown":
      return opts.count > 1 ? "next" : null;
    default:
      return null;
  }
}

/** Selection after an arrow key (wraps around). */
export function moveSelection(index: number, count: number, cmd: "prev" | "next"): number {
  if (count <= 0) return 0;
  return cmd === "next" ? (index + 1) % count : (index - 1 + count) % count;
}

/** Is the event target a field where letters are text, not shortcuts? */
export function isTypingTarget(t: EventTarget | null): boolean {
  const el = t as { tagName?: string; isContentEditable?: boolean } | null;
  if (!el?.tagName) return false;
  const tag = el.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || el.isContentEditable === true;
}

/** "1 conferma in attesa" / "3 conferme in attesa". */
export const pendingLabel = (n: number): string => (n === 1 ? "1 conferma in attesa" : `${n} conferme in attesa`);
