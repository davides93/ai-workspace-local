import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import "./FormSection.css";

/** Stack of titled sections of a long form, separated by a rule. */
export function FormSections({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("form-sections", className)}>{children}</div>;
}

/** One titled part of a form: a heading, a one-line hint and the fields. */
export function FormSection({ id, title, hint, actions, children }: { id: string; title: string; hint?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="form-section" aria-labelledby={`fs-${id}`}>
      <div className="form-section-head">
        <h3 id={`fs-${id}`}>{title}</h3>
        {hint && <span className="muted text-sm">{hint}</span>}
        {actions && <div className="form-section-actions">{actions}</div>}
      </div>
      {children}
    </section>
  );
}
