import { useId } from "react";
import { IconButton, Select } from "../../../shared/ui";
import type { Page } from "../model/filter";

export const PAGE_SIZES = [25, 50, 100] as const;

/** "Righe 1–50 di 312", previous/next page and the page size. */
export function Pager({ page, size, onPage, onSize }: { page: Page<unknown>; size: number; onPage: (p: number) => void; onSize: (n: number) => void }) {
  const id = useId();
  return (
    <nav className="act-pager" aria-label="Pagine dell'attività">
      <span className="text-sm num" aria-live="polite">
        {page.total === 0 ? "Nessuna riga" : <>Righe <b>{page.from}–{page.to}</b> di <b>{page.total}</b></>}
      </span>
      <div className="row nowrap">
        <label htmlFor={id} className="text-sm faint">Per pagina</label>
        <Select id={id} className="act-size" value={size} onChange={(e) => onSize(Number(e.target.value))}>
          {PAGE_SIZES.map((n) => <option key={n} value={n}>{n}</option>)}
        </Select>
        <IconButton aria-label="Pagina precedente" icon="back" disabled={page.page <= 1} onClick={() => onPage(page.page - 1)} />
        <span className="text-sm num" aria-label={`Pagina ${page.page} di ${page.pages}`}>{page.page} / {page.pages}</span>
        <IconButton aria-label="Pagina successiva" icon="forward" disabled={page.page >= page.pages} onClick={() => onPage(page.page + 1)} />
      </div>
    </nav>
  );
}
