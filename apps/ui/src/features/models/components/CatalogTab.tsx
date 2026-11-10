import { useMemo } from "react";
import { setQuery } from "../../../app/router";
import { EmptyState, Field, Input, QueryState, Segmented, Skeleton } from "../../../shared/ui";
import type { CatalogModel, DownloadProgress } from "../../../shared/api/types";
import { useCatalog } from "../api";
import { findCatalogJob } from "../model/catalog";
import { hiddenCount } from "../model/adviser";
import { filterCatalog } from "../model/filters";
import { useQueryText } from "../useQueryText";
import { CatalogCard } from "./CatalogCard";
import { ShowAllToggle, useRunnableFilter } from "./ShowAll";
import { useProviderAuthDialog } from "./ProviderAuthDialog";

/** Catalogo (Consigliati): curated models with the fit computed for this device. */
export function CatalogTab({ jobs }: { jobs: DownloadProgress[] }) {
  const catalog = useCatalog();
  return (
    <QueryState
      query={catalog}
      skeleton={<div className="mh-grid">{Array.from({ length: 6 }, (_, i) => <Skeleton key={i} height={260} className="skeleton-card" />)}</div>}
    >
      {(list) => <CatalogList list={list} jobs={jobs} />}
    </QueryState>
  );
}

function CatalogList({ list, jobs }: { list: CatalogModel[]; jobs: DownloadProgress[] }) {
  const [text, setText] = useQueryText("q");
  const { simple, showAll, compatOnly } = useRunnableFilter();
  // Simple mode: only what runs here unless "Mostra tutti"; the developer view keeps its own filter.
  const fitOnly = simple ? !showAll : compatOnly;
  const auth = useProviderAuthDialog();
  const shown = useMemo(() => filterCatalog(list, { q: text, fitOnly }), [list, text, fitOnly]);

  return (
    <div className="stack stack-lg">
      <h2 className="sr-only">Catalogo consigliato</h2>
      <p className="muted text-sm">
        Modelli scelti e provati. La compatibilità è calcolata su RAM, VRAM e acceleratori di questo device: quelli che ci stanno meglio vengono prima.
      </p>
      <div className="mh-toolbar" role="search">
        <Field label="Cerca nel catalogo" htmlFor="cat-q" className="mh-search">
          <Input id="cat-q" type="search" placeholder="agent, vision, mobile, coding…" value={text} onChange={(e) => setText(e.target.value)} />
        </Field>
        {simple ? (
          <ShowAllToggle showAll={showAll} hidden={hiddenCount(list)} />
        ) : (
          <div className="field">
            <span className="field-label">Mostra</span>
            <Segmented
              label="Filtro di compatibilità" value={fitOnly ? "fit" : "all"} onChange={(v) => setQuery({ compat: v === "fit" ? "1" : "" })}
              options={[{ id: "all", label: "Tutti" }, { id: "fit", label: "Che ci stanno" }]}
            />
          </div>
        )}
      </div>
      <p className="mh-count" role="status" aria-live="polite">{shown.length === list.length ? `${list.length} modelli` : `${shown.length} di ${list.length} modelli`}</p>
      {shown.length === 0 ? (
        <EmptyState icon="search" title="Nessun modello corrisponde" description="Cambia la ricerca o mostra anche quelli che non ci stanno su questo device." />
      ) : (
        <div className="mh-grid">
          {shown.map((c) => <CatalogCard key={c.id} model={c} job={findCatalogJob(jobs, c)} onSetToken={() => auth.open("hf")} />)}
        </div>
      )}
      {auth.dialog}
    </div>
  );
}
