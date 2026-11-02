import { cx } from "../lib/cx";

/** Horizontal gauge. The width is the only inline style: it is a computed value. */
export function Meter({ value, max, tone, label }: { value: number; max: number; tone?: "ok" | "warn" | "crit"; label?: string }) {
  const pct = max > 0 ? Math.max(0, Math.min(100, (value / max) * 100)) : 0;
  return (
    <div className={cx("meter", tone)} role="meter" aria-label={label} aria-valuenow={value} aria-valuemin={0} aria-valuemax={max}>
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}
