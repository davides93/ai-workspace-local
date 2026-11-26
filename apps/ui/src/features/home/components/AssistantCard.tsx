import { Link } from "../../../app/router";
import { Panel } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
/** Simple mode: which assistant answers (its role first, the model's human name below) and a way to change it. */
export function AssistantCard({ name, role }: { name: string; role?: string }) {
  return (
    <Panel title={t("home.il_tuo_assistente")} icon="cube">
      <div className="hm-assistant">
        {role ? (
          <span className="hm-assistant-text">
            <span className="hm-assistant-name" title={role}>{role}</span>
            <span className="hm-assistant-model faint text-sm" title={name}>{name}</span>
          </span>
        ) : <span className="hm-assistant-name" title={name}>{name}</span>}
        <Link to="/models" className="btn sm ghost">{t("home.cambia")}</Link>
      </div>
    </Panel>
  );
}
