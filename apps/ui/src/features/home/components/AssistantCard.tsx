import { Link } from "../../../app/router";
import { Panel } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
/** Simple mode: which model answers, by its human name, and a way to change it. */
export function AssistantCard({ name }: { name: string }) {
  return (
    <Panel title={t("home.il_tuo_assistente")} icon="cube">
      <div className="hm-assistant">
        <span className="hm-assistant-name" title={name}>{name}</span>
        <Link to="/models" className="btn sm ghost">{t("home.cambia")}</Link>
      </div>
    </Panel>
  );
}
