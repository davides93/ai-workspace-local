import { pickRuntime } from "../../models/model/profile";
import { useMemo, useState } from "react";
import { useUiMode } from "../../../app/useUiMode";
import { canReason, effectiveThinking, isThinkingMode, THINKING_HINT, THINKING_LABEL, THINKING_MODES } from "../../../shared/lib/thinking";
import { STYLE_PRESETS, styleOf, withStyle, type StyleChoice } from "../model/style";
import { Button, Dialog, Field, Input, Select, StatusPill } from "../../../shared/ui";
import { RoutePanel } from "./RoutePanel";
import { formatBytes, formatContext } from "../../../shared/lib/format";
import type { AdapterRef, InferenceProfile, LocalModel, RuntimeInfo } from "../../../shared/api/types";
import {
  AUTO_RUNTIME, blankProfile, chatRuntimes, compatibleModels, CONTEXT_PRESETS, hasProfileErrors, hiddenProfileErrors, idForProfile, modelAfterRuntimeChange, normalizeProfile, validateProfile,
  type ProfileIssue,
} from "../model/profiles";

import { t } from "../../../shared/i18n";
const CUSTOM = "__custom__";
const err = (issues: ProfileIssue[], field: ProfileIssue["field"]) => issues.find((i) => i.field === field && i.level === "error")?.message;
const warn = (issues: ProfileIssue[], field: ProfileIssue["field"]) => issues.find((i) => i.field === field && i.level === "warn")?.message;

/** Number field that keeps what is typed while it is not a valid number yet. */
const num = (v: number | undefined) => (v === undefined || Number.isNaN(v) ? "" : v);
const parse = (s: string): number | undefined => (s === "" ? undefined : Number(s));

const modelLabel = (m: LocalModel) => `${m.name}${m.quant ? ` · ${m.quant}` : ""} · ${formatBytes(m.size_bytes)}`;

