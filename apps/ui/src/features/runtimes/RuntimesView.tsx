import { useMemo, useState } from "react";
import { navigate, useParams } from "../../app/router";
import { useSystem } from "../../shared/query/system";
import { Button, Chip, Drawer, ErrorState, Icon, InlineText, Page, PageHeader, SchemaForm, Section, Segmented, StatusPill, Toggle } from "../../shared/ui";
import type { RuntimeInfo, RuntimeKind } from "../../shared/api/types";
import { useModels } from "../models/api";
import { useRefreshRuntimes, useRuntimeAction, useRuntimeLogs, useRuntimes, useUpdateRuntimeSettings } from "./api";

const KIND_META: Record<RuntimeKind, { title: string; desc: string }> = {
  sidecar: { title: "Sidecar gestiti", desc: "Processi avviati, monitorati e fermati dall'app; parlano HTTP compatibile OpenAI." },
  embedded: { title: "Embedded", desc: "Librerie collegate in-process via FFI: nessun processo esterno; sono il default su mobile." },
  system: { title: "Modelli di sistema", desc: "Modelli già presenti nel sistema operativo, condivisi tra le app." },
  remote: { title: "Remoti in LAN", desc: "Runtime di altri dispositivi AIW o server compatibili nella tua rete." },
};
const ORDER: RuntimeKind[] = ["sidecar", "embedded", "system", "remote"];

export function RuntimesView() {
  const rts = useRuntimes({ poll: true });
  const models = useModels();
  const system = useSystem();
  const refresh = useRefreshRuntimes();
  const action = useRuntimeAction();
  const [filter, setFilter] = useState<"here" | "all">("here");
  const runtimeId = useParams().runtimeId;
  const editing = rts.data?.find((r) => r.id === runtimeId) ?? null;

  const deviceAccels = useMemo(() => new Set(system.data?.device.accelerators.map((a) => a.kind) ?? []), [system.data]);
  const list = (rts.data ?? []).filter((r) => filter === "all" || r.status !== "unsupported");
  const counts = {
    running: rts.data?.filter((r) => r.status === "running").length ?? 0,
    available: rts.data?.filter((r) => r.status !== "unsupported").length ?? 0,
    total: rts.data?.length ?? 0,
  };
  const busyId = action.isPending ? action.variables?.runtime.id : undefined;

  return (
    <Page>
      <PageHeader
        eyebrow="Inferenza" title="Runtime"
        subtitle={`${counts.running} attivi · ${counts.available} utilizzabili qui · ${counts.total} nel catalogo`}
        actions={<>
          <Segmented label="Filtro" value={filter} onChange={setFilter} options={[{ id: "here", label: "Su questo device" }, { id: "all", label: "Tutto il catalogo" }]} />
          <Button icon="refresh" onClick={() => refresh.mutate()} disabled={refresh.isPending}>{refresh.isPending ? "Rilevo…" : "Rileva"}</Button>
        </>}
      />
      {rts.isError && !rts.data && <ErrorState error={rts.error} onRetry={() => void rts.refetch()} />}
      {ORDER.map((kind) => {
        const items = list.filter((r) => r.kind === kind);
        if (items.length === 0) return null;
        return (
          <Section key={kind} title={KIND_META[kind].title} hint={KIND_META[kind].desc}>
            <div className="grid">
              {items.map((r) => (
                <RuntimeCard key={r.id} r={r} deviceAccels={deviceAccels} busy={busyId === r.id}
                  onAct={(a) => action.mutate({ runtime: r, action: a })} onEdit={() => navigate(`/runtimes/${encodeURIComponent(r.id)}`)} />
              ))}
            </div>
          </Section>
        );
      })}
      <p className="faint text-sm">
        Ogni runtime è un manifest JSON. Per aggiungerne uno basta creare un file in <code>~/.aiw/runtimes/</code>: compare qui con le sue impostazioni.
      </p>
      {editing && (
        <RuntimeDrawer key={editing.id} r={editing} models={models.data?.filter((m) => m.runtimes.includes(editing.id)).map((m) => m.id) ?? []} onClose={() => navigate("/runtimes")} />
      )}
    </Page>
  );
}

