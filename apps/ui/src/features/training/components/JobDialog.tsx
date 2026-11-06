import { useState } from "react";
import { Link } from "../../../app/router";
import { Button, Dialog, Field, Input, Select } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { DatasetInfo, TrainDoctor, TrainMethod } from "../../../shared/api/types";
import { useEvalSuites } from "../../evals/api";
import { useModels } from "../../models/api";
import { useProfileChoices } from "../../workflows/api";
import { useBuildDataset, useInstallTrainer, useStartTrainJob } from "../api";
import { ImportSftExamples } from "./ImportSftExamples";
import { blankJobForm, jobRequest, METHOD_HINT, METHOD_LABEL, METHODS, methodBlocker, readyTrainers, type JobForm } from "../model/training";

/** Which base model, dataset and knobs; the engine checks that the device, the trainer and the model fit. */
export function JobDialog({ doctor, datasets, onClose }: { doctor: TrainDoctor | undefined; datasets: DatasetInfo[]; onClose: () => void }) {
  const start = useStartTrainJob();
  const models = useModels();
  const build = useBuildDataset();
  const install = useInstallTrainer();
  const [step, setStep] = useState(0);
  const steps = ["Obiettivo", "Dati", "Modello", "Confronto e avvio"];
  const suites = useEvalSuites();
  const profiles = useProfileChoices();
  const [f, setF] = useState<JobForm>(() => ({ ...blankJobForm(), dataset: datasets.find((d) => d.method === "sft")?.id ?? "" }));
  const [shown, setShown] = useState<string | undefined>();
  const set = <K extends keyof JobForm>(k: K, v: JobForm[K]) => setF((x) => ({ ...x, [k]: v }));
  const forMethod = datasets.filter((d) => d.method === f.method);
  const trainers = readyTrainers(doctor, f.method);
  const blocker = methodBlocker(doctor, f.method);
  const suggestedTrainer = trainers.find((t) => t.id === f.trainer) ?? trainers[0];
  const methodFit = suggestedTrainer?.methods.find((method) => method.method === f.method);
  const local = (models.data ?? []).filter((model) => {
    const format = suggestedTrainer?.id === "mlx-lm-lora" ? "mlx" : "safetensors";
    const limit = methodFit?.max_params_b;
    const compatible = model.format === format ||
      (format === "mlx" && model.runtime_compat?.some((runtime) => runtime.id === "mlx-lm"));
    return compatible && (!limit || !model.params_b || model.params_b <= limit * 1.1);
  });
  const missing = doctor?.trainers.find((t) => t.state === "missing" && t.methods.some((m) => m.method === f.method && !m.blocked));
  const changeMethod = (method: TrainMethod) =>
    setF((x) => ({ ...x, method, trainer: "", dataset: datasets.find((d) => d.method === method)?.id ?? "" }));
  const submit = () => {
    if (step < steps.length - 1) {
      if ((step === 1 && !f.dataset) || (step === 2 && !f.base.trim()) || blocker || !doctor) return;
      if (step === 2) {
        const draft = jobRequest(f);
        if (draft.error) { setShown(draft.error); return; }
      }
      setShown(undefined);
      setStep(step + 1);
      return;
    }
    const r = jobRequest(f);
    setShown(r.error);
    if (r.req && !blocker && doctor && !start.isPending) start.mutate(r.req, { onSuccess: onClose });
  };
  return (
    <Dialog
      title={`Nuovo addestramento · ${step + 1}/${steps.length} · ${steps[step]}`} size="md" onClose={onClose}
      footer={<>
        <Button onClick={step === 0 ? onClose : () => setStep(step - 1)}>{step === 0 ? "Annulla" : "Indietro"}</Button>
        <Button variant="primary" icon="bolt" onClick={submit}
          disabled={start.isPending || install.isPending || build.isPending || !!blocker || !doctor ||
            (step === 1 && !f.dataset) || (step === 2 && !f.base.trim())}>
          {step < steps.length - 1 ? "Continua" : "Avvia addestramento"}
        </Button>
      </>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        {step === 0 && <>
        <Field label="Metodo" htmlFor="tr-method" hint={METHOD_HINT[f.method]}>
          <Select id="tr-method" value={f.method} onChange={(e) => changeMethod(e.target.value as TrainMethod)}>
            {METHODS.map((m) => <option key={m} value={m}>{METHOD_LABEL[m]} · {METHOD_HINT[m]}</option>)}
          </Select>
        </Field>
        {blocker && <p className="muted text-sm" role="status">{blocker}</p>}
          {suggestedTrainer && <p className="text-sm">Trainer consigliato su questo dispositivo: <b>{suggestedTrainer.name}</b>.</p>}
          {missing && <Button onClick={() => install.mutate(missing.id)} disabled={install.isPending}>
            {install.isPending ? "Installazione in corso…" : `Installa ${missing.name}`}
          </Button>}
          {install.isError && <p className="tr-error" role="alert">{errorMessage(install.error)}</p>}
        </>}
        {step === 1 && <>
        <Field label="Dataset" htmlFor="tr-dataset" hint="Gli esempi di validazione vengono separati automaticamente.">
          <Select id="tr-dataset" value={f.dataset} onChange={(e) => set("dataset", e.target.value)}>
            <option value="">Scegli un dataset…</option>
            {forMethod.map((d) => <option key={d.id} value={d.id}>{d.name} · {d.examples} esempi</option>)}
          </Select>
        </Field>
          {f.method === "sft" && <Button disabled={build.isPending}
            onClick={() => build.mutate({ method: "sft", sources: ["feedback"] }, { onSuccess: (data) => set("dataset", data.id) })}>
            {build.isPending ? "Preparo il dataset…" : "Crea dai pollici in su"}
          </Button>}
          {f.method === "sft" && <ImportSftExamples onImported={(id) => set("dataset", id)} />}
          {build.isError && <p className="tr-error" role="alert">{errorMessage(build.error)}</p>}
          <Link to="/training/datasets" onClick={onClose}>Apri le opzioni dei dataset</Link>
        </>}
        {step === 2 && <>
        <Field label="Modello base" htmlFor="tr-base" hint={suggestedTrainer?.id === "mlx-lm-lora"
          ? "Scegli un modello compatibile con MLX LM. Per MedGemma il percorso usa la parte testuale."
          : "Scegli un modello safetensors locale o indica un repository Hugging Face."}>
          <Input id="tr-base" list="tr-base-list" value={f.base} onChange={(e) => setF((current) => ({ ...current, base: e.target.value, evalProfile: "" }))} placeholder="Qwen/Qwen3-4B" autoComplete="off" mono />
          <datalist id="tr-base-list">{local.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}</datalist>
        </Field>
          <details>
            <summary>Trainer e parametri avanzati</summary>
        <Field label="Trainer" htmlFor="tr-trainer" hint="Se non scegli, uso il primo pronto che conosce il metodo.">
          <Select id="tr-trainer" value={f.trainer} onChange={(e) => set("trainer", e.target.value)}>
            <option value="">Automatico</option>
            {trainers.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </Select>
        </Field>
        <Field label="Nome dell'adapter" htmlFor="tr-name" hint="Facoltativo.">
          <Input id="tr-name" value={f.name} onChange={(e) => set("name", e.target.value)} autoComplete="off" />
        </Field>
        <div className="tr-form-grid">
          <Field label="Passi" htmlFor="tr-iters"><Input id="tr-iters" inputMode="numeric" value={f.iters} onChange={(e) => set("iters", e.target.value)} placeholder="auto" /></Field>
          <Field label="Epoche" htmlFor="tr-epochs"><Input id="tr-epochs" inputMode="decimal" value={f.epochs} onChange={(e) => set("epochs", e.target.value)} placeholder="3" /></Field>
          <Field label="Rango LoRA" htmlFor="tr-rank"><Input id="tr-rank" inputMode="numeric" value={f.rank} onChange={(e) => set("rank", e.target.value)} placeholder="16" /></Field>
          <Field label="Learning rate" htmlFor="tr-lr"><Input id="tr-lr" inputMode="decimal" value={f.lr} onChange={(e) => set("lr", e.target.value)} placeholder="2e-4" /></Field>
          <Field label="Batch" htmlFor="tr-batch"><Input id="tr-batch" inputMode="numeric" value={f.batch} onChange={(e) => set("batch", e.target.value)} placeholder="auto" /></Field>
        </div>
          </details>
        </>}
        {step === 3 && <>
          <p className="text-sm"><b>{METHOD_LABEL[f.method]}</b> su <b>{f.base}</b> · {forMethod.find((d) => d.id === f.dataset)?.name}</p>
          <p className="faint text-sm">Al termine troverai l’adapter nella pagina Addestramento. Scegli una suite con esempi separati per confrontarlo con il modello base.</p>
        <Field label="Confronto a fine addestramento" htmlFor="tr-suite" hint="Esegue la suite con e senza l'adapter e ne riporta i risultati.">
          <Select id="tr-suite" value={f.evalSuite} onChange={(e) => set("evalSuite", e.target.value)}>
            <option value="">Nessuno</option>
            {(suites.data ?? []).map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </Select>
        </Field>
        {f.evalSuite && (
          <Field label="Profilo di partenza" htmlFor="tr-profile" hint="Il suo modello viene provato com'è e con l'adapter.">
            <Select id="tr-profile" value={f.evalProfile} onChange={(e) => set("evalProfile", e.target.value)}>
              <option value="">Scegli…</option>
              {(profiles.data ?? []).filter((p) => p.model === f.base && !p.params.adapters?.length).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </Select>
          </Field>
        )}
        </>}
        {(shown || start.isError) && <p className="tr-error" role="alert">{shown ?? errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>Avvia</button>
      </form>
    </Dialog>
  );
}
