import { useState } from "react";
import { useApp } from "../state/app";
import { Icon, Meter, formatBytes, formatMb, useAsync } from "../components/ui";
import type { CatalogModel, Fit } from "../api/types";

const FIT: Record<Fit, { label: string; tone: string }> = {
  great: { label: "Ci sta comodo", tone: "ok" },
  ok: { label: "Ci sta", tone: "info" },
  tight: { label: "Al limite", tone: "warn" },
  no: { label: "Non ci sta", tone: "crit" },
};

export function ModelsView() {
  const { api, toast, system } = useApp();
  const [tab, setTab] = useState<"installed" | "catalog">("installed");
  const [q, setQ] = useState("");
  const models = useAsync(() => api.models(), [api]);
  const catalog = useAsync(() => api.catalog(), [api]);
  const downloads = useAsync(() => api.downloads(), [api], 1000);

  const pull = async (c: CatalogModel) => {
    try {
      await api.pullModel(c.id);
      toast(`Download avviato: ${c.name}`);
      downloads.reload();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  const active = downloads.data?.filter((d) => d.status === "running") ?? [];
  const totalInstalled = models.data?.reduce((s, m) => s + m.size_bytes, 0) ?? 0;
  const filteredCatalog = (catalog.data ?? []).filter((c) => !q || `${c.name} ${c.family} ${c.tags.join(" ")}`.toLowerCase().includes(q.toLowerCase()));
  const vram = system?.device.accelerators.find((a) => a.memory_mb)?.memory_mb;

  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Inferenza</span>
          <h1>Modelli</h1>
          <span className="muted">
            {models.data?.length ?? 0} installati · {formatBytes(totalInstalled)} su disco · memoria disponibile: RAM {formatMb(system?.device.memory_mb)}{vram ? `, VRAM ${formatMb(vram)}` : ""}
          </span>
        </div>
        <div className="actions">
          <div className="seg" role="group" aria-label="Vista">
            <button aria-pressed={tab === "installed"} onClick={() => setTab("installed")}>Installati</button>
            <button aria-pressed={tab === "catalog"} onClick={() => setTab("catalog")}>Catalogo</button>
          </div>
        </div>
      </div>

      {active.length > 0 && (
        <section className="section">
          <h2>Download in corso</h2>
          <div className="panel">
            <div className="list">
              {active.map((d) => (
                <div className="list-item" key={d.id}>
                  <Icon name="download" />
                  <div className="grow stack" style={{ gap: 4 }}>
                    <div className="row" style={{ justifyContent: "space-between" }}>
                      <span className="mono ellipsis" style={{ fontSize: "var(--fs-sm)" }}>{d.file}</span>
                      <span className="mono faint num" style={{ fontSize: "var(--fs-xs)" }}>{formatBytes(d.received)} / {formatBytes(d.total)}</span>
                    </div>
                    <Meter value={d.received} max={d.total} />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </section>
      )}

      {tab === "installed" && (
        <div className="table-wrap">
          <table className="data">
            <thead>
              <tr><th>Modello</th><th>Formato</th><th>Dimensione</th><th>Parametri</th><th>Contesto</th><th>Capacità</th><th>Runtime compatibili</th></tr>
            </thead>
            <tbody>
              {models.data?.map((m) => (
                <tr key={m.id}>
                  <td><b>{m.name}</b><div className="mono faint" style={{ fontSize: "var(--fs-xs)" }}>{m.id}</div></td>
                  <td><div className="chips"><span className="chip">.{m.format}</span>{m.quant && <span className="chip">{m.quant}</span>}</div></td>
                  <td className="num">{formatBytes(m.size_bytes)}</td>
                  <td className="num">{m.params_b ? `${m.params_b}B` : "—"}</td>
                  <td className="num">{m.context_length ? `${Math.round(m.context_length / 1024)}K` : "—"}</td>
                  <td><div className="chips">{m.capabilities.map((c) => <span key={c} className="chip">{c}</span>)}</div></td>
                  <td className="mono" style={{ fontSize: "var(--fs-xs)" }}>{m.runtimes.join(", ")}</td>
                </tr>
              ))}
              {models.data?.length === 0 && (
                <tr><td colSpan={7}><div className="empty">Nessun modello. Apri il catalogo e scarica un modello adatto a questo device.</div></td></tr>
              )}
            </tbody>
          </table>
        </div>
      )}

      {tab === "catalog" && (
        <>
          <div className="row" style={{ marginBottom: 14 }}>
            <input className="input" style={{ maxWidth: 360 }} id="catalog-search" aria-label="Cerca nel catalogo" placeholder="Cerca: agent, vision, mobile, coding…" value={q} onChange={(e) => setQ(e.target.value)} />
            <span className="faint" style={{ fontSize: "var(--fs-sm)" }}>La compatibilità è calcolata su RAM, VRAM e acceleratori di questo device.</span>
          </div>
          <div className="grid">
            {filteredCatalog.map((c) => {
              const dl = downloads.data?.find((d) => d.model_id === c.id && d.status === "running");
              return (
                <article key={c.id} className="rt-card">
                  <div className="rt-top">
                    <div className="t">
                      <b>{c.name}</b>
                      <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>{c.publisher} · {c.license}</span>
                    </div>
                    <span className={`pill ${FIT[c.fit].tone}`}>{FIT[c.fit].label}</span>
                  </div>
                  <p className="rt-desc">{c.description}</p>
                  <dl className="kv">
                    <dt>Parametri</dt><dd className="num">{c.params_b}B{c.active_params_b ? ` (${c.active_params_b}B attivi)` : ""}</dd>
                    <dt>File</dt><dd className="num">{c.format} · {c.quant} · {formatBytes(c.size_bytes)}</dd>
                    <dt>Contesto</dt><dd className="num">{Math.round(c.context_length / 1024)}K token</dd>
                    <dt>Su questo device</dt><dd>{c.fit_detail}</dd>
                  </dl>
                  <div className="chips">{c.capabilities.map((x) => <span key={x} className="chip">{x}</span>)}</div>
                  <div className="rt-foot">
                    {c.installed ? (
                      <span className="pill ok"><Icon name="check" size={13} /> Installato</span>
                    ) : dl ? (
                      <div className="grow"><Meter value={dl.received} max={dl.total} /></div>
                    ) : c.source.kind === "system" ? (
                      <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>Modello di sistema, non si scarica</span>
                    ) : (
                      <button className="btn primary" disabled={c.fit === "no"} onClick={() => pull(c)}><Icon name="download" size={14} /> Scarica</button>
                    )}
                    {c.source.kind === "hf" && <span className="mono faint ellipsis" style={{ fontSize: "0.7rem", marginLeft: "auto", maxWidth: "55%" }}>{c.source.repo}</span>}
                  </div>
                </article>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
