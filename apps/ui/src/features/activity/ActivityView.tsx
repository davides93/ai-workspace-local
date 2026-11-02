import { useMemo, useState } from "react";
import { downloadFile } from "../../shared/lib/download";
import { Button, EmptyState, Icon, Page, PageHeader, QueryState } from "../../shared/ui";
import { useAudit } from "./api";
import { AuditFilterBar } from "./components/AuditFilterBar";
import { AuditTable } from "./components/AuditTable";
import { Pager } from "./components/Pager";
import { auditExport, BACKEND_LIMIT, emptyFilters, facets, filterAudit, isFiltering, newestFirst, paginate, type AuditFilters } from "./model/filter";
import "./activity.css";

/** Attività: the audit trail with filters, search, pagination and a JSON export of what is on screen. */
export function ActivityView() {
  const audit = useAudit();
  const [filters, setFilters] = useState<AuditFilters>(emptyFilters);
  const [page, setPage] = useState(1);
  const [size, setSize] = useState(50);

  const all = useMemo(() => newestFirst(audit.data ?? []), [audit.data]);
  const menus = useMemo(() => facets(all), [all]);
  const filtered = useMemo(() => filterAudit(all, filters), [all, filters]);
  const shown = paginate(filtered, page, size);
  const truncated = all.length >= BACKEND_LIMIT;

  const change = (next: AuditFilters) => {
    setFilters(next);
    setPage(1);
  };
  const exportJson = () => {
    const { filename, content } = auditExport(filtered, filters);
    downloadFile(filename, content);
  };

  return (
    <Page className="act">
      <PageHeader
        eyebrow="Controllo" title="Attività"
        subtitle="Audit trail di ogni decisione di policy, azione dei tool e comando di sistema."
        actions={<Button icon="download" disabled={filtered.length === 0} onClick={exportJson} title="Scarica le righe filtrate in un file JSON">Esporta JSON{filtered.length > 0 ? ` (${filtered.length})` : ""}</Button>}
      />
      <p className={truncated ? "act-note is-warn" : "act-note"} role={truncated ? "status" : undefined}>
        <Icon name="info" size={15} />
        <span>
          Il workspace restituisce soltanto le <b>ultime {BACKEND_LIMIT} voci</b> dell'audit: la cronologia più vecchia non è raggiungibile da questa pagina.
          {truncated && " L'elenco è al limite, quindi alcune voci più vecchie potrebbero mancare."}
        </span>
      </p>
      <QueryState query={audit} isEmpty={(l) => l.length === 0} empty={<EmptyState icon="activity" title="Ancora nessuna attività" description="Qui compaiono le decisioni di policy, le azioni dei tool e i comandi di sistema." />}>
        {() => (
          <>
            <AuditFilterBar filters={filters} onChange={change} {...menus} />
            <AuditTable
              rows={shown.rows}
              empty={isFiltering(filters) ? (
                <EmptyState icon="search" title="Nessuna riga corrisponde ai filtri" action={<Button size="sm" onClick={() => change(emptyFilters)}>Azzera i filtri</Button>} />
              ) : "Ancora nessuna attività."}
            />
            <Pager page={shown} size={size} onPage={setPage} onSize={(n) => { setSize(n); setPage(1); }} />
          </>
        )}
      </QueryState>
    </Page>
  );
}