function RuntimeCard({ r, deviceAccels, busy, onAct, onEdit }: {
  r: RuntimeInfo; deviceAccels: Set<string>; busy: boolean; onAct: (a: "start" | "stop") => void; onEdit: () => void;
}) {
  const dim = r.status === "unsupported";
  const pathLike = !!r.status_detail && (r.status_detail.includes("/") || r.status_detail.includes("\\"));
  return (
    <article className={`rt-card ${r.status === "running" ? "is-running" : ""} ${dim ? "is-dim" : ""}`}>
      <div className="rt-top">
        <div className="t">
          <b>{r.name}</b>
          <span className="faint text-xs">{r.vendor}</span>
        </div>
        <span className="kind-badge">{r.kind}</span>
      </div>
      <div className="row">
        <StatusPill status={r.status} />
        {r.status_detail && r.status !== "installable" && (
          <span className={`faint mono ellipsis text-xs maxw-full ${pathLike ? "dir-rtl" : ""}`} title={r.status_detail}>{r.status_detail}</span>
        )}
      </div>
      <p className="rt-desc"><InlineText text={r.description} /></p>
      <div className="chips" aria-label="Acceleratori">
        {r.accelerators.map((a) => <Chip key={a} hot={deviceAccels.has(a)} title={deviceAccels.has(a) ? "Presente su questo device" : undefined}>{a}</Chip>)}
        {r.formats.map((f) => <Chip key={f}>.{f}</Chip>)}
      </div>
      {r.loaded_models.length > 0 && <div className="mono faint ellipsis text-xs">caricato: {r.loaded_models.join(", ")}</div>}
      <div className="rt-foot">
        {r.status === "running" || r.status === "starting" ? (
          <Button icon="stop" disabled={busy} onClick={() => onAct("stop")}>Ferma</Button>
        ) : r.status === "installable" ? (
          <span className="mono faint ellipsis text-xs" title={r.install_hint}>{r.install_hint ?? "installa dal sito del progetto"}</span>
        ) : r.status !== "unsupported" ? (
          <Button variant="primary" icon="play" disabled={busy} onClick={() => onAct("start")}>{busy ? "Avvio…" : "Avvia"}</Button>
        ) : (
          <span className="faint text-xs">Disponibile su: {r.platforms.join(", ")}</span>
        )}
        <Button variant="ghost" className="ml-auto" icon="settings" onClick={onEdit}>Impostazioni</Button>
      </div>
    </article>
  );
}

function RuntimeDrawer({ r, models, onClose }: { r: RuntimeInfo; models: string[]; onClose: () => void }) {
  const update = useUpdateRuntimeSettings();
  const action = useRuntimeAction();
  const [value, setValue] = useState<Record<string, unknown>>(r.settings);
  const [advanced, setAdvanced] = useState(false);
  const [model, setModel] = useState(models[0] ?? "");
  const logs = useRuntimeLogs(r.id, r.status === "running");
  const save = () => update.mutate({ id: r.id, settings: value }, { onSuccess: onClose });
  const startWith = () => update.mutate({ id: r.id, settings: value }, { onSuccess: () => action.mutate({ runtime: r, action: "start", model }, { onSuccess: onClose }) });
  return (
    <Drawer title={r.name} subtitle={`${r.kind} · API ${r.api} · ${r.platforms.join(" / ")}`} onClose={onClose}
      footer={<>
        <Button onClick={onClose}>Annulla</Button>
        <Button variant="primary" onClick={save} disabled={update.isPending}>Salva</Button>
      </>}>
      <dl className="kv">
        <dt>Stato</dt><dd><StatusPill status={r.status} /></dd>
        <dt>Endpoint</dt><dd className="mono">{r.endpoint ?? "—"}</dd>
        <dt>Capacità</dt><dd>{r.capabilities.join(", ")}</dd>
        {r.homepage && <><dt>Progetto</dt><dd><a href={r.homepage} target="_blank" rel="noreferrer">{r.homepage.replace(/^https?:\/\//, "")}</a></dd></>}
        {r.install_hint && <><dt>Installazione</dt><dd className="mono">{r.install_hint}</dd></>}
      </dl>
      <div className="stack">
        <div className="row row-between">
          <h3>Impostazioni</h3>
          <span className="row text-sm"><Toggle label="Impostazioni avanzate" checked={advanced} onChange={setAdvanced} /> Avanzate</span>
        </div>
        <SchemaForm idPrefix={`rt-${r.id}`} schema={r.settings_schema} value={value} onChange={setValue} showAdvanced={advanced} />
      </div>
      {(logs.data?.length ?? 0) > 0 && (
        <div className="stack">
          <h3>Log del processo</h3>
          <pre className="mono logbox">{logs.data!.slice(-60).join("\n")}</pre>
        </div>
      )}
      {models.length > 0 && r.status !== "unsupported" && (
        <div className="stack">
          <h3>Avvia con un modello</h3>
          <div className="row nowrap">
            <select className="input grow" aria-label="Modello" value={model} onChange={(e) => setModel(e.target.value)}>
              {models.map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
            <Button variant="primary" onClick={startWith} disabled={update.isPending || action.isPending}><Icon name="play" size={14} /> Avvia</Button>
          </div>
        </div>
      )}
    </Drawer>
  );
}
