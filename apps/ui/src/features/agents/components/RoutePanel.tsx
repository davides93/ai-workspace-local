import { useRoute } from "../../models/api";
import { formatMb } from "../../../shared/lib/format";
import type { InferenceProfile } from "../../../shared/api/types";

import { t, t as tr } from "../../../shared/i18n";
const OFFLOAD: Record<string, string> = { get all() { return tr("agents.tutto_sull_acceleratore"); }, get partial() { return tr("agents.diviso_tra_gpu_e_ram"); }, get none() { return tr("agents.sulla_cpu"); } };

/**
 * «Runtime: Automatico»: what the router would choose now for the model of the profile, with the
 * alternatives and the runtimes that were left out, each with its reason. Developer mode only.
 */
export function RoutePanel({ profile }: { profile: InferenceProfile }) {
  const remote = profile.params.allow_remote === true;
  const route = useRoute(profile.model, profile.params.context, remote);
  if (profile.model.trim() === "") return <p className="field-hint">{t("agents.scegli_un_modello_il_runtime")}</p>;
  if (route.isLoading) return <p className="field-hint" role="status">{t("agents.controllo_i_runtime")}</p>;
  if (route.isError || !route.data) return <p className="field-hint">{t("agents.non_riesco_a_stimare_la")}</p>;
  const { candidates, excluded, notes } = route.data;
  const [first, ...others] = candidates;
  return (
    <div className="route-panel" aria-live="polite">
      {first ? (
        <p className="route-choice">
          <b>{t("agents.adesso_sceglierebbe", { runtime_name: first.runtime_name })}</b>
          <span className="field-hint">{" "}{t("agents.memoria_cache_kv", { reason: first.reason, p: formatMb(first.need_mb), p2: formatMb(first.kv_mb), p3: first.kv_estimated ? ", stimata" : "" })}</span>
        </p>
      ) : (
        <p className="field-error" role="alert">{t("agents.nessun_runtime_puo_eseguire_questo")}</p>
      )}
      {others.length > 0 && (
        <details>
          <summary>{t("agents.alternative", { length: others.length })}</summary>
          <ol className="route-list" start={2}>
            {others.map((c) => (
              <li key={c.runtime_id}><b>{c.runtime_name}</b> · {OFFLOAD[c.offload] ?? c.offload} · {c.reason}</li>
            ))}
          </ol>
        </details>
      )}
      {excluded.length > 0 && (
        <details>
          <summary>{t("agents.esclusi", { length: excluded.length })}</summary>
          <ul className="route-list">
            {excluded.map((x) => <li key={x.runtime_id}><b>{x.runtime_name}</b> · {x.reason}</li>)}
          </ul>
        </details>
      )}
      {notes.map((n) => <p key={n} className="field-hint">{n}</p>)}
    </div>
  );
}
