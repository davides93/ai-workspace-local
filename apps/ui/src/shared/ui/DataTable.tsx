import type { ReactNode } from "react";
import { cx } from "../lib/cx";

import { t } from "../i18n";
export interface Column<T> {
  key: string;
  header: ReactNode;
  render: (row: T) => ReactNode;
  /** Applied to the cells of this column (e.g. `num`, `mono`). */
  className?: string;
  nowrap?: boolean;
}

/** Data table inside a horizontally scrollable wrapper (it never widens the page on phones). */
export function DataTable<T>({ columns, rows, rowKey, onRowClick, empty, label, className }: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  onRowClick?: (row: T) => void;
  /** Shown in place of the body when there are no rows. */
  empty?: ReactNode;
  /** Accessible name of the table. */
  label: string;
  className?: string;
}) {
  return (
    <div className={cx("table-wrap", className)}>
      <table className="data" aria-label={label}>
        <thead>
          <tr>
            {columns.map((c) => <th key={c.key} scope="col">{c.header}</th>)}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={rowKey(row)}
              className={onRowClick ? "clickable" : undefined}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
            >
              {columns.map((c) => <td key={c.key} className={cx(c.className, c.nowrap && "nowrap")}>{c.render(row)}</td>)}
            </tr>
          ))}
          {rows.length === 0 && (
            <tr>
              <td colSpan={columns.length} className="table-empty">{empty ?? t("ui.nessun_dato")}</td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
