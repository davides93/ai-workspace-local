import { useRoute } from "../../models/api";
import { formatMb } from "../../../shared/lib/format";
import type { InferenceProfile } from "../../../shared/api/types";

const OFFLOAD: Record<string, string> = { all: "tutto sull'acceleratore", partial: "diviso tra GPU e RAM", none: "sulla CPU" };

/**
 * «Runtime: Automatico»: what the router would choose now for the model of the profile, with the
 * alternatives and the runtimes that were left out, each with its reason. Developer mode only.
 */
export function RoutePanel({ profile }: { profile: InferenceProfile }) {
  const remote = profile.params.allow_remote === true;
  const route = useRoute(profile.model, profile.params.context, remote);
  if (profile.model.trim() === "") return <p className="field-hint">Scegli un modello: il runtime si decide quando parte una conversazione.</p>;
  if (route.isLoading) return <p className="field-hint" role="status">Controllo i runtime…</p>;
  if (route.isError || !route.data) return <p className="field-hint">Non riesco a stimare la scelta adesso: il runtime verrà deciso all'avvio.</p>;
  const { candidates, excluded, notes } = route.data;
  const [first, ...others] = candidates;
  return (
    <div className="route-panel" aria-live="polite">
      {first ? (
        <p className="route-choice">
          <b>Adesso sceglierebbe {first.runtime_name}</b>
          <span className="field-hint"> · {first.reason}. Memoria: ~{formatMb(first.need_mb)} (cache KV ~{formatMb(first.kv_mb)}{first.kv_estimated ? ", stimata" : ""}).</span>
        </p>
      ) : (
        <p className="field-error" role="alert">Nessun runtime può eseguire questo modello su questo dispositivo.</p>
      )}
      {others.length > 0 && (
        <details>
          <summary>Alternative ({others.length})</summary>
          <ol className="route-list" start={2}>
            {others.map((c) => (
              <li key={c.runtime_id}><b>{c.runtime_name}</b> · {OFFLOAD[c.offload] ?? c.offload} · {c.reason}</li>
            ))}
          </ol>
        </details>
      )}
      {excluded.length > 0 && (
        <details>
          <summary>Esclusi ({excluded.length})</summary>
          <ul className="route-list">
            {excluded.map((x) => <li key={x.runtime_id}><b>{x.runtime_name}</b> · {x.reason}</li>)}
          </ul>
        </details>
      )}
      {notes.map((n) => <p key={n} className="field-hint">{n}</p>)}
    </div>
  );
}
