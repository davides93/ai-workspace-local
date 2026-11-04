import { useMemo, useState } from "react";
import { Button, Dialog, Field, Input, Select, StatusPill } from "../../../shared/ui";
import { formatBytes, formatContext } from "../../../shared/lib/format";
import type { InferenceProfile, LocalModel, RuntimeInfo } from "../../../shared/api/types";
import {
  blankProfile, chatRuntimes, compatibleModels, CONTEXT_PRESETS, hasProfileErrors, idForProfile, modelAfterRuntimeChange, normalizeProfile, validateProfile,
  type ProfileIssue,
} from "../model/profiles";

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
  const set = (patch: Partial<InferenceProfile>) => setDraft((d) => ({ ...d, ...patch }));
  const setParam = (patch: Partial<InferenceProfile["params"]>) => setDraft((d) => ({ ...d, params: { ...d.params, ...patch } }));

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
      title={isNew ? "Nuovo profilo di inferenza" : `Modifica «${profile.name}»`} size="lg" onClose={onClose}
      footer={<>
        <Button onClick={onClose}>Annulla</Button>
        <Button variant="primary" onClick={submit} disabled={saving || hasProfileErrors(all)}>{saving ? "Salvataggio…" : isNew ? "Crea profilo" : "Salva"}</Button>
      </>}
    >
      <form className="form-grid" onSubmit={(e) => { e.preventDefault(); submit(); }} noValidate>
        <div className="cols-2">
          <Field label="Nome" htmlFor="pf-name" error={err(issues, "name")}>
            <Input id="pf-name" data-autofocus value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder="Veloce" autoComplete="off" aria-invalid={!!err(issues, "name")} />
          </Field>
          <Field label="Descrizione" htmlFor="pf-desc" hint="Facoltativa">
            <Input id="pf-desc" value={draft.description ?? ""} onChange={(e) => set({ description: e.target.value })} placeholder="Gemma 4B, risposte rapide" autoComplete="off" />
          </Field>
        </div>

        <div className="cols-2">
          <Field label="Runtime" htmlFor="pf-runtime" error={err(issues, "runtime_id")} hint={runtime ? <span className="row"><StatusPill status={runtime.status} /><span>{runtime.status_detail ?? ""}</span></span> : undefined}>
            <Select id="pf-runtime" value={draft.runtime_id} onChange={(e) => changeRuntime(e.target.value)} aria-invalid={!!err(issues, "runtime_id")}>
              {!draft.runtime_id && <option value="">Scegli un runtime…</option>}
              {draft.runtime_id && !runtime && <option value={draft.runtime_id}>{draft.runtime_id} (non registrato)</option>}
              {usable.map((r) => <option key={r.id} value={r.id}>{r.name} · {r.kind}</option>)}
            </Select>
          </Field>
          <Field
            label="Modello" htmlFor="pf-model" error={err(issues, "model")} hint={warn(issues, "model") ?? (fits.length === 0 ? "Questo runtime non elenca modelli installati: scrivi l'id del modello." : `${fits.length} compatibili con il runtime`)}
          >
            {showCustom ? (
              <div className="copyfield-row">
                <Input id="pf-model" mono value={draft.model} onChange={(e) => set({ model: e.target.value })} placeholder="id del modello" autoComplete="off" aria-invalid={!!err(issues, "model")} />
                {fits.length > 0 && <Button size="sm" onClick={() => { setCustom(false); set({ model: modelAfterRuntimeChange("", models, draft.runtime_id) }); }}>Dall'elenco</Button>}
              </div>
            ) : (
              <Select id="pf-model" value={draft.model} onChange={(e) => (e.target.value === CUSTOM ? (setCustom(true), set({ model: "" })) : set({ model: e.target.value }))} aria-invalid={!!err(issues, "model")}>
                {draft.model === "" && <option value="">Scegli un modello…</option>}
                {fits.map((m) => <option key={m.id} value={m.id}>{modelLabel(m)}</option>)}
                <option value={CUSTOM}>Altro… (scrivi l'id)</option>
              </Select>
            )}
          </Field>
        </div>

        <fieldset className="param-set">
          <legend className="field-label">Parametri di generazione</legend>
          <div className="param-grid">
            <Field label="Temperatura" htmlFor="pf-temp" error={err(issues, "temperature")} hint="0 = prevedibile, 2 = molto creativo">
              <Input id="pf-temp" className="num" type="number" step={0.05} min={0} max={2} value={num(draft.params.temperature)} onChange={(e) => setParam({ temperature: parse(e.target.value) })} />
            </Field>
            <Field label="Contesto (token)" htmlFor="pf-ctx" error={err(issues, "context")} hint={warn(issues, "context") ?? (draft.params.context ? `circa ${formatContext(draft.params.context)}` : "Predefinito del runtime")}>
              <Input id="pf-ctx" className="num" type="number" step={1024} min={512} list="pf-ctx-presets" value={num(draft.params.context)} onChange={(e) => setParam({ context: parse(e.target.value) })} />
              <datalist id="pf-ctx-presets">{CONTEXT_PRESETS.map((c) => <option key={c} value={c} />)}</datalist>
            </Field>
            <Field label="Risposta massima (token)" htmlFor="pf-max" error={err(issues, "max_tokens")} hint={warn(issues, "max_tokens")}>
              <Input id="pf-max" className="num" type="number" step={256} min={1} value={num(draft.params.max_tokens)} onChange={(e) => setParam({ max_tokens: parse(e.target.value) })} />
            </Field>
            <Field label="Top-p" htmlFor="pf-topp" error={err(issues, "top_p")} hint="Facoltativo, da 0 a 1">
              <Input id="pf-topp" className="num" type="number" step={0.05} min={0} max={1} value={num(draft.params.top_p)} onChange={(e) => setParam({ top_p: parse(e.target.value) })} />
            </Field>
          </div>
        </fieldset>
        <button type="submit" className="sr-only" tabIndex={-1}>Salva</button>
      </form>
    </Dialog>
  );
}
