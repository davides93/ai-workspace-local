import { useMemo, useState } from "react";
import { downloadFile } from "../../shared/lib/download";
import { Button, EmptyState, Icon, Page, PageHeader, QueryState } from "../../shared/ui";
import { useAudit } from "./api";
import { AuditFilterBar } from "./components/AuditFilterBar";
import { AuditTable } from "./components/AuditTable";
import { Pager } from "./components/Pager";
import { auditExport, BACKEND_LIMIT, emptyFilters, facets, filterAudit, isFiltering, newestFirst, paginate, type AuditFilters } from "./model/filter";
import "./activity.css";

import { t, t as tr } from "../../shared/i18n";
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
        eyebrow={t("nav.group_control")} title={t("activity.attivita")}
        subtitle={t("activity.audit_trail_di_ogni_decisione")}
        actions={<Button icon="download" disabled={filtered.length === 0} onClick={exportJson} title={tr("activity.scarica_le_righe_filtrate_in")}>{(filtered.length > 0 ? t("activity.esporta_json_count", { count: filtered.length }) : t("activity.esporta_json"))}</Button>}
      />
      <p className={truncated ? "act-note is-warn" : "act-note"} role={truncated ? "status" : undefined}>
        <Icon name="info" size={15} />
        <span>
          Il workspace restituisce soltanto le <b>{t("activity.ultime_voci", { BACKEND_LIMIT })}</b> dell'audit: la cronologia più vecchia non è raggiungibile da questa pagina.
          {truncated && t("activity.l_elenco_e_al_limite")}
        </span>
      </p>
      <QueryState query={audit} isEmpty={(l) => l.length === 0} empty={<EmptyState icon="activity" title={t("activity.ancora_nessuna_attivita")} description={t("activity.qui_compaiono_le_decisioni_di")} />}>
        {() => (
          <>
            <AuditFilterBar filters={filters} onChange={change} {...menus} />
            <AuditTable
              rows={shown.rows}
              empty={isFiltering(filters) ? (
                <EmptyState icon="search" title={tr("activity.nessuna_riga_corrisponde_ai_filtri")} action={<Button size="sm" onClick={() => change(emptyFilters)}>{tr("activity.azzera_i_filtri")}</Button>} />
              ) : t("activity.ancora_nessuna_attivita_2")}
            />
            <Pager page={shown} size={size} onPage={setPage} onSize={(n) => { setSize(n); setPage(1); }} />
          </>
        )}
      </QueryState>
    </Page>
  );
}
