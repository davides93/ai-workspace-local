import { useMemo, useState } from "react";
import { useApp } from "../state/app";
import { Drawer, Icon, InlineText, SchemaForm, StatusPill, useAsync } from "../components/ui";
import type { RuntimeInfo, RuntimeKind } from "../api/types";

const KIND_META: Record<RuntimeKind, { title: string; desc: string }> = {
  sidecar: { title: "Sidecar gestiti", desc: "Processi avviati, monitorati e fermati dall'app; parlano HTTP compatibile OpenAI." },
  embedded: { title: "Embedded", desc: "Librerie collegate in-process via FFI: nessun processo esterno; sono il default su mobile." },
  system: { title: "Modelli di sistema", desc: "Modelli già presenti nel sistema operativo, condivisi tra le app." },
  remote: { title: "Remoti in LAN", desc: "Runtime di altri dispositivi AIW o server compatibili nella tua rete." },
};
const ORDER: RuntimeKind[] = ["sidecar", "embedded", "system", "remote"];

export function RuntimesView() {
  const { api, system, toast } = useApp();
  const rts = useAsync(() => api.runtimes(), [api], 5000);
  const models = useAsync(() => api.models(), [api]);
  const [filter, setFilter] = useState<"here" | "all">("here");
  const [editing, setEditing] = useState<RuntimeInfo | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const deviceAccels = useMemo(() => new Set(system?.device.accelerators.map((a) => a.kind) ?? []), [system]);
  const list = (rts.data ?? []).filter((r) => filter === "all" || r.status !== "unsupported");
  const counts = {
    running: rts.data?.filter((r) => r.status === "running").length ?? 0,
    available: rts.data?.filter((r) => r.status !== "unsupported").length ?? 0,
    total: rts.data?.length ?? 0,
  };

  const act = async (r: RuntimeInfo, action: "start" | "stop") => {
    setBusy(r.id);
    try {
      await api.runtimeAction(r.id, action);
      toast(`${r.name}: ${action === "start" ? "avviato" : "fermato"}`);
      rts.reload();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Inferenza</span>
          <h1>Runtime</h1>
          <span className="muted">{counts.running} attivi · {counts.available} utilizzabili qui · {counts.total} nel catalogo</span>
        </div>
        <div className="actions">
          <div className="seg" role="group" aria-label="Filtro">
            <button aria-pressed={filter === "here"} onClick={() => setFilter("here")}>Su questo device</button>
            <button aria-pressed={filter === "all"} onClick={() => setFilter("all")}>Tutto il catalogo</button>
          </div>
          <button className="btn" onClick={() => rts.reload()}><Icon name="refresh" size={15} /> Rileva</button>
        </div>
      </div>
      {rts.error && <div className="empty">Errore nel caricamento: {rts.error}</div>}
      {ORDER.map((kind) => {
        const items = list.filter((r) => r.kind === kind);
        if (items.length === 0) return null;
        return (
          <section className="section" key={kind}>
            <div className="section-head">
              <h2>{KIND_META[kind].title}</h2>
              <span className="muted">{KIND_META[kind].desc}</span>
            </div>
            <div className="grid">
              {items.map((r) => (
                <RuntimeCard key={r.id} r={r} deviceAccels={deviceAccels} busy={busy === r.id} onAct={act} onEdit={() => setEditing(r)} />
              ))}
            </div>
          </section>
        );
      })}
      <p className="faint" style={{ fontSize: "var(--fs-sm)" }}>
        Ogni runtime è un manifest JSON. Per aggiungerne uno basta creare un file in <code>~/.aiw/runtimes/</code>: compare qui con le sue impostazioni.
      </p>
      {editing && (
        <RuntimeDrawer r={editing} models={models.data?.filter((m) => m.runtimes.includes(editing.id)).map((m) => m.id) ?? []}
          onClose={() => setEditing(null)}
          onSaved={() => { rts.reload(); setEditing(null); }} />
      )}
    </div>
  );
}

function RuntimeCard({ r, deviceAccels, busy, onAct, onEdit }: {
  r: RuntimeInfo; deviceAccels: Set<string>; busy: boolean; onAct: (r: RuntimeInfo, a: "start" | "stop") => void; onEdit: () => void;
}) {
  const dim = r.status === "unsupported";
  return (
    <article className={`rt-card ${r.status === "running" ? "is-running" : ""} ${dim ? "is-dim" : ""}`}>
      <div className="rt-top">
        <div className="t">
          <b>{r.name}</b>
          <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>{r.vendor}</span>
        </div>
        <span className="kind-badge">{r.kind}</span>
      </div>
      <div className="row">
        <StatusPill status={r.status} />
        {r.status_detail && r.status !== "installable" && (
          <span className="faint mono ellipsis" style={{ fontSize: "var(--fs-xs)", maxWidth: "100%", direction: r.status_detail.includes("/") || r.status_detail.includes("\\") ? "rtl" : undefined }} title={r.status_detail}>{r.status_detail}</span>
        )}
      </div>
      <p className="rt-desc"><InlineText text={r.description} /></p>
      <div className="chips" aria-label="Acceleratori">
        {r.accelerators.map((a) => <span key={a} className={`chip ${deviceAccels.has(a) ? "hot" : ""}`} title={deviceAccels.has(a) ? "Presente su questo device" : undefined}>{a}</span>)}
        {r.formats.map((f) => <span key={f} className="chip">.{f}</span>)}
      </div>
      {r.loaded_models.length > 0 && <div className="mono faint ellipsis" style={{ fontSize: "var(--fs-xs)" }}>caricato: {r.loaded_models.join(", ")}</div>}
      <div className="rt-foot">
        {r.status === "running" || r.status === "starting" ? (
          <button className="btn" disabled={busy} onClick={() => onAct(r, "stop")}><Icon name="stop" size={14} /> Ferma</button>
        ) : r.status === "installable" ? (
          <span className="mono faint ellipsis" style={{ fontSize: "var(--fs-xs)" }} title={r.install_hint}>{r.install_hint ?? "installa dal sito del progetto"}</span>
        ) : r.status !== "unsupported" ? (
          <button className="btn primary" disabled={busy} onClick={() => onAct(r, "start")}><Icon name="play" size={14} /> {busy ? "Avvio…" : "Avvia"}</button>
        ) : (
          <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>Disponibile su: {r.platforms.join(", ")}</span>
        )}
        <button className="btn ghost" style={{ marginLeft: "auto" }} onClick={onEdit}><Icon name="settings" size={14} /> Impostazioni</button>
      </div>
    </article>
  );
}

function RuntimeDrawer({ r, models, onClose, onSaved }: { r: RuntimeInfo; models: string[]; onClose: () => void; onSaved: () => void }) {
  const { api, toast } = useApp();
  const [value, setValue] = useState<Record<string, unknown>>(r.settings);
  const [advanced, setAdvanced] = useState(false);
  const [model, setModel] = useState(models[0] ?? "");
  const logs = useAsync(() => api.runtimeLogs(r.id), [api, r.id], r.status === "running" ? 2000 : undefined);
  const save = async () => {
    try {
      await api.updateRuntimeSettings(r.id, value);
      toast("Impostazioni salvate");
      onSaved();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };
  const startWith = async () => {
    try {
      await api.updateRuntimeSettings(r.id, value);
      await api.runtimeAction(r.id, "start", { model });
      toast(`${r.name} avviato con ${model}`);
      onSaved();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };
  return (
    <Drawer title={r.name} subtitle={`${r.kind} · API ${r.api} · ${r.platforms.join(" / ")}`} onClose={onClose}
      footer={<>
        <button className="btn" onClick={onClose}>Annulla</button>
        <button className="btn primary" onClick={save}>Salva</button>
      </>}>
      <dl className="kv">
        <dt>Stato</dt><dd><StatusPill status={r.status} /></dd>
        <dt>Endpoint</dt><dd className="mono">{r.endpoint ?? "—"}</dd>
        <dt>Capacità</dt><dd>{r.capabilities.join(", ")}</dd>
        {r.homepage && <><dt>Progetto</dt><dd><a href={r.homepage} target="_blank" rel="noreferrer">{r.homepage.replace(/^https?:\/\//, "")}</a></dd></>}
        {r.install_hint && <><dt>Installazione</dt><dd className="mono">{r.install_hint}</dd></>}
      </dl>
      <div className="stack">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h3>Impostazioni</h3>
          <label className="row" style={{ fontSize: "var(--fs-sm)" }}>
            <span className="toggle"><input id={`adv-${r.id}`} type="checkbox" checked={advanced} onChange={(e) => setAdvanced(e.target.checked)} /></span>
            Avanzate
          </label>
        </div>
        <SchemaForm idPrefix={`rt-${r.id}`} schema={r.settings_schema} value={value} onChange={setValue} showAdvanced={advanced} />
      </div>
      {(logs.data?.length ?? 0) > 0 && (
        <div className="stack">
          <h3>Log del processo</h3>
          <pre className="mono" style={{ margin: 0, maxHeight: 220, overflow: "auto", background: "var(--code-bg)", border: "1px solid var(--line)", borderRadius: "var(--r-sm)", padding: 10, fontSize: "0.72rem" }}>{logs.data!.slice(-60).join("\n")}</pre>
        </div>
      )}
      {models.length > 0 && r.status !== "unsupported" && (
        <div className="stack">
          <h3>Avvia con un modello</h3>
          <div className="row">
            <select className="input" style={{ flex: 1 }} aria-label="Modello" value={model} onChange={(e) => setModel(e.target.value)}>
              {models.map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
            <button className="btn primary" onClick={startWith}><Icon name="play" size={14} /> Avvia</button>
          </div>
        </div>
      )}
    </Drawer>
  );
}