/** Create or edit an inference profile: runtime, a model the runtime can serve, generation parameters. */
export function ProfileDialog({ profile, profiles, runtimes, models, saving, onSave, onClose }: {
  /** `null` creates a new profile. */
  profile: InferenceProfile | null;
  profiles: InferenceProfile[];
  runtimes: RuntimeInfo[];
  models: LocalModel[];
  saving: boolean;
  onSave: (p: InferenceProfile) => void;
  onClose: () => void;
}) {
  const isNew = profile === null;
  const [uiMode] = useUiMode();
  const developer = uiMode === "developer";
  const usable = useMemo(() => chatRuntimes(runtimes), [runtimes]);
  const [draft, setDraft] = useState<InferenceProfile>(() => {
    if (profile) return structuredClone(profile);
    const rt = usable.find((r) => r.status === "running" || r.status === "available") ?? usable[0];
    return blankProfile(rt?.id ?? "", compatibleModels(models, rt?.id ?? "")[0]?.id ?? "");
  });
  const fits = useMemo(() => compatibleModels(models, draft.runtime_id), [models, draft.runtime_id]);
  const [custom, setCustom] = useState(() => draft.model !== "" && !fits.some((m) => m.id === draft.model));
  const showCustom = custom || fits.length === 0;
  const all = validateProfile(draft, { runtimes, models });
  const [initial] = useState(() => JSON.stringify(draft));
  // Errors show up once the form has been touched; the button stays off until it is valid.
  const issues = JSON.stringify(draft) === initial ? all.filter((i) => i.level === "warn") : all;
  const runtime = runtimes.find((r) => r.id === draft.runtime_id);
  const auto = draft.runtime_id === AUTO_RUNTIME;
  const set = (patch: Partial<InferenceProfile>) => setDraft((d) => ({ ...d, ...patch }));
  const setParam = (patch: Partial<InferenceProfile["params"]>) => setDraft((d) => ({ ...d, params: { ...d.params, ...patch } }));

  /** The first adapter of the profile (others set through the API stay as they are). */
  const setAdapter = (patch: Partial<AdapterRef>) =>
    setDraft((d) => {
      const [first, ...rest] = d.params.adapters ?? [];
      return { ...d, params: { ...d.params, adapters: [{ ...(first ?? { path: "" }), ...patch }, ...rest] } };
    });

  const availableModels = models.filter((model) => pickRuntime(model) !== null);
  const chosenModel = availableModels.find((model) => model.id === draft.model);
  const style = styleOf(draft.params);
  const thinking = effectiveThinking(undefined, draft.params.thinking);
  const hiddenErrors = hiddenProfileErrors(all);
  const changeModel = (id: string) => {
    const model = availableModels.find((item) => item.id === id);
    const runtimeId = model && (draft.runtime_id === AUTO_RUNTIME ? AUTO_RUNTIME : pickRuntime(model, draft.runtime_id));
    if (!runtimeId) return;
    set({ model: id, runtime_id: runtimeId });
    setCustom(false);
  };

  const changeRuntime = (runtime_id: string) => {
    const nextFits = compatibleModels(models, runtime_id);
    const keepCustom = custom && draft.model !== "" && !nextFits.some((m) => m.id === draft.model);
    set({ runtime_id, model: keepCustom ? draft.model : modelAfterRuntimeChange(draft.model, models, runtime_id) });
    setCustom(keepCustom);
  };
  const submit = () => {
    if (hasProfileErrors(all)) return;
    const out = normalizeProfile(draft);
    onSave(isNew ? { ...out, id: idForProfile(out.name, profiles) } : out);
  };

  return (
    <Dialog
      title={isNew ? t("agents.nuovo_profilo_di_inferenza") : t("agents.modifica", { name: profile.name })} size="lg" onClose={onClose}
      footer={<>
        <Button onClick={onClose}>{t("agents.annulla")}</Button>
        <Button variant="primary" onClick={submit} disabled={saving || hasProfileErrors(all)}>{saving ? t("agents.salvataggio") : isNew ? t("agents.crea_profilo") : t("agents.salva")}</Button>
      </>}
    >
      <form className="form-grid" onSubmit={(e) => { e.preventDefault(); submit(); }} noValidate>
        <div className="cols-2">
          <Field label={t("agents.nome")} htmlFor="pf-name" error={err(issues, "name")}>
            <Input id="pf-name" data-autofocus value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder={t("agents.veloce")} autoComplete="off" aria-invalid={!!err(issues, "name")} />
          </Field>
          <Field label={t("agents.descrizione")} htmlFor="pf-desc" hint={t("agents.facoltativa")}>
            <Input id="pf-desc" value={draft.description ?? ""} onChange={(e) => set({ description: e.target.value })} placeholder={t("agents.gemma_4b_risposte_rapide")} autoComplete="off" />
          </Field>
        </div>

        <Field label={t("agents.modello_disponibile_qui")} htmlFor="pf-model-simple" error={developer ? undefined : err(issues, "model")} hint={chosenModel ? t("agents.runtime", { p: runtime?.name ?? draft.runtime_id }) : undefined}>
          <Select id="pf-model-simple" value={chosenModel?.id ?? ""} onChange={(event) => changeModel(event.target.value)}>
            <option value="">{draft.model && !chosenModel ? draft.model : t("agents.scegli_un_modello")}</option>
            {availableModels.map((model) => <option key={model.id} value={model.id}>{model.name}</option>)}
          </Select>
        </Field>
        <Field label={t("agents.stile_di_risposta")} htmlFor="pf-style" hint={STYLE_PRESETS.find((p) => p.id === style)?.hint}>
          <Select id="pf-style" value={style} onChange={(event) => setDraft((d) => ({ ...d, params: withStyle(d.params, event.target.value as StyleChoice) }))}>
            <option value="default">{t("agents.predefinito_del_modello")}</option>
            {STYLE_PRESETS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
            <option value="custom" disabled>{t("agents.personalizzato")}</option>
          </Select>
        </Field>
        {(canReason(chosenModel) || draft.params.thinking !== undefined) && (
          <Field label={t("agents.ragionamento")} htmlFor="pf-thinking" hint={THINKING_HINT[thinking]}>
            <Select id="pf-thinking" value={thinking} onChange={(event) => isThinkingMode(event.target.value) && setParam({ thinking: event.target.value })}>
              {THINKING_MODES.map((m) => <option key={m} value={m}>{THINKING_LABEL[m]}</option>)}
            </Select>
          </Field>
        )}
        {!developer && hiddenErrors.length > 0 && (
          <p className="field-error" role="alert">{t("agents.questo_valore_si_corregge_nella", { message: hiddenErrors[0].message })}</p>
        )}

        {developer && <details open={!chosenModel && !availableModels.length || undefined}>
          <summary>{t("agents.runtime_e_parametri_avanzati")}</summary>
        <div className="cols-2">
          <Field label={t("agents.runtime_2")} htmlFor="pf-runtime" error={err(issues, "runtime_id")} hint={auto ? t("agents.alla_partenza_aiw_sceglie_il") : runtime ? <span className="row"><StatusPill status={runtime.status} /><span>{runtime.status_detail ?? ""}</span></span> : undefined}>
            <Select id="pf-runtime" value={draft.runtime_id} onChange={(e) => changeRuntime(e.target.value)} aria-invalid={!!err(issues, "runtime_id")}>
              {!draft.runtime_id && <option value="">{t("agents.scegli_un_runtime")}</option>}
              <option value={AUTO_RUNTIME}>{t("agents.automatico_lo_sceglie_aiw")}</option>
              {draft.runtime_id && !runtime && <option value={draft.runtime_id}>{t("agents.non_registrato", { runtime_id: draft.runtime_id })}</option>}
              {usable.map((r) => <option key={r.id} value={r.id}>{r.name} · {r.kind}</option>)}
            </Select>
          </Field>
          <Field
            label={t("agents.modello")} htmlFor="pf-model" error={err(issues, "model")} hint={warn(issues, "model") ?? (fits.length === 0 ? t("agents.questo_runtime_non_elenca_modelli") : t("agents.compatibili_con_il_runtime", { length: fits.length }))}
          >
            {showCustom ? (
              <div className="copyfield-row">
                <Input id="pf-model" mono value={draft.model} onChange={(e) => set({ model: e.target.value })} placeholder={t("agents.id_del_modello")} autoComplete="off" aria-invalid={!!err(issues, "model")} />
                {fits.length > 0 && <Button size="sm" onClick={() => { setCustom(false); set({ model: modelAfterRuntimeChange("", models, draft.runtime_id) }); }}>{t("agents.dall_elenco")}</Button>}
              </div>
            ) : (
              <Select id="pf-model" value={draft.model} onChange={(e) => (e.target.value === CUSTOM ? (setCustom(true), set({ model: "" })) : set({ model: e.target.value }))} aria-invalid={!!err(issues, "model")}>
                {draft.model === "" && <option value="">{t("agents.scegli_un_modello")}</option>}
                {fits.map((m) => <option key={m.id} value={m.id}>{modelLabel(m)}</option>)}
                <option value={CUSTOM}>{t("agents.altro_scrivi_l_id")}</option>
              </Select>
            )}
          </Field>
        </div>
        {auto && (
          <div className="stack">
            <RoutePanel profile={draft} />
            <label className="row">
              <input type="checkbox" checked={draft.params.allow_remote === true} onChange={(e) => setParam({ allow_remote: e.target.checked ? true : undefined })} />
              <span>{t("agents.consenti_un_dispositivo_accoppiato_o")}</span>
            </label>
          </div>
        )}

        <fieldset className="param-set">
          <legend className="field-label">{t("agents.parametri_di_generazione")}</legend>
          <div className="param-grid">
            <Field label={t("agents.temperatura")} htmlFor="pf-temp" error={err(issues, "temperature")} hint={t("agents.0_prevedibile_2_molto_creativo")}>
              <Input id="pf-temp" className="num" type="number" step={0.05} min={0} max={2} value={num(draft.params.temperature)} onChange={(e) => setParam({ temperature: parse(e.target.value) })} />
            </Field>
            <Field label={t("agents.contesto_token")} htmlFor="pf-ctx" error={err(issues, "context")} hint={warn(issues, "context") ?? (draft.params.context ? t("agents.circa", { p: formatContext(draft.params.context) }) : t("agents.predefinito_del_runtime"))}>
              <Input id="pf-ctx" className="num" type="number" step={1024} min={512} list="pf-ctx-presets" value={num(draft.params.context)} onChange={(e) => setParam({ context: parse(e.target.value) })} />
              <datalist id="pf-ctx-presets">{CONTEXT_PRESETS.map((c) => <option key={c} value={c} />)}</datalist>
            </Field>
            <Field label={t("agents.risposta_massima_token")} htmlFor="pf-max" error={err(issues, "max_tokens")} hint={warn(issues, "max_tokens")}>
              <Input id="pf-max" className="num" type="number" step={256} min={1} value={num(draft.params.max_tokens)} onChange={(e) => setParam({ max_tokens: parse(e.target.value) })} />
            </Field>
            <Field label={t("agents.budget_di_ragionamento_token")} htmlFor="pf-tbudget" error={err(issues, "thinking_budget")} hint={t("agents.facoltativo_il_tetto_per_cio")}>
              <Input id="pf-tbudget" className="num" type="number" step={256} min={1} value={num(draft.params.thinking_budget)} onChange={(e) => setParam({ thinking_budget: parse(e.target.value) })} />
            </Field>
            <Field label={t("agents.top_p")} htmlFor="pf-topp" error={err(issues, "top_p")} hint={t("agents.facoltativo_da_0_a_1")}>
              <Input id="pf-topp" className="num" type="number" step={0.05} min={0} max={1} value={num(draft.params.top_p)} onChange={(e) => setParam({ top_p: parse(e.target.value) })} />
            </Field>
          </div>
        </fieldset>
        <Field
          label={t("agents.adapter_lora")} htmlFor="pf-adapter" error={err(issues, "adapters")}
          hint={auto ? t("agents.con_il_runtime_automatico_non") : runtime?.adapter_format ? runtime.adapter_format === "gguf" ? t("agents.percorso_di_un_file_gguf") : t("agents.percorso_della_cartella_dell_adapter") : t("agents.questo_runtime_non_carica_adapter")}
        >
          <div className="copyfield-row">
            <Input
              id="pf-adapter" mono value={draft.params.adapters?.[0]?.path ?? ""} disabled={auto || (!runtime?.adapter_format && !draft.params.adapters?.length)}
              onChange={(e) => setAdapter({ path: e.target.value })} placeholder={runtime?.adapter_format === "gguf" ? "/percorso/adapter.gguf" : "/percorso/adapter"} autoComplete="off" aria-invalid={!!err(issues, "adapters")}
            />
            <Input
              id="pf-adapter-scale" className="num" type="number" step={0.1} min={0} aria-label={t("agents.scala_dell_adapter")} placeholder={t("agents.scala")} disabled={!draft.params.adapters?.length}
              value={num(draft.params.adapters?.[0]?.scale)} onChange={(e) => setAdapter({ scale: parse(e.target.value) })}
            />
          </div>
        </Field>
        </details>}
        <button type="submit" className="sr-only" tabIndex={-1}>{t("agents.salva")}</button>
      </form>
    </Dialog>
  );
}
