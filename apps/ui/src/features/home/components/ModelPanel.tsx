import { Link } from "../../../app/router";
import { EmptyState, Icon, KeyValue, Panel, QueryState, StatusPill } from "../../../shared/ui";
import { useAgents, useProfiles } from "../../agents/api";
import { useRuntimes } from "../../runtimes/api";
import { useConfig } from "../../settings/api";
import { defaultProfile } from "../model/overview";

import { t, t as tr } from "../../../shared/i18n";
/** The profile of the default agent: which model and runtime a new session uses. */
export function ModelPanel() {
  const agents = useAgents();
  const profiles = useProfiles();
  const runtimes = useRuntimes();
  const config = useConfig();
  const profile = defaultProfile(agents.data ?? [], profiles.data ?? [], config.data?.effective["agent.default"]);
  const runtime = runtimes.data?.find((r) => r.id === profile?.runtime_id);
  return (
    <Panel title={t("home.modello_predefinito")} icon="cube">
      <QueryState query={profiles}>
        {() =>
          !profile ? (
            <EmptyState icon="cube" title={t("home.nessun_profilo")} description={t("home.crea_un_profilo_di_inferenza")} action={<Link to="/agents" className="btn sm">{t("home.apri_i_profili")}</Link>} />
          ) : (
            <div className="stack stack-lg">
              <KeyValue
                items={[
                  ["Profilo", <b>{profile.name}</b>],
                  ["Modello", profile.model ? <span className="mono">{profile.model}</span> : <span className="faint">{t("home.non_scelto")}</span>],
                  ["Runtime", <span className="row"><span className="mono">{profile.runtime_id}</span>{runtime && <StatusPill status={runtime.status} />}</span>],
                ]}
              />
              <Link to="/models" className="link-more">{tr("home.gestisci_i_modelli")}{" "}<Icon name="forward" size={14} /></Link>
            </div>
          )
        }
      </QueryState>
    </Panel>
  );
}
