import type { ReactNode } from "react";
import { cx } from "../lib/cx";

export type PillTone = "ok" | "run" | "warn" | "crit" | "info" | "neutral";

/** Rounded status label. */
export function Pill({ tone = "neutral", children, className, title }: { tone?: PillTone; children: ReactNode; className?: string; title?: string }) {
  return <span className={cx("pill", tone, className)} title={title}>{children}</span>;
}

export type LedTone = "ok" | "run" | "warn" | "crit" | "off";

/** Status LED: color and pulse say the state before any text does. */
export function Led({ tone = "off", small = false, className }: { tone?: LedTone; small?: boolean; className?: string }) {
  return <span className={cx("led", tone, small && "sm", className)} aria-hidden="true" />;
}

/** Monospace tag (formats, quantizations, accelerators); `hot` highlights it. */
export function Chip({ children, hot, title, className }: { children: ReactNode; hot?: boolean; title?: string; className?: string }) {
  return <span className={cx("chip", hot && "hot", className)} title={title}>{children}</span>;
}

export function Chips({ children, label }: { children: ReactNode; label?: string }) {
  return <div className="chips" aria-label={label}>{children}</div>;
}
