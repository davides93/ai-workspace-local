import { useApi } from "../../shared/api/context";
import { useConnection } from "../../shared/query/system";
import { Button, Icon } from "../../shared/ui";

import { t, tNode } from "../../shared/i18n";
/** Shown on a demo transport (mockup or fallback), driven by `capabilities.demo`, not by the system mode. */
export function DemoBanner() {
  const api = useApi();
  if (!api.capabilities.demo) return null;
  return (
    <div className="banner" role="status">
      <Icon name="sparkle" size={16} />
      <span>
        <b>{t("shell.demo_interattiva")}</b> · {api.notice ?? tNode("shell.demo_text", { cmd: <code>aiw serve</code> })}
      </span>
    </div>
  );
}

/** The workspace stopped answering (any failure of the system poll except 401). */
export function ConnectionBanner() {
  const { lost, retry, retrying } = useConnection();
  if (!lost) return null;
  return (
    <div className="banner banner-crit" role="alert">
      <Icon name="alert" size={16} />
      <span><b>{t("shell.connessione_al_workspace_persa")}</b> · {t("shell.connection_lost_text")}</span>
      <Button size="sm" className="banner-cta" icon="refresh" onClick={retry} disabled={retrying}>{retrying ? t("shell.provo") : t("shell.riprova")}</Button>
    </div>
  );
}
