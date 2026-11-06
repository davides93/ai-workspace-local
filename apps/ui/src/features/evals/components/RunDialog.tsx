import { useState } from "react";
import { navigate } from "../../../app/router";
import { Button, Dialog, Field, Input, Select } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { EvalSuite } from "../../../shared/api/types";
import { useAgentChoices, useProfileChoices } from "../../workflows/api";
import { useStartEval } from "../api";
import { MAX_REPEAT } from "../model/evals";

/** Asks which agent and inference profile to use (the suite's own by default) and how many times, then starts the run. */
export function RunDialog({ suite, onClose }: { suite: EvalSuite; onClose: () => void }) {
  const agents = useAgentChoices();
  const profiles = useProfileChoices();
  const start = useStartEval();
  const [agent, setAgent] = useState("");
  const [profile, setProfile] = useState("");
  const [repeat, setRepeat] = useState("1");
  const n = Number(repeat);
  const repeatError = Number.isInteger(n) && n >= 1 && n <= MAX_REPEAT ? undefined : `Un numero intero da 1 a ${MAX_REPEAT}.`;
  const submit = () => {
    if (repeatError) return;
    start.mutate(
      { suiteId: suite.id, agent: agent || undefined, profile: profile || undefined, repeat: n },
      { onSuccess: (r) => { onClose(); navigate(`/evals/reports?report=${encodeURIComponent(r.id)}`); } },
    );
  };
  return (
    <Dialog
      title={`Esegui «${suite.name}»`} size="sm" onClose={onClose}
      footer={<><Button onClick={onClose}>Annulla</Button><Button variant="primary" icon="play" onClick={submit} disabled={start.isPending || !!repeatError}>Avvia</Button></>}
    >
      <form className="stack stack-lg" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <p className="muted text-sm">{suite.tasks.length} task. L'esecuzione prosegue in secondo piano e il report si aggiorna a ogni task.</p>
        <Field label="Agente" htmlFor="ev-agent" hint="Sostituisce quello indicato dalla suite.">
          <Select id="ev-agent" value={agent} onChange={(e) => setAgent(e.target.value)}>
            <option value="">Quello della suite</option>
            {(agents.data ?? []).map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </Select>
        </Field>
        <Field label="Profilo di inferenza" htmlFor="ev-profile" hint="Per confrontare due modelli: esegui la suite una volta per profilo.">
          <Select id="ev-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
            <option value="">Quello dell'agente</option>
            {(profiles.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
        </Field>
        <Field label="Ripetizioni" htmlFor="ev-repeat" error={repeatError} hint="Quante volte si esegue ogni task: utile perché un agente non risponde sempre uguale.">
          <Input id="ev-repeat" type="number" min={1} max={MAX_REPEAT} value={repeat} onChange={(e) => setRepeat(e.target.value)} />
        </Field>
        {start.isError && <p className="run-error" role="alert">{errorMessage(start.error)}</p>}
        <button type="submit" className="sr-only" tabIndex={-1}>Avvia</button>
      </form>
    </Dialog>
  );
}
