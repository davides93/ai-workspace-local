import { useState } from "react";
import { Button, Dialog, Field, Input, Select } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { DatasetInfo, TrainDoctor, TrainMethod } from "../../../shared/api/types";
import { useEvalSuites } from "../../evals/api";
import { useModels } from "../../models/api";
import { useProfileChoices } from "../../workflows/api";
import { useStartTrainJob } from "../api";
import { blankJobForm, jobRequest, METHOD_HINT, METHOD_LABEL, METHODS, methodBlocker, readyTrainers, type JobForm } from "../model/training";

/** Which base model, dataset and knobs; the engine checks that the device, the trainer and the model fit. */
export function JobDialog({ doctor, datasets, onClose }: { doctor: TrainDoctor | undefined; datasets: DatasetInfo[]; onClose: () => void }) {
  const start = useStartTrainJob();
  const models = useModels();
  const suites = useEvalSuites();
  const profiles = useProfileChoices();
  const [f, setF] = useState<JobForm>(() => ({ ...blankJobForm(), dataset: datasets[0]?.id ?? "" }));
  const [shown, setShown] = useState<string | undefined>();
  const set = <K extends keyof JobForm>(k: K, v: JobForm[K]) => setF((x) => ({ ...x, [k]: v }));
  const forMethod = datasets.filter((d) => d.method === f.method);
  const trainers = readyTrainers(doctor, f.method);
  const blocker = methodBlocker(doctor, f.method);
  const local = (models.data ?? []).filter((m) => m.format !== "gguf");
  const changeMethod = (method: TrainMethod) =>
    setF((x) => ({ ...x, method, trainer: "", dataset: datasets.find((d) => d.method === method)?.id ?? "" }));
  const submit = () => {
    const r = jobRequest(f);
    setShown(r.error);
    if (r.req) start.mutate(r.req, { onSuccess: onClose });
  };
  return (
    <Dialog
      title="Nuovo addestramento" size="md" onClose={onClose}
      footer={<><Button onClick={onClose}>Annulla</Button><Button variant="primary" icon="bolt" onClick={submit} disabled={start.isPending || !!blocker}>Avvia</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <Field label="Metodo" htmlFor="tr-method" hint={METHOD_HINT[f.method]}>
          <Select id="tr-method" value={f.method} onChange={(e) => changeMethod(e.target.value as TrainMethod)}>
            {METHODS.map((m) => <option key={m} value={m}>{METHOD_LABEL[m]}</option>)}
          </Select>
        </Field>
        {blocker && <p className="muted text-sm" role="status">{blocker}</p>}
        <Field label="Trainer" htmlFor="tr-trainer" hint="Se non scegli, uso il primo pronto che conosce il metodo.">
          <Select id="tr-trainer" value={f.trainer} onChange={(e) => set("trainer", e.target.value)}>
            <option value="">Automatico</option>
            {trainers.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </Select>
        </Field>
        <Field label="Modello base" htmlFor="tr-base" hint="Un modello locale (non GGUF) o un repository Hugging Face, che il trainer scarica.">
          <Input id="tr-base" list="tr-base-list" value={f.base} onChange={(e) => set("base", e.target.value)} placeholder="Qwen/Qwen3-4B" autoComplete="off" mono />
          <datalist id="tr-base-list">{local.map((m) => <option key={m.id} value={m.name} />)}</datalist>
        </Field>
        <Field label="Dataset" htmlFor="tr-dataset" error={forMethod.length === 0 ? `Nessun dataset ${METHOD_LABEL[f.method]}: creane uno dalla scheda Dataset.` : undefined}>
          <Select id="tr-dataset" value={f.dataset} onChange={(e) => set("dataset", e.target.value)}>
            {forMethod.map((d) => <option key={d.id} value={d.id}>{d.name} · {d.examples} esempi</option>)}
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
              {(profiles.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </Select>
          </Field>
        )}
        {(shown || start.isError) && <p className="tr-error" role="alert">{shown ?? errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>Avvia</button>
      </form>
    </Dialog>
  );
}
