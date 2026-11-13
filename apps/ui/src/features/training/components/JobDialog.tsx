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

import { t as tr, tNode as trNode } from "../../../shared/i18n";
/** Which base model, dataset and knobs; the engine checks that the device, the trainer and the model fit. */
export function JobDialog({ doctor, datasets, onClose }: { doctor: TrainDoctor | undefined; datasets: DatasetInfo[]; onClose: () => void }) {
  const start = useStartTrainJob();
  const models = useModels();
  const build = useBuildDataset();
  const install = useInstallTrainer();
  const [step, setStep] = useState(0);
  const steps = [tr("training.step_goal"), tr("training.step_data"), tr("training.step_model"), tr("training.step_compare")];
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
      title={tr("training.nuovo_addestramento", { step: step + 1, total: steps.length, title: steps[step] })} size="md" onClose={onClose}
      footer={<>
        <Button onClick={step === 0 ? onClose : () => setStep(step - 1)}>{step === 0 ? tr("training.annulla") : tr("training.indietro")}</Button>
        <Button variant="primary" icon="bolt" onClick={submit}
          disabled={start.isPending || install.isPending || build.isPending || !!blocker || !doctor ||
            (step === 1 && !f.dataset) || (step === 2 && !f.base.trim())}>
          {step < steps.length - 1 ? tr("training.continua") : tr("training.avvia_addestramento")}
        </Button>
      </>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        {step === 0 && <>
        <Field label={tr("training.metodo")} htmlFor="tr-method" hint={METHOD_HINT[f.method]}>
          <Select id="tr-method" value={f.method} onChange={(e) => changeMethod(e.target.value as TrainMethod)}>
            {METHODS.map((m) => <option key={m} value={m}>{METHOD_LABEL[m]} · {METHOD_HINT[m]}</option>)}
          </Select>
        </Field>
        {blocker && <p className="muted text-sm" role="status">{blocker}</p>}
          {suggestedTrainer && <p className="text-sm">{trNode("training.suggested_trainer", { name: <b>{suggestedTrainer.name}</b> })}</p>}
          {missing && <Button onClick={() => install.mutate(missing.id)} disabled={install.isPending}>
            {install.isPending ? tr("training.installazione_in_corso") : tr("training.installa_2", { name: missing.name })}
          </Button>}
          {install.isError && <p className="tr-error" role="alert">{errorMessage(install.error)}</p>}
        </>}
        {step === 1 && <>
        <Field label={tr("training.dataset")} htmlFor="tr-dataset" hint={tr("training.gli_esempi_di_validazione_vengono")}>
          <Select id="tr-dataset" value={f.dataset} onChange={(e) => set("dataset", e.target.value)}>
            <option value="">{tr("training.scegli_un_dataset")}</option>
            {forMethod.map((d) => <option key={d.id} value={d.id}>{tr("training.esempi_2", { name: d.name, examples: d.examples })}</option>)}
          </Select>
        </Field>
          {f.method === "sft" && <Button disabled={build.isPending}
            onClick={() => build.mutate({ method: "sft", sources: ["feedback"] }, { onSuccess: (data) => set("dataset", data.id) })}>
            {build.isPending ? tr("training.preparo_il_dataset") : tr("training.crea_dai_pollici_in_su")}
          </Button>}
          {f.method === "sft" && <ImportSftExamples onImported={(id) => set("dataset", id)} />}
          {build.isError && <p className="tr-error" role="alert">{errorMessage(build.error)}</p>}
          <Link to="/training/datasets" onClick={onClose}>{tr("training.apri_le_opzioni_dei_dataset")}</Link>
        </>}
        {step === 2 && <>
        <Field label={tr("training.modello_base")} htmlFor="tr-base" hint={suggestedTrainer?.id === "mlx-lm-lora" ? tr("training.scegli_un_modello_compatibile_con") : tr("training.scegli_un_modello_safetensors_locale")}>
          <Input id="tr-base" list="tr-base-list" value={f.base} onChange={(e) => setF((current) => ({ ...current, base: e.target.value, evalProfile: "" }))} placeholder="Qwen/Qwen3-4B" autoComplete="off" mono />
          <datalist id="tr-base-list">{local.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}</datalist>
        </Field>
          <details>
            <summary>{tr("training.trainer_e_parametri_avanzati")}</summary>
        <Field label={tr("training.trainer")} htmlFor="tr-trainer" hint={tr("training.se_non_scegli_uso_il")}>
          <Select id="tr-trainer" value={f.trainer} onChange={(e) => set("trainer", e.target.value)}>
            <option value="">{tr("training.automatico")}</option>
            {trainers.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </Select>
        </Field>
        <Field label={tr("training.nome_dell_adapter")} htmlFor="tr-name" hint={tr("training.facoltativo")}>
          <Input id="tr-name" value={f.name} onChange={(e) => set("name", e.target.value)} autoComplete="off" />
        </Field>
        <div className="tr-form-grid">
          <Field label={tr("training.passi")} htmlFor="tr-iters"><Input id="tr-iters" inputMode="numeric" value={f.iters} onChange={(e) => set("iters", e.target.value)} placeholder={tr("training.auto")} /></Field>
          <Field label={tr("training.epoche")} htmlFor="tr-epochs"><Input id="tr-epochs" inputMode="decimal" value={f.epochs} onChange={(e) => set("epochs", e.target.value)} placeholder="3" /></Field>
          <Field label={tr("training.rango_lora")} htmlFor="tr-rank"><Input id="tr-rank" inputMode="numeric" value={f.rank} onChange={(e) => set("rank", e.target.value)} placeholder="16" /></Field>
          <Field label={tr("training.learning_rate")} htmlFor="tr-lr"><Input id="tr-lr" inputMode="decimal" value={f.lr} onChange={(e) => set("lr", e.target.value)} placeholder="2e-4" /></Field>
          <Field label={tr("training.batch")} htmlFor="tr-batch"><Input id="tr-batch" inputMode="numeric" value={f.batch} onChange={(e) => set("batch", e.target.value)} placeholder={tr("training.auto")} /></Field>
        </div>
          </details>
        </>}
        {step === 3 && <>
          <p className="text-sm"><b>{METHOD_LABEL[f.method]}</b> su <b>{f.base}</b> · {forMethod.find((d) => d.id === f.dataset)?.name}</p>
          <p className="faint text-sm">{tr("training.al_termine_troverai_l_adapter")}</p>
        <Field label={tr("training.confronto_a_fine_addestramento")} htmlFor="tr-suite" hint={tr("training.esegue_la_suite_con_e")}>
          <Select id="tr-suite" value={f.evalSuite} onChange={(e) => set("evalSuite", e.target.value)}>
            <option value="">{tr("training.nessuno")}</option>
            {(suites.data ?? []).map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </Select>
        </Field>
        {f.evalSuite && (
          <Field label={tr("training.profilo_di_partenza")} htmlFor="tr-profile" hint={tr("training.il_suo_modello_viene_provato")}>
            <Select id="tr-profile" value={f.evalProfile} onChange={(e) => set("evalProfile", e.target.value)}>
              <option value="">{tr("training.scegli")}</option>
              {(profiles.data ?? []).filter((p) => p.model === f.base && !p.params.adapters?.length).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </Select>
          </Field>
        )}
        </>}
        {(shown || start.isError) && <p className="tr-error" role="alert">{shown ?? errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>{tr("training.avvia")}</button>
      </form>
    </Dialog>
  );
}
