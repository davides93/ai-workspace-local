import type { CSSProperties, ReactNode } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { cx } from "../lib/cx";
import { errorMessage } from "../lib/errors";
import { Button } from "./Button";
import { Icon } from "./Icon";

import { t, t as tr } from "../i18n";
/** Nothing to show yet: say why and offer the next step. */
export function EmptyState({ icon, title, description, action, className, headingLevel = 3 }: {
  icon?: string;
  title: string;
  /** `2` when the state sits straight under the page title (no section heading in between), so no level is skipped. */
  headingLevel?: 2 | 3;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("empty", className)}>
      {icon && <Icon name={icon} size={22} />}
      {headingLevel === 2 ? <h2>{title}</h2> : <h3>{title}</h3>}
      {description && <p className="muted">{description}</p>}
      {action && <div className="row">{action}</div>}
    </div>
  );
}

/** A request failed: show the reason and a retry button. */
export function ErrorState({ error, onRetry, title = tr("ui.qualcosa_non_ha_funzionato"), retrying }: {
  error: unknown;
  onRetry?: () => void;
  title?: string;
  retrying?: boolean;
}) {
  return (
    <div className="empty error-state" role="alert">
      <Icon name="alert" size={22} />
      <h3>{title}</h3>
      <p className="muted">{errorMessage(error)}</p>
      {onRetry && <Button size="sm" icon="refresh" onClick={onRetry} disabled={retrying}>{t("ui.riprova")}</Button>}
    </div>
  );
}

/** Placeholder block shown while data loads. Sizes are layout values, not styling. */
export function Skeleton({ width, height = 14, className }: { width?: number | string; height?: number | string; className?: string }) {
  const style: CSSProperties = { width, height };
  return <span className={cx("skeleton", className)} style={style} aria-hidden="true" />;
}

/** Several skeleton lines of varying width. */
export function SkeletonLines({ lines = 3 }: { lines?: number }) {
  return (
    <div className="skeleton-lines" role="status" aria-label={t("ui.caricamento")}>
      {Array.from({ length: lines }, (_, i) => <Skeleton key={i} width={`${92 - ((i * 17) % 40)}%`} />)}
    </div>
  );
}

/** Full page placeholder (route suspense fallback). */
export function PageSkeleton() {
  return (
    <div className="page" role="status" aria-label={t("ui.caricamento")}>
      <div className="stack">
        <Skeleton width={90} height={11} />
        <Skeleton width="min(320px, 60%)" height={28} />
        <Skeleton width="min(520px, 90%)" />
      </div>
      <div className="skeleton-grid">
        {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} height={96} className="skeleton-card" />)}
      </div>
    </div>
  );
}

/**
 * Uniform loading / error / empty handling of a query:
 * skeleton on the first load, error state with "Riprova", empty state when `isEmpty` says so.
 */
export function QueryState<T>({ query, children, isEmpty, empty, skeleton }: {
  query: UseQueryResult<T>;
  children: (data: T) => ReactNode;
  isEmpty?: (data: T) => boolean;
  empty?: ReactNode;
  skeleton?: ReactNode;
}) {
  if (query.isPending) return <>{skeleton ?? <SkeletonLines lines={4} />}</>;
  if (query.isError && query.data === undefined) {
    return <ErrorState error={query.error} onRetry={() => void query.refetch()} retrying={query.isFetching} />;
  }
  const data = query.data as T;
  if (isEmpty?.(data) && empty) return <>{empty}</>;
  return <>{children(data)}</>;
}
