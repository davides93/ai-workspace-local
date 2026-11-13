import { useId } from "react";
import { IconButton, Select } from "../../../shared/ui";
import type { Page } from "../model/filter";

import { t } from "../../../shared/i18n";
export const PAGE_SIZES = [25, 50, 100] as const;

/** "Righe 1–50 di 312", previous/next page and the page size. */
export function Pager({ page, size, onPage, onSize }: { page: Page<unknown>; size: number; onPage: (p: number) => void; onSize: (n: number) => void }) {
  const id = useId();
  return (
    <nav className="act-pager" aria-label={t("activity.pagine_dell_attivita")}>
      <span className="text-sm num" aria-live="polite">
        {page.total === 0 ? t("activity.nessuna_riga") : <>Righe <b>{page.from}–{page.to}</b> di <b>{page.total}</b></>}
      </span>
      <div className="row nowrap">
        <label htmlFor={id} className="text-sm faint">{t("activity.per_pagina")}</label>
        <Select id={id} className="act-size" value={size} onChange={(e) => onSize(Number(e.target.value))}>
          {PAGE_SIZES.map((n) => <option key={n} value={n}>{n}</option>)}
        </Select>
        <IconButton aria-label={t("activity.pagina_precedente")} icon="back" disabled={page.page <= 1} onClick={() => onPage(page.page - 1)} />
        <span className="text-sm num" aria-label={t("activity.pagina_di", { page: page.page, pages: page.pages })}>{page.page} / {page.pages}</span>
        <IconButton aria-label={t("activity.pagina_successiva")} icon="forward" disabled={page.page >= page.pages} onClick={() => onPage(page.page + 1)} />
      </div>
    </nav>
  );
}
