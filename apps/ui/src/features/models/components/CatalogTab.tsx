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

import { t } from "../../../shared/i18n";
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
      <h2 className="sr-only">{t("models.catalogo_consigliato")}</h2>
      <p className="muted text-sm">
        {t("models.modelli_scelti_e_provati_la")}</p>
      <div className="mh-toolbar" role="search">
        <Field label={t("models.cerca_nel_catalogo")} htmlFor="cat-q" className="mh-search">
          <Input id="cat-q" type="search" placeholder={t("models.agent_vision_mobile_coding")} value={text} onChange={(e) => setText(e.target.value)} />
        </Field>
        {simple ? (
          <ShowAllToggle showAll={showAll} hidden={hiddenCount(list)} />
        ) : (
          <div className="field">
            <span className="field-label">{t("models.mostra")}</span>
            <Segmented
              label={t("models.filtro_di_compatibilita")} value={fitOnly ? "fit" : "all"} onChange={(v) => setQuery({ compat: v === "fit" ? "1" : "" })}
              options={[{ id: "all", label: t("models.tutti") }, { id: "fit", label: t("models.che_ci_stanno") }]}
            />
          </div>
        )}
      </div>
      <p className="mh-count" role="status" aria-live="polite">{shown.length === list.length ? t("models.modelli_2", { length: list.length }) : t("models.di_modelli", { length: shown.length, length2: list.length })}</p>
      {shown.length === 0 ? (
        <EmptyState icon="search" title={t("models.nessun_modello_corrisponde")} description={t("models.cambia_la_ricerca_o_mostra")} />
      ) : (
        <div className="mh-grid">
          {shown.map((c) => <CatalogCard key={c.id} model={c} job={findCatalogJob(jobs, c)} onSetToken={() => auth.open("hf")} />)}
        </div>
      )}
      {auth.dialog}
    </div>
  );
}
