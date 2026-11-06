import { Button, Pill } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { AgentProfile, InferenceProfile } from "../../../shared/api/types";
import { useAgentRouting, useResetRouting } from "../api";

/** "0,75" for a reward mean; "—" while nothing was rewarded. */
const meanText = (m: number | undefined): string => (m === undefined ? "—" : m.toFixed(2).replace(".", ","));

interface Props {
  agent: AgentProfile;
  /** The agent as saved: the statistics only exist for it. `undefined` for a new agent. */
  savedAgent: AgentProfile | undefined;
  profiles: InferenceProfile[];
  onChange: (patch: Partial<AgentProfile>) => void;
}

/**
 * The candidate profiles of an agent. With the setting `routing.bandit` on, each run takes the one that
 * earned the most thumbs up and passed evaluation tasks so far (UCB1: a profile never tried goes first).
 */
export function RoutingSection({ agent, savedAgent, profiles, onChange }: Props) {
  const candidates = agent.candidate_profiles ?? [];
  const hasSaved = (savedAgent?.candidate_profiles ?? []).length > 0;
  const stats = useAgentRouting(agent.id, hasSaved);
  const reset = useResetRouting();
  const toggle = (id: string) => onChange({ candidate_profiles: candidates.includes(id) ? candidates.filter((c) => c !== id) : [...candidates, id] });
  const rows = stats.data?.candidates ?? [];
  return (
    <div className="form-grid">
      <p className="faint text-sm">
        Facoltativo. Elenca i profili tra cui l'agente può scegliere a ogni nuovo run. Un pollice in su (o un task di valutazione superato)
        vale 1, un pollice in giù (o un task fallito) vale 0. Il router si attiva da Impostazioni → <span className="mono">routing.bandit</span>;
        spento, l'agente usa sempre il profilo scelto in «Modello».
      </p>
      <fieldset className="tr-sources">
        <legend className="field-label">Profili candidati</legend>
        {profiles.map((p) => (
          <label key={p.id}>
            <input type="checkbox" checked={candidates.includes(p.id)} onChange={() => toggle(p.id)} /> {p.name}{" "}
            <span className="faint text-xs mono">{p.model || p.runtime_id}</span>
          </label>
        ))}
      </fieldset>
      {hasSaved && (
        <div>
          <div className="row mb-2">
            <b className="text-sm">Cosa ha imparato finora</b>
            {stats.data && <Pill tone={stats.data.enabled ? "ok" : "neutral"}>{stats.data.enabled ? "router attivo" : "router spento"}</Pill>}
            <Button size="sm" onClick={() => reset.mutate(agent.id)} disabled={reset.isPending || rows.every((r) => r.n === 0)}>Azzera</Button>
          </div>
          {stats.isError && <p className="tr-error" role="alert">{errorMessage(stats.error)}</p>}
          {rows.length > 0 && (
            <table className="table text-sm" aria-label="Statistiche del router">
              <thead><tr><th>Profilo</th><th className="num">Valutazioni</th><th className="num">Reward media</th></tr></thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.profile}>
                    <td className="mono">{r.profile}</td>
                    <td className="num">{r.n}</td>
                    <td className="num">{meanText(r.mean)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="faint text-xs mt-2">Le statistiche riguardano l'agente salvato: i profili appena spuntati compaiono dopo il salvataggio.</p>
        </div>
      )}
    </div>
  );
}
