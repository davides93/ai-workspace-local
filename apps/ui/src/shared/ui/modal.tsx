import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Open modals, innermost last: only the top one reacts to Esc and Tab. */
const stack: symbol[] = [];

function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => !el.hasAttribute("hidden") && el.getClientRects().length > 0);
}

/**
 * Modal behaviour shared by Dialog, Drawer, Sheet and the command palette:
 * focus moves inside on open (first `[data-autofocus]`, else the first control, else the
 * container), Tab is trapped, Esc closes, and focus returns to the opener on close.
 */
export function useModal(ref: RefObject<HTMLElement | null>, onClose: () => void): void {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const token = Symbol("modal");
    stack.push(token);
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = el.querySelector<HTMLElement>("[data-autofocus]") ?? focusables(el)[0] ?? el;
    first.focus({ preventScroll: true });

    const onKey = (e: KeyboardEvent) => {
      if (stack[stack.length - 1] !== token) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        closeRef.current();
        return;
      }
      if (e.key !== "Tab") return;
      const items = focusables(el);
      if (items.length === 0) {
        e.preventDefault();
        el.focus();
        return;
      }
      const firstEl = items[0];
      const lastEl = items[items.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === firstEl || active === el)) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && active === lastEl) {
        e.preventDefault();
        firstEl.focus();
      } else if (!el.contains(active)) {
        e.preventDefault();
        firstEl.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      const i = stack.indexOf(token);
      if (i >= 0) stack.splice(i, 1);
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, [ref]);
}

/** Backdrop + positioned surface rendered in a portal; the base of every overlay. */
export function ModalShell({ variant, title, onClose, children, className }: {
  variant: "dialog" | "drawer" | "sheet" | "palette";
  /** Accessible name of the dialog. */
  title: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useModal(ref, onClose);
  return createPortal(
    <div className={`overlay overlay-${variant}`}>
      <div className="overlay-backdrop" onClick={onClose} aria-hidden="true" />
      <div
        ref={ref}
        className={`overlay-surface ${variant}${className ? ` ${className}` : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
