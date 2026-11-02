import { useState } from "react";
import { navigate, useParams } from "../../app/router";
import { formatBytes, formatContext, formatMb, formatNumber, formatParams } from "../../shared/lib/format";
import { toast } from "../../shared/lib/toast";
import { useSystem } from "../../shared/query/system";
import { Button, Chip, Chips, DataTable, Dialog, EmptyState, Icon, IconButton, Meter, Page, PageHeader, Pill, QueryState, TabPanel, Tabs, type Column } from "../../shared/ui";
import type { CatalogModel, LocalModel } from "../../shared/api/types";
import { isActiveDownload, useCatalog, useDeleteModel, useDownloads, useModels, usePullModel } from "./api";
import { ActiveDownloads, DownloadsList } from "./components/Downloads";
import { Discover } from "./components/Discover";
import { FIT } from "./model/fit";

type Tab = "installed" | "discover" | "downloads" | "catalog";
const TABS: Tab[] = ["installed", "discover", "downloads", "catalog"];

export function ModelsView() {
  const models = useModels();
  const downloads = useDownloads();
  const system = useSystem();
  const rawTab = useParams().tab;
  const tab: Tab = TABS.includes(rawTab as Tab) ? (rawTab as Tab) : "installed";
  const jobs = downloads.data ?? [];

  const totalInstalled = models.data?.reduce((s, m) => s + m.size_bytes, 0) ?? 0;
  const vram = system.data?.device.accelerators.find((a) => a.memory_mb)?.memory_mb;

  return (
    <Page>
      <PageHeader
        eyebrow="Inferenza" title="Modelli"
        subtitle={`${models.data?.length ?? 0} installati · ${formatBytes(totalInstalled)} su disco · memoria disponibile: RAM ${formatMb(system.data?.device.memory_mb)}${vram ? `, VRAM ${formatMb(vram)}` : ""}`}
      />
      <Tabs
        idPrefix="models" label="Sezioni dei modelli" value={tab} onChange={(t) => navigate(t === "installed" ? "/models" : `/models/${t}`)}
        tabs={[
          { id: "installed", label: "Installati", count: models.data?.length },
          { id: "discover", label: "Scopri" },
          { id: "downloads", label: "Download", count: jobs.filter(isActiveDownload).length || undefined },
          { id: "catalog", label: "Catalogo" },
        ]}
      />
      <ActiveDownloads jobs={tab === "downloads" ? [] : jobs} />
      <TabPanel idPrefix="models" id={tab}>
        {tab === "installed" && <Installed />}
        {tab === "discover" && <Discover />}
        {tab === "downloads" && <DownloadsList jobs={jobs} />}
        {tab === "catalog" && <Catalog />}
      </TabPanel>
    </Page>
  );
}

function Installed() {
  const models = useModels();
  const remove = useDeleteModel();
  const [confirm, setConfirm] = useState<LocalModel | null>(null);
  const columns: Column<LocalModel>[] = [
    { key: "model", header: "Modello", render: (m) => <><b>{m.name}</b><div className="mono faint text-xs">{m.id}</div></> },
    { key: "format", header: "Formato", render: (m) => <Chips><Chip>.{m.format}</Chip>{m.quant && <Chip>{m.quant}</Chip>}</Chips> },
    { key: "size", header: "Dimensione", className: "num", render: (m) => formatBytes(m.size_bytes) },
    { key: "params", header: "Parametri", className: "num", render: (m) => formatParams(m.params_b) },
    { key: "ctx", header: "Contesto", className: "num", render: (m) => formatContext(m.context_length) },
    { key: "caps", header: "Capacità", render: (m) => <Chips>{m.capabilities.map((c) => <Chip key={c}>{c}</Chip>)}</Chips> },
    { key: "rt", header: "Runtime compatibili", className: "mono text-xs", render: (m) => m.runtimes.join(", ") },
    { key: "origin", header: "Origine", render: (m) => (m.origin ? <Pill>{m.origin}</Pill> : "—") },
    { key: "actions", header: <span className="sr-only">Azioni</span>, render: (m) => m.removable && <IconButton aria-label={`Elimina ${m.name}`} icon="trash" size={16} onClick={() => setConfirm(m)} /> },
  ];
  return (
    <QueryState query={models}>
      {(list) => (
        <>
          <DataTable label="Modelli installati" columns={columns} rows={list} rowKey={(m) => m.id}
            empty={<EmptyState icon="cube" title="Nessun modello" description="Apri Scopri o il catalogo e scarica un modello adatto a questo device." action={<Button variant="primary" icon="search" onClick={() => navigate("/models/discover")}>Scopri modelli</Button>} />} />
          {confirm && (
            <Dialog title="Elimina il modello" size="sm" onClose={() => setConfirm(null)}
              footer={<>
                <Button onClick={() => setConfirm(null)}>Annulla</Button>
                <Button variant="danger" icon="trash" onClick={() => remove.mutate(confirm.id, { onSettled: () => setConfirm(null) })} disabled={remove.isPending}>Elimina</Button>
              </>}>
              <p>Vuoi eliminare <b>{confirm.name}</b> dal disco? Libera {formatBytes(confirm.size_bytes)}; potrai riscaricarlo in seguito.</p>
            </Dialog>
          )}
        </>
      )}
    </QueryState>
  );
}

