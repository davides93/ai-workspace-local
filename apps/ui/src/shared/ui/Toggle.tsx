import { cx } from "../lib/cx";

/** On/off switch (`role="switch"`). Give it a visible label via `label` or an `aria-label`. */
export function Toggle({ checked, onChange, label, disabled, id, className }: {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** Accessible name; use it when no visible label is associated through `aria-labelledby`. */
  label?: string;
  disabled?: boolean;
  id?: string;
  className?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cx("switch", className)}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
    >
      <span className="switch-thumb" aria-hidden="true" />
    </button>
  );
}
