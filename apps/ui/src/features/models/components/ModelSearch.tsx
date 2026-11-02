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
        <Field label="Cerca modelli" htmlFor="disc-q" className="mh-search">
          <Input
            id="disc-q" type="search" value={text} onChange={(e) => setText(e.target.value)}
            placeholder={provider === "ollama" ? "qwen3, llama3.2:3b, nome esatto…" : "qwen, gemma, llama, vision…"}
          />
        </Field>
        <Field label="Formato" htmlFor="disc-format">
          <Select id="disc-format" value={format} onChange={(e) => setQuery({ format: e.target.value })}>
            <option value="">Tutti</option>
            {formats.map((f) => <option key={f} value={f}>.{f}</option>)}
          </Select>
        </Field>
        <div className="field">
          <span className="field-label">Ordina per</span>
          <Segmented label="Ordina per" value={sort} onChange={(s) => setQuery({ sort: s === "downloads" ? "" : s })} options={sortOptionsFor(provider)} />
        </div>
      </div>

      {search.isPending && (
        <div className="mh-grid" role="status" aria-label="Caricamento dei risultati">
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} height={168} className="skeleton-card" />)}
        </div>
      )}
      {search.isError && !search.data && (
        <HubError error={search.error} onRetry={() => void search.refetch()} retrying={search.isFetching} onSetToken={onSetToken} />
      )}
      {search.isSuccess && items.length === 0 && (
        <EmptyState
          icon="search" title={q ? `Nessun risultato per «${q}»` : "Nessun modello da mostrare"}
          description="Prova un altro termine, un altro formato o un altro provider."
          action={exact ? <Button icon="list" onClick={() => setOpen(stubModel(provider, exact))}>Apri «{exact}» per nome esatto</Button> : undefined}
        />
      )}

      {items.length > 0 && (
        <>
          <div className="mh-grid">
            {items.map((m) => <RemoteModelCard key={`${m.provider}/${m.id}`} model={m} onOpen={() => setOpen(m)} />)}
          </div>
          <div className="row mh-more">
            <span className="faint text-sm" role="status" aria-live="polite">{items.length} {items.length === 1 ? "risultato" : "risultati"}{search.hasNextPage ? " finora" : ""}</span>
            {search.hasNextPage && (
              <Button icon="down" onClick={() => void search.fetchNextPage()} disabled={search.isFetchingNextPage}>
                {search.isFetchingNextPage ? "Carico…" : "Carica altri"}
              </Button>
            )}
          </div>
          {search.isFetchNextPageError && <HubError error={search.error} title="Non riesco a caricare altri risultati" onRetry={() => void search.fetchNextPage()} retrying={search.isFetchingNextPage} />}
          {exact && !items.some((m) => m.id.toLowerCase() === exact.toLowerCase()) && (
            <p className="faint text-sm">
              Non lo trovi? <Button size="sm" variant="ghost" onClick={() => setOpen(stubModel(provider, exact))}>Apri «{exact}» per nome esatto</Button>
            </p>
          )}
        </>
      )}
      {open && <VariantsDrawer model={open} onClose={() => setOpen(null)} />}
    </div>
  );
}
