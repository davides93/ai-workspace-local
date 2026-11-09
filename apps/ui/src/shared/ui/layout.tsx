import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";

/** Page title block: eyebrow (group), title, subtitle and the page actions. */
export function PageHeader({ eyebrow, title, subtitle, actions, back }: {
  eyebrow?: string;
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
  /** Phone "back to the list" link used by detail pages. */
  back?: ReactNode;
}) {
  return (
    <header className="page-head">
      <div className="titles">
        {back}
        {eyebrow && <span className="eyebrow">{eyebrow}</span>}
        <h1>{title}</h1>
        {subtitle && <span className="muted">{subtitle}</span>}
      </div>
      {actions && <div className="actions">{actions}</div>}
    </header>
  );
}

/** Scrollable page body of a route. */
export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("page", className)}>{children}</div>;
}

/** Bordered surface with an optional header row. */
export function Panel({ title, icon, actions, children, padded = true, className }: {
  title?: ReactNode;
  icon?: string;
  actions?: ReactNode;
  children: ReactNode;
  padded?: boolean;
  className?: string;
}) {
  return (
    <div className={cx("panel", className)}>
      {(title || actions) && (
        <div className="panel-head">
          {icon && <Icon name={icon} />}
          <h2 className="grow">{title}</h2>
          {actions}
        </div>
      )}
      {padded ? <div className="panel-body">{children}</div> : children}
    </div>
  );
}

/** Titled group of content inside a page. */
export function Section({ title, hint, actions, children, className }: {
  title: string;
  hint?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cx("section", className)}>
      <div className="section-head">
        <h2>{title}</h2>
        {hint && <span className="muted">{hint}</span>}
        {actions && <div className="section-actions">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

/**
 * List and detail side by side. On phones (≤ 700 px) it becomes a single column: the list while
 * nothing is selected, the detail as its own page once an item is open (`detailOpen`), with a
 * back link rendered by the caller through `PageHeader back` or `BackLink`.
 */
export function MasterDetail({ list, detail, detailOpen, ratio = "narrow", className }: {
  list: ReactNode;
  detail: ReactNode;
  /** True when the URL names a selected item. */
  detailOpen: boolean;
  ratio?: "narrow" | "wide";
  className?: string;
}) {
  return (
    <div className={cx("master-detail", `md-${ratio}`, className)} data-detail={detailOpen ? "open" : "closed"}>
      <div className="md-list">{list}</div>
      <div className="md-detail">{detail}</div>
    </div>
  );
}

/** Key/value list (`dl`). Pass `[label, value]` pairs; falsy values are skipped. */
export function KeyValue({ items, className }: { items: ([string, ReactNode] | false | null | undefined)[]; className?: string }) {
  return (
    <dl className={cx("kv", className)}>
      {items.map((it) =>
        it ? (
          <div className="kv-row" key={it[0]}>
            <dt>{it[0]}</dt>
            <dd>{it[1]}</dd>
          </div>
        ) : null,
      )}
    </dl>
  );
}
