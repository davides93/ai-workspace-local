import { useUpdateStatus } from "../../features/settings/updateApi";
import { useSystem } from "../../shared/query/system";
import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { navGroups, routeLabel } from "../navigation";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";
import { InboxBadge } from "../../features/inbox/components/InboxBadge";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";

import { t } from "../../shared/i18n";
/** Left navigation on desktop; icon-only between 701 and 1100 px (labels stay as tooltips and accessible names). */
export function Rail({ current, onOpenPalette }: { current?: RouteId; onOpenPalette: () => void }) {
  const { data: system } = useSystem();
  const update = useUpdateStatus().data?.available;
  const [mode, setMode] = useUiMode();
  return (
    <nav className="rail" aria-label={t("shell.sezioni")}>
      <div className="brand">
        <div className="brand-mark"><Icon name="bolt" size={18} /></div>
        <div className="brand-text">
          <div className="brand-name">{t("shell.ai_workspace")}</div>
          <div className="brand-sub">{t("shell.locale", { p: system?.version ?? "…" })}</div>
        </div>
      </div>
      {update && (
        <Link to="/settings" className="rail-update" title={t("shell.disponibile_la_versione", { version: update.version })} aria-label={t("shell.aggiornamento_disponibile_versione", { version: update.version })}>
          <Icon name="download" size={15} />
          <span className="nav-text">{t("shell.aggiornamento", { version: update.version })}</span>
        </Link>
      )}
      <DeviceChip />
      <button type="button" className="rail-search" onClick={onOpenPalette} title={t("shell.cerca_e_comandi_ctrl_k")} aria-label={t("shell.cerca_nome_accessibile")}>
        <Icon name="search" size={16} />
        <span className="nav-text">{t("shell.cerca")}</span>
        <kbd className="nav-text">Ctrl K</kbd>
      </button>
      {navGroups(GROUPS, ROUTES, mode).map(({ group, label, routes }) => (
        <div className="nav-group" key={group.id}>
          {label ? <div className="nav-label eyebrow">{label}</div> : <div className="nav-label-spacer" />}
          {routes.map((r) => (
            <Link key={r.id} to={routeHref(r)} className="nav-item" aria-current={current === r.id ? "page" : undefined} title={routeLabel(r, mode)}>
              <Icon name={r.icon} />
              <span className="nav-text">{routeLabel(r, mode)}</span>
              {r.id === "inbox" && <InboxBadge />}
            </Link>
          ))}
        </div>
      ))}
      <div className="rail-foot">
        {mode === "simple" && (
          <button type="button" className="rail-more nav-text" onClick={() => setMode("developer")}>{t("shell.mostra_tutte_le_sezioni")}</button>
        )}
        <KillSwitch />
      </div>
    </nav>
  );
}
