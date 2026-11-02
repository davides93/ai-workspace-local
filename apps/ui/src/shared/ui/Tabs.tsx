import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../lib/cx";

export interface TabItem<T extends string> {
  id: T;
  label: string;
  /** Small counter next to the label. */
  count?: number;
}

/** DOM id of a tab button and of its panel, derived from the prefix so they can reference each other. */
export const tabId = (prefix: string, id: string) => `${prefix}-tab-${id}`;
export const panelId = (prefix: string, id: string) => `${prefix}-panel-${id}`;

/** Tab list with roving tabindex: arrows, Home and End move between tabs. */
export function Tabs<T extends string>({ tabs, value, onChange, label, idPrefix, className }: {
  tabs: TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  idPrefix: string;
  className?: string;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const move = (e: KeyboardEvent, index: number) => {
    let next = index;
    if (e.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else return;
    e.preventDefault();
    onChange(tabs[next].id);
    listRef.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  };
  return (
    <div className={cx("tabs", className)} role="tablist" aria-label={label} ref={listRef}>
      {tabs.map((t, i) => {
        const selected = t.id === value;
        return (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={tabId(idPrefix, t.id)}
            aria-selected={selected}
            aria-controls={panelId(idPrefix, t.id)}
            tabIndex={selected ? 0 : -1}
            className="tab"
            onClick={() => onChange(t.id)}
            onKeyDown={(e) => move(e, i)}
          >
            {t.label}
            {t.count !== undefined && <span className="tab-count">{t.count}</span>}
          </button>
        );
      })}
    </div>
  );
}

export function TabPanel({ idPrefix, id, children }: { idPrefix: string; id: string; children: ReactNode }) {
  return (
    <div role="tabpanel" id={panelId(idPrefix, id)} aria-labelledby={tabId(idPrefix, id)} tabIndex={0} className="tabpanel">
      {children}
    </div>
  );
}
