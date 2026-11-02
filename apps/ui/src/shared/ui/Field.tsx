import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";
import { cx } from "../lib/cx";

/** Label + control + hint/error. `htmlFor` must match the control id. */
export function Field({ label, htmlFor, hint, error, children, className }: {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  error?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("field", className)}>
      <label htmlFor={htmlFor}>{label}</label>
      {children}
      {hint && <span className="hint">{hint}</span>}
      {error && <span className="field-error" role="alert">{error}</span>}
    </div>
  );
}

export function Input({ className, mono, ...rest }: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return <input className={cx("input", mono && "mono", className)} {...rest} />;
}

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={cx("input", className)} {...rest}>{children}</select>;
}

export function Textarea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={cx("input", className)} {...rest} />;
}
