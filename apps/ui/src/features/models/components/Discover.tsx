import { useEffect, useState } from "react";
import { setQuery, useQueryParams } from "../../../app/router";
import { formatBytes, formatCompact, relTime } from "../../../shared/lib/format";
import { Button, Chip, Chips, Drawer, EmptyState, ErrorState, Icon, Pill, SkeletonLines } from "../../../shared/ui";
import type { RemoteModel } from "../../../shared/api/types";
import { useModelProviders, useModelSearch, usePullModel, useRemoteModel } from "../api";
import { FIT, PROVIDER_FORMATS } from "../model/fit";

/** First version of the live discovery: search a provider, pick a variant, download it. */
export function Discover() {
  const query = useQueryParams();
  const providers = useModelProviders();
  const provider = query.provider || "hf";
  const format = query.format ?? "";
  const [text, setText] = useState(query.q ?? "");
  const [open, setOpen] = useState<RemoteModel | null>(null);

  // Keep the URL (shareable filters) in sync with the box, without a history entry per keystroke.
  useEffect(() => {
    const t = setTimeout(() => setQuery({ q: text }), 350);
    return () => clearTimeout(t);
  }, [text]);

  const search = useModelSearch({ provider, q: query.q ?? "", format: format || undefined, sort: "downloads", limit: 20 });
  const items = search.data?.pages.flatMap((p) => p.items) ?? [];
  const info = providers.data?.find((p) => p.id === provider);

  return (
    <div className="stack stack-lg">
      <div className="row filters">
        <div className="field">
          <label htmlFor="disc-provider">Provider</label>
          <select id="disc-provider" className="input" value={provider} onChange={(e) => setQuery({ provider: e.target.value })}>
            {(providers.data ?? [{ id: "hf", name: "Hugging Face" }]).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        </div>
        <div className="field grow">
          <label htmlFor="disc-q">Cerca</label>
          <input id="disc-q" className="input" type="search" placeholder="qwen, gemma, llama, vision…" value={text} onChange={(e) => setText(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="disc-format">Formato</label>
          <select id="disc-format" className="input" value={format} onChange={(e) => setQuery({ format: e.target.value })}>
            <option value="">Tutti</option>
            {PROVIDER_FORMATS.map((f) => <option key={f} value={f}>{f}</option>)}
          </select>
        </div>
      </div>
      {info && <p className="faint text-sm">{info.description}</p>}

      {search.isPending && <SkeletonLines lines={5} />}
      {search.isError && <ErrorState error={search.error} onRetry={() => void search.refetch()} />}
      {search.isSuccess && items.length === 0 && <EmptyState icon="search" title="Nessun risultato" description="Prova un altro termine, un altro formato o un altro provider." />}
      <div className="grid">
        {items.map((m) => <ResultCard key={`${m.provider}/${m.id}`} m={m} onOpen={() => setOpen(m)} />)}
      </div>
      {search.hasNextPage && <div className="row"><Button onClick={() => void search.fetchNextPage()} disabled={search.isFetchingNextPage}>Mostra altri</Button></div>}
      {open && <VariantsDrawer model={open} onClose={() => setOpen(null)} />}
    </div>
  );
}

function ResultCard({ m, onOpen }: { m: RemoteModel; onOpen: () => void }) {
  return (
    <article className="rt-card">
      <div className="rt-top">
        <div className="t">
          <b className="ellipsis">{m.name}</b>
          <span className="faint text-xs ellipsis">{m.publisher}{m.license ? ` · ${m.license}` : ""}</span>
        </div>
        {m.gated && <Pill tone="warn"><Icon name="lock" size={12} /> gated</Pill>}
      </div>
      {m.description && <p className="rt-desc">{m.description}</p>}
      <Chips>{m.formats.map((f) => <Chip key={f}>.{f}</Chip>)}{m.capabilities.map((c) => <Chip key={c}>{c}</Chip>)}</Chips>
      <div className="rt-foot faint text-xs">
        {m.downloads !== undefined && <span className="num">{formatCompact(m.downloads)} download</span>}
        {m.likes !== undefined && <span className="num">{formatCompact(m.likes)} like</span>}
        {m.updated_at && <span>{relTime(m.updated_at)}</span>}
        <Button size="sm" className="ml-auto" onClick={onOpen}>Varianti</Button>
      </div>
    </article>
  );
}

function VariantsDrawer({ model, onClose }: { model: RemoteModel; onClose: () => void }) {
  const detail = useRemoteModel(model.provider, model.id);
  const pull = usePullModel();
  return (
    <Drawer title={model.name} subtitle={`${model.provider} · ${model.id}`} onClose={onClose}>
      {detail.isPending && <SkeletonLines lines={4} />}
      {detail.isError && <ErrorState error={detail.error} onRetry={() => void detail.refetch()} />}
      {detail.data?.variants.map((v) => (
        <div key={v.id} className="variant">
          <div className="row row-between">
            <b>{v.label}</b>
            <span className="row">
              {v.recommended && <Pill tone="run">consigliata</Pill>}
              <Pill tone={FIT[v.fit].tone}>{FIT[v.fit].label}</Pill>
            </span>
          </div>
          <div className="faint text-sm">{formatBytes(v.size_bytes)} · {v.files.length} {v.files.length === 1 ? "file" : "file"}{v.files.some((f) => f.role === "mmproj") ? " (con mmproj)" : ""} · {v.fit_detail}</div>
          <Chips label="Runtime compatibili">
            {v.runtimes.map((r) => <Chip key={r.id} hot={r.available} title={r.reason || undefined}>{r.name}</Chip>)}
          </Chips>
          {v.installed ? (
            <Pill tone="ok"><Icon name="check" size={13} /> Installata</Pill>
          ) : (
            <Button variant="primary" size="sm" icon="download" disabled={v.fit === "no" || pull.isPending}
              onClick={() => pull.mutate({ provider: model.provider, model: model.id, variant: v.id, revision: detail.data?.revision }, { onSuccess: onClose })}>
              Scarica
            </Button>
          )}
        </div>
      ))}
    </Drawer>
  );
}
