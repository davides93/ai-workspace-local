import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { isNearBottom } from "../model/scroll";

/**
 * Keeps a conversation scrolled to its end, but only for a reader who is at the end. Someone who
 * scrolled up to re-read is left alone and offered a "Nuovi messaggi" pill instead.
 *
 * Following uses a ResizeObserver on the content, so streaming text, late-loading images and a tool card that
 * opens are all covered, not just changes of the item list. The pill does not: it appears when `tail` (what the end of the
 * conversation looks like, `tailSignature`) changes while the reader is elsewhere. The height of the list also changes while
 * someone scrolls through a long conversation (rows far from the screen are laid out when they come near), and that is not news.
 */
export function useStickToBottom(scrollRef: RefObject<HTMLElement | null>, contentRef: RefObject<HTMLElement | null>, resetKey: unknown, tail: string) {
  const stick = useRef(true);
  const [unseen, setUnseen] = useState(false);

  // A different conversation starts at its end.
  useEffect(() => {
    stick.current = true;
    setUnseen(false);
  }, [resetKey]);

  // Something new at the end while the reader is elsewhere.
  const lastTail = useRef(tail);
  useEffect(() => {
    if (lastTail.current === tail) return;
    lastTail.current = tail;
    if (!stick.current) setUnseen(true);
  }, [tail]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onScroll = () => {
      stick.current = isNearBottom(el);
      if (stick.current) setUnseen(false);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [scrollRef]);

  useEffect(() => {
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el || !content || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      if (stick.current) el.scrollTop = el.scrollHeight;
    });
    ro.observe(content);
    return () => ro.disconnect();
  }, [scrollRef, contentRef]);

  /**
   * Jumps to the end and follows again (after the user sends a message, or taps the pill). It jumps
   * instead of animating: the scroll events of an animation would look like the reader leaving the end.
   */
  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    stick.current = true;
    setUnseen(false);
    el.scrollTop = el.scrollHeight;
  }, [scrollRef]);

  return { unseen, scrollToBottom };
}
