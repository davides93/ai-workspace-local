import { useState } from "react";
import { navigate } from "../../../app/router";
import { Button, Dialog, Field, Input, Select } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { EvalSuite } from "../../../shared/api/types";
import { useAgentChoices, useProfileChoices } from "../../workflows/api";
import { useStartEval } from "../api";
import { MAX_REPEAT } from "../model/evals";

import { t } from "../../../shared/i18n";
/** Asks which agent and inference profile to use (the suite's own by default) and how many times, then starts the run. */
export function RunDialog({ suite, onClose }: { suite: EvalSuite; onClose: () => void }) {
  const agents = useAgentChoices();
  const profiles = useProfileChoices();
  const start = useStartEval();
  const [agent, setAgent] = useState("");
  const [profile, setProfile] = useState("");
  const [repeat, setRepeat] = useState("1");
  const n = Number(repeat);
  const repeatError = Number.isInteger(n) && n >= 1 && n <= MAX_REPEAT ? undefined : t("evals.integer_range", { min: 1, max: MAX_REPEAT });
  const submit = () => {
    if (repeatError) return;
    start.mutate(
      { suiteId: suite.id, agent: agent || undefined, profile: profile || undefined, repeat: n },
      { onSuccess: (r) => { onClose(); navigate(`/evals/reports?report=${encodeURIComponent(r.id)}`); } },
    );
  };
  return (
    <Dialog
      title={t("evals.esegui", { name: suite.name })} size="sm" onClose={onClose}
      footer={<><Button onClick={onClose}>{t("evals.annulla")}</Button><Button variant="primary" icon="play" onClick={submit} disabled={start.isPending || !!repeatError}>{t("evals.avvia")}</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">{t("evals.task_l_esecuzione_prosegue_in", { length: suite.tasks.length })}</p>
        <Field label={t("evals.agente")} htmlFor="ev-agent" hint={t("evals.sostituisce_quello_indicato_dalla_suite")}>
          <Select id="ev-agent" value={agent} onChange={(e) => setAgent(e.target.value)}>
            <option value="">{t("evals.quello_della_suite")}</option>
            {(agents.data ?? []).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <Field label={t("evals.profilo_di_inferenza")} htmlFor="ev-profile" hint={t("evals.per_confrontare_due_modelli_esegui")}>
          <Select id="ev-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
            <option value="">{t("evals.quello_dell_agente")}</option>
            {(profiles.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
        </Field>
        <Field label={t("evals.ripetizioni")} htmlFor="ev-repeat" error={repeatError} hint={t("evals.quante_volte_si_esegue_ogni_2")}>
          <Input id="ev-repeat" type="number" min={1} max={MAX_REPEAT} value={repeat} onChange={(e) => setRepeat(e.target.value)} />
        </Field>
        {start.isError && <p className="run-error" role="alert">{errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>{t("evals.avvia")}</button>
      </form>
    </Dialog>
  );
}
