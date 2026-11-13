import { useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { Link } from "../../../app/router";
import type { LocalModel, RuntimeInfo } from "../../../shared/api/types";
import { Button, Chip, Chips, CopyField, Drawer, Field, KeyValue, Pill, SchemaForm, Select, StatusPill, Toggle } from "../../../shared/ui";
import { useRuntimeAction, useUpdateRuntimeSettings } from "../api";
import { apiLabel, compatibleModels, FIT_META, settingsDirty } from "../model/runtimes";
import { RuntimeInstall } from "./RuntimeInstall";
import { RuntimeLogs } from "./RuntimeLogs";

import { t, tNode } from "../../../shared/i18n";
/** `#/runtimes/:runtimeId` — state, start with a model, settings and logs of one runtime. */
export function RuntimeDrawer({ runtime: r, models, deviceAccels, onClose }: {
  runtime: RuntimeInfo;
  /** Installed models; the ones this runtime can run are offered when starting. */
  models: LocalModel[];
  deviceAccels: Set<string>;
  onClose: () => void;
}) {
  const update = useUpdateRuntimeSettings();
  const action = useRuntimeAction(true);
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
        {dirty && <span className="faint text-sm grow">{t("runtimes.modifiche_non_salvate")}</span>}
        <Button onClick={onClose}>{dirty ? t("runtimes.annulla") : t("runtimes.chiudi")}</Button>
        <Button variant="primary" onClick={save} disabled={!dirty || pending}>{update.isPending ? t("runtimes.salvo") : t("runtimes.salva_impostazioni")}</Button>
      </>}
    >
      <section className="stack" aria-label={t("runtimes.stato")}>
        <KeyValue items={[
          [t("runtimes.stato"), <span className="row"><StatusPill status={r.status} />{r.status_detail && r.status !== "installable" && <span className="faint text-xs">{r.status_detail}</span>}</span>],
          r.endpoint ? [t("runtimes.endpoint"), <span className="mono">{r.endpoint}</span>] : false,
          [t("runtimes.capacita"), <Chips>{r.capabilities.map((c) => <Chip key={c}>{c}</Chip>)}</Chips>],
          [t("runtimes.acceleratori"), <Chips>{r.accelerators.map((a) => <Chip key={a} hot={deviceAccels.has(a)}>{a}</Chip>)}</Chips>],
          r.loaded_models.length > 0 ? [t("runtimes.caricato_label"), <span className="mono">{r.loaded_models.join(", ")}</span>] : false,
          r.homepage ? [t("runtimes.progetto"), <a href={r.homepage} target="_blank" rel="noreferrer">{r.homepage.replace(/^https?:\/\//, "")}</a>] : false,
        ]} />
        {r.status !== "installable" && r.install_hint && <CopyField label={t("runtimes.installazione")} value={r.install_hint} />}
      </section>

      {(r.status === "installable" || r.id === "localai") && <RuntimeInstall runtime={r} />}

      <section className="stack" aria-label={t("runtimes.avvio_2")}>
        <h3>{active ? t("runtimes.in_esecuzione") : t("runtimes.avvia_con_un_modello")}</h3>
        {active && r.kind === "system" ? (
          <Link to="/models?origin=system">{t("runtimes.vedi_i_modelli_di_sistema")}</Link>
        ) : active ? (
          <div className="row">
            <Button icon="stop" disabled={pending} onClick={() => action.mutate({ runtime: r, action: "stop" })}>{t("runtimes.ferma_2", { name: r.name })}</Button>
            {r.status === "starting" && <span className="faint text-sm">{t("runtimes.avvio_in_corso")}</span>}
          </div>
        ) : !canStart ? (
          <p className="faint text-sm">{r.status === "installable" ? t("runtimes.installa_il_runtime_sopra_per") : t("runtimes.questo_runtime_non_e_utilizzabile")}</p>
        ) : options.length === 0 ? (
          <p className="muted text-sm">{tNode("runtimes.no_compatible_model", { link: <Link to="/models/discover">{t("runtimes.cerca_un_modello")}</Link> }, { formats: r.formats.map((f) => `.${f}`).join(", ") || "—" })}</p>
        ) : (
          <div className="rt-start">
            <Field label={t("runtimes.modello")} htmlFor="rt-model" className="grow">
              <Select id="rt-model" value={chosen?.id ?? ""} onChange={(e) => setModelId(e.target.value)}>
                {options.map((m) => <option key={m.id} value={m.id}>{m.name}{m.quant ? ` · ${m.quant}` : ""}</option>)}
              </Select>
            </Field>
            <Button variant="primary" icon="play" onClick={() => void start()} disabled={pending}>{action.isPending ? t("runtimes.avvio") : dirty ? t("runtimes.salva_e_avvia") : t("runtimes.avvia_2")}</Button>
            {chosen?.fit && <p className="rt-fit faint text-xs"><Pill tone={FIT_META[chosen.fit].tone}>{FIT_META[chosen.fit].label}</Pill> {chosen.fit_detail}</p>}
          </div>
        )}
      </section>

      <section className="stack" aria-label={t("runtimes.impostazioni")}>
        <div className="row row-between">
          <h3>{t("runtimes.impostazioni")}</h3>
          <span className="row text-sm">
            <Toggle label={t("runtimes.show_advanced_settings")} checked={advanced} onChange={setAdvanced} /> {t("runtimes.avanzate")}</span>
        </div>
        <SchemaForm idPrefix={`rt-${r.id}`} schema={r.settings_schema} value={draft} onChange={setDraft} showAdvanced={advanced} />
        {Object.keys(r.settings_schema.properties).length > 0 && (
          <div>
            <Button variant="ghost" size="sm" icon="refresh" onClick={() => setDraft({})} disabled={Object.keys(draft).length === 0}>{t("runtimes.ripristina_i_predefiniti")}</Button>
          </div>
        )}
      </section>

      {action.isError && <p role="alert" className="crit-text">{errorMessage(action.error)}</p>}
      <RuntimeLogs runtimeId={r.id} />
    </Drawer>
  );
}
