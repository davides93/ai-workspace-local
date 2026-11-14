import { useEffect, useState, type RefObject } from "react";

/** Is the focus parked where the person is not working on something else: nowhere, in the message box, or in the conversation? */
export function focusIsParked(active: Element | null): boolean {
  if (!active || active === document.body || active === document.documentElement) return true;
  return active.id === "composer" || !!active.closest('[role="log"]');
}

/**
 * Moves the focus to the first thing to answer (`[data-ask-first]`) when the card appears, but only if the person was in the
 * message box or the log (or nowhere): a card must not take the focus away from a field the person is typing in.
 */
export function useFocusOnOpen(root: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    if (focusIsParked(document.activeElement)) el.querySelector<HTMLElement>("[data-ask-first]")?.focus({ preventScroll: true });
    el.scrollIntoView?.({ block: "nearest" });
  }, [root]);
}

/**
 * Text for a polite live region. A region that exists with its text already in it is not always read, so the text goes in a
 * moment after the card is on screen.
 */
export function useAnnounce(text: string): string {
  const [said, setSaid] = useState("");
  useEffect(() => {
    const id = setTimeout(() => setSaid(text), 120);
    return () => clearTimeout(id);
  }, [text]);
  return said;
}
