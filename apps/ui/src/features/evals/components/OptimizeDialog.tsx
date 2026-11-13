import { useState } from "react";
import { navigate } from "../../../app/router";
import { Button, Dialog, Field, Input, Select } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { EvalSuite } from "../../../shared/api/types";
import { useAgentChoices, useProfileChoices } from "../../workflows/api";
import { useStartOptimization } from "../api";
import { MAX_REPEAT } from "../model/evals";
import { DEFAULT_VARIANTS, MAX_VARIANTS, variantsError } from "../model/optimizations";

import { t } from "../../../shared/i18n";
/**
 * Asks which agent's prompt to improve and how many variants to try, then starts the optimization. The suite runs once with the
 * current prompt and once per variant; nothing changes in the agent until the user applies a result.
 */
export function OptimizeDialog({ suite, onClose }: { suite: EvalSuite; onClose: () => void }) {
  const agents = useAgentChoices();
  const profiles = useProfileChoices();
  const start = useStartOptimization();
  const [agent, setAgent] = useState(suite.agent ?? "");
  const [variants, setVariants] = useState(String(DEFAULT_VARIANTS));
  const [profile, setProfile] = useState("");
  const [repeat, setRepeat] = useState("1");
  const chosen = agent || agents.data?.[0]?.id || "";
  const variantsProblem = variantsError(variants);
  const r = Number(repeat);
  const repeatProblem = Number.isInteger(r) && r >= 1 && r <= MAX_REPEAT ? undefined : t("evals.integer_range", { min: 1, max: MAX_REPEAT });
  const runs = (variantsProblem ? DEFAULT_VARIANTS : Number(variants)) + 1;
  const submit = () => {
    if (variantsProblem || repeatProblem || !chosen) return;
    start.mutate(
      { suiteId: suite.id, agent: chosen, variants: Number(variants), profile: profile || undefined, repeat: r },
      { onSuccess: (o) => { onClose(); navigate(`/evals/optimizations?opt=${encodeURIComponent(o.id)}`); } },
    );
  };
  return (
    <Dialog
      title={t("evals.ottimizza_il_prompt_con", { name: suite.name })} size="sm" onClose={onClose}
      footer={<><Button onClick={onClose}>{t("evals.annulla")}</Button><Button variant="primary" icon="sparkle" onClick={submit} disabled={start.isPending || !!variantsProblem || !!repeatProblem || !chosen}>{t("evals.avvia")}</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">
          {t("evals.la_suite_gira_volte_con", { runs })}</p>
        <Field label={t("evals.agente")} htmlFor="op-agent" hint={t("evals.di_quale_agente_provare_il")}>
          <Select id="op-agent" value={chosen} onChange={(e) => setAgent(e.target.value)} disabled={!agents.data?.length}>
            {(agents.data ?? []).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <Field label={t("evals.varianti")} htmlFor="op-variants" error={variantsProblem} hint={t("evals.da_1_a_piu_varianti", { max: MAX_VARIANTS })}>
          <Input id="op-variants" type="number" min={1} max={MAX_VARIANTS} value={variants} onChange={(e) => setVariants(e.target.value)} />
        </Field>
        <Field label={t("evals.profilo_di_inferenza")} htmlFor="op-profile" hint={t("evals.per_i_task_e_se")}>
          <Select id="op-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
            <option value="">{t("evals.quello_dell_agente")}</option>
            {(profiles.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
        </Field>
        <Field label={t("evals.ripetizioni")} htmlFor="op-repeat" error={repeatProblem} hint={t("evals.quante_volte_si_esegue_ogni")}>
          <Input id="op-repeat" type="number" min={1} max={MAX_REPEAT} value={repeat} onChange={(e) => setRepeat(e.target.value)} />
        </Field>
        {start.isError && <p className="run-error" role="alert">{errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>{t("evals.avvia")}</button>
      </form>
    </Dialog>
  );
}
