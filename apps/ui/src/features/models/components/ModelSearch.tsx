import { useMemo, useState } from "react";
import { setQuery, useQueryParams } from "../../../app/router";
import { Button, EmptyState, Field, Input, Segmented, Select, Skeleton } from "../../../shared/ui";
import type { ModelProviderInfo, RemoteModel } from "../../../shared/api/types";
import { useModelSearch } from "../api";
import { exactRef, formatOptions, parseSort, sortOptionsFor, stubModel } from "../model/providers";
import { useQueryText } from "../useQueryText";
import { HubError } from "./HubError";
import { RemoteModelCard } from "./RemoteModelCard";
import { VariantsDrawer } from "./VariantsDrawer";

import { t } from "../../../shared/i18n";
const PAGE_SIZE = 12;

/** Live search on a provider: debounced box, format and sort filters, result cards and "Carica altri". */
export function ModelSearch({ provider, info, onSetToken }: { provider: string; info: ModelProviderInfo | undefined; onSetToken: () => void }) {
  const query = useQueryParams();
  const [text, setText] = useQueryText("q");
  const q = query.q ?? "";
  const format = query.format ?? "";
  const sort = parseSort(query.sort);
  const [open, setOpen] = useState<RemoteModel | null>(null);

  const search = useModelSearch({ provider, q, format: format || undefined, sort, limit: PAGE_SIZE });
  const items = useMemo(() => search.data?.pages.flatMap((p) => p.items) ?? [], [search.data]);
  const exact = exactRef(provider, q);
  const formats = formatOptions(info);

  return (
    <div className="stack stack-lg">
      <div className="mh-toolbar" role="search">
        <Field label={t("models.cerca_modelli")} htmlFor="disc-q" className="mh-search">
          <Input
            id="disc-q" type="search" value={text} onChange={(e) => setText(e.target.value)}
            placeholder={provider === "ollama" ? t("models.qwen3_llama3_2_3b_nome") : t("models.qwen_gemma_llama_vision")}
          />
        </Field>
        <Field label={t("models.formato")} htmlFor="disc-format">
          <Select id="disc-format" value={format} onChange={(e) => setQuery({ format: e.target.value })}>
            <option value="">{t("models.tutti")}</option>
            {formats.map((f) => <option key={f} value={f}>.{f}</option>)}
          </Select>
        </Field>
        <div className="field">
          <span className="field-label">{t("models.ordina_per")}</span>
          <Segmented label={t("models.ordina_per")} value={sort} onChange={(s) => setQuery({ sort: s === "downloads" ? "" : s })} options={sortOptionsFor(provider)} />
        </div>
      </div>

      {search.isPending && (
        <div className="mh-grid" role="status" aria-label={t("models.caricamento_dei_risultati")}>
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} height={168} className="skeleton-card" />)}
        </div>
      )}
      {search.isError && !search.data && (
        <HubError error={search.error} onRetry={() => void search.refetch()} retrying={search.isFetching} onSetToken={onSetToken} />
      )}
      {search.isSuccess && items.length === 0 && (
        <EmptyState
          icon="search" title={q ? t("models.nessun_risultato_per", { q }) : t("models.nessun_modello_da_mostrare")}
          description={t("models.prova_un_altro_termine_un")}
          action={exact ? <Button icon="list" onClick={() => setOpen(stubModel(provider, exact))}>{t("models.apri_per_nome_esatto", { exact })}</Button> : undefined}
        />
      )}

      {items.length > 0 && (
        <>
          <div className="mh-grid">
            {items.map((m) => <RemoteModelCard key={`${m.provider}/${m.id}`} model={m} onOpen={() => setOpen(m)} />)}
          </div>
          <div className="row mh-more">
            <span className="faint text-sm" role="status" aria-live="polite">{items.length} {items.length === 1 ? t("models.risultato") : t("models.risultati")}{search.hasNextPage ? t("models.finora") : ""}</span>
            {search.hasNextPage && (
              <Button icon="down" onClick={() => void search.fetchNextPage()} disabled={search.isFetchingNextPage}>
                {search.isFetchingNextPage ? t("models.carico") : t("models.carica_altri")}
              </Button>
            )}
          </div>
          {search.isFetchNextPageError && <HubError error={search.error} title={t("models.non_riesco_a_caricare_altri")} onRetry={() => void search.fetchNextPage()} retrying={search.isFetchingNextPage} />}
          {exact && !items.some((m) => m.id.toLowerCase() === exact.toLowerCase()) && (
            <p className="faint text-sm">
              {t("models.non_lo_trovi")} <Button size="sm" variant="ghost" onClick={() => setOpen(stubModel(provider, exact))}>{t("models.apri_per_nome_esatto", { exact })}</Button>
            </p>
          )}
        </>
      )}
      {open && <VariantsDrawer model={open} onClose={() => setOpen(null)} />}
    </div>
  );
}