function Catalog() {
  const catalog = useCatalog();
  const downloads = useDownloads();
  const pull = usePullModel();
  const [q, setQ] = useState("");
  const filtered = (c: CatalogModel[]) => c.filter((m) => !q || `${m.name} ${m.family} ${m.tags.join(" ")}`.toLowerCase().includes(q.toLowerCase()));
  return (
    <>
      <div className="row mb-3">
        <input className="input input-search" id="catalog-search" aria-label="Cerca nel catalogo" placeholder="Cerca: agent, vision, mobile, coding…" value={q} onChange={(e) => setQ(e.target.value)} />
        <span className="faint text-sm">La compatibilità è calcolata su RAM, VRAM e acceleratori di questo device.</span>
      </div>
      <QueryState query={catalog}>
        {(list) => (
          <div className="grid">
            {filtered(list).map((c) => {
              const dl = downloads.data?.find((d) => d.model_id === c.id && isActiveDownload(d));
              return (
                <article key={c.id} className="rt-card">
                  <div className="rt-top">
                    <div className="t">
                      <b>{c.name}</b>
                      <span className="faint text-xs">{c.publisher} · {c.license}</span>
                    </div>
                    <Pill tone={FIT[c.fit].tone}>{FIT[c.fit].label}</Pill>
                  </div>
                  <p className="rt-desc">{c.description}</p>
                  <dl className="kv">
                    <dt>Parametri</dt><dd className="num">{formatParams(c.params_b)}{c.active_params_b ? ` (${formatNumber(c.active_params_b)}B attivi)` : ""}</dd>
                    <dt>File</dt><dd className="num">{c.format} · {c.quant} · {formatBytes(c.size_bytes)}</dd>
                    <dt>Contesto</dt><dd className="num">{formatContext(c.context_length)} token</dd>
                    <dt>Su questo device</dt><dd>{c.fit_detail}</dd>
                  </dl>
                  <Chips>{c.capabilities.map((x) => <Chip key={x}>{x}</Chip>)}</Chips>
                  <div className="rt-foot">
                    {c.installed ? (
                      <Pill tone="ok"><Icon name="check" size={13} /> Installato</Pill>
                    ) : dl ? (
                      <div className="grow"><Meter label={`Download di ${c.name}`} value={dl.received} max={dl.total} /></div>
                    ) : c.downloadable === false ? (
                      <span className="faint text-xs">Modello di sistema, non si scarica</span>
                    ) : (
                      <Button variant="primary" icon="download" disabled={c.fit === "no" || pull.isPending} onClick={() => pull.mutate(c.id, { onSuccess: () => toast(`Download avviato: ${c.name}`) })}>Scarica</Button>
                    )}
                    {c.source.kind === "hf" && <span className="mono faint ellipsis text-xs ml-auto maxw-55">{c.source.repo}</span>}
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </QueryState>
    </>
  );
}
