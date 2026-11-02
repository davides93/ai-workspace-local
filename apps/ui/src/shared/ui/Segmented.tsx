import { cx } from "../lib/cx";

export interface SegmentedOption<T extends string> {
  id: T;
  label: string;
  title?: string;
}

/** Compact single-choice control (theme, filters, views). Not a tab list: it does not own a panel. */
export function Segmented<T extends string>({ options, value, onChange, label, className }: {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  className?: string;
}) {
  return (
    <div className={cx("seg", className)} role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.id} type="button" aria-pressed={value === o.id} title={o.title} onClick={() => onChange(o.id)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
