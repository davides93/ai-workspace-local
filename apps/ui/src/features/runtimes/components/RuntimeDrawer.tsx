import { useState } from "react";
import { Link } from "../../../app/router";
import type { LocalModel, RuntimeInfo } from "../../../shared/api/types";
import { Button, Chip, Chips, CopyField, Drawer, Field, KeyValue, Pill, SchemaForm, Select, StatusPill, Toggle } from "../../../shared/ui";
import { useRuntimeAction, useUpdateRuntimeSettings } from "../api";
import { apiLabel, compatibleModels, FIT_META, settingsDirty } from "../model/runtimes";
import { RuntimeLogs } from "./RuntimeLogs";

/** `#/runtimes/:runtimeId` — state, start with a model, settings and logs of one runtime. */
export function RuntimeDrawer({ runtime: r, models, deviceAccels, onClose }: {
  runtime: RuntimeInfo;
  /** Installed models; the ones this runtime can run are offered when starting. */
  models: LocalModel[];
  deviceAccels: Set<string>;
  onClose: () => void;
}) {
  const update = useUpdateRuntimeSettings();
  const action = useRuntimeAction();
  const [draft, setDraft] = useState<Record<string, unknown>>(r.settings);
  const [advanced, setAdvanced] = useState(false);
  const options = compatibleModels(models, r.id);
  const [modelId, setModelId] = useState(options[0]?.id ?? "");
  const chosen = options.find((m) => m.id === modelId) ?? options[0];
  const dirty = settingsDirty(r.settings_schema, r.settings, draft);
  const active = r.status === "running" || r.status === "starting";
  const canStart = !active && r.status !== "unsupported" && r.status !== "installable";
  const pending = update.isPending || action.isPending;

  const save = () => update.mutate({ id: r.id, settings: draft });
  const start = async () => {
    // Settings first, so the process starts with what the form shows.
    if (dirty) {
      try {
        await update.mutateAsync({ id: r.id, settings: draft });
      } catch {
        return; // the failure is already shown as a toast
      }
    }
    action.mutate({ runtime: r, action: "start", model: chosen?.id });
  };

  return (
    <Drawer
      title={r.name} subtitle={`${r.kind} · ${apiLabel(r.api)} · ${r.platforms.join(" / ")}`} onClose={onClose}
      footer={<>
        {dirty && <span className="faint text-sm grow">Modifiche non salvate</span>}
        <Button onClick={onClose}>{dirty ? "Annulla" : "Chiudi"}</Button>
        <Button variant="primary" onClick={save} disabled={!dirty || pending}>{update.isPending ? "Salvo…" : "Salva impostazioni"}</Button>
      </>}
    >
      <section className="stack" aria-label="Stato">
        <KeyValue items={[
          ["Stato", <span className="row"><StatusPill status={r.status} />{r.status_detail && r.status !== "installable" && <span className="faint text-xs">{r.status_detail}</span>}</span>],
          r.endpoint ? ["Endpoint", <span className="mono">{r.endpoint}</span>] : false,
          ["Capacità", <Chips>{r.capabilities.map((c) => <Chip key={c}>{c}</Chip>)}</Chips>],
          ["Acceleratori", <Chips>{r.accelerators.map((a) => <Chip key={a} hot={deviceAccels.has(a)}>{a}</Chip>)}</Chips>],
          r.loaded_models.length > 0 ? ["Caricato", <span className="mono">{r.loaded_models.join(", ")}</span>] : false,
          r.homepage ? ["Progetto", <a href={r.homepage} target="_blank" rel="noreferrer">{r.homepage.replace(/^https?:\/\//, "")}</a>] : false,
        ]} />
        {r.install_hint && <CopyField label="Installazione" value={r.install_hint} />}
      </section>

      <section className="stack" aria-label="Avvio">
        <h3>{active ? "In esecuzione" : "Avvia con un modello"}</h3>
        {active ? (
          <div className="row">
            <Button icon="stop" disabled={pending} onClick={() => action.mutate({ runtime: r, action: "stop" })}>Ferma {r.name}</Button>
            {r.status === "starting" && <span className="faint text-sm">Avvio in corso…</span>}
          </div>
        ) : !canStart ? (
          <p className="faint text-sm">{r.status === "installable" ? "Installa il runtime per poterlo avviare." : "Questo runtime non è utilizzabile su questo device."}</p>
        ) : options.length === 0 ? (
          <p className="muted text-sm">Nessun modello installato è compatibile con questo runtime. <Link to="/models/discover">Cerca un modello</Link> adatto ai suoi formati ({r.formats.map((f) => `.${f}`).join(", ") || "—"}).</p>
        ) : (
          <div className="rt-start">
            <Field label="Modello" htmlFor="rt-model" className="grow">
              <Select id="rt-model" value={chosen?.id ?? ""} onChange={(e) => setModelId(e.target.value)}>
                {options.map((m) => <option key={m.id} value={m.id}>{m.name}{m.quant ? ` · ${m.quant}` : ""}</option>)}
              </Select>
            </Field>
            <Button variant="primary" icon="play" onClick={() => void start()} disabled={pending}>{action.isPending ? "Avvio…" : dirty ? "Salva e avvia" : "Avvia"}</Button>
            {chosen?.fit && <p className="rt-fit faint text-xs"><Pill tone={FIT_META[chosen.fit].tone}>{FIT_META[chosen.fit].label}</Pill> {chosen.fit_detail}</p>}
          </div>
        )}
      </section>

      <section className="stack" aria-label="Impostazioni">
        <div className="row row-between">
          <h3>Impostazioni</h3>
          <span className="row text-sm">
            <Toggle label="Mostra le impostazioni avanzate" checked={advanced} onChange={setAdvanced} /> Avanzate
          </span>
        </div>
        <SchemaForm idPrefix={`rt-${r.id}`} schema={r.settings_schema} value={draft} onChange={setDraft} showAdvanced={advanced} />
        {Object.keys(r.settings_schema.properties).length > 0 && (
          <div>
            <Button variant="ghost" size="sm" icon="refresh" onClick={() => setDraft({})} disabled={Object.keys(draft).length === 0}>Ripristina i predefiniti</Button>
          </div>
        )}
      </section>

      <RuntimeLogs runtimeId={r.id} />
    </Drawer>
  );
}
