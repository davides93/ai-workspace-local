import { useState } from "react";
import { useUpdateStatus } from "../../features/settings/updateApi";
import { useSystem } from "../../shared/query/system";
import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { hasNavigation, navGroups, routeLabel } from "../navigation";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";
import { InboxBadge } from "../../features/inbox/components/InboxBadge";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";
import { UpdateDialog } from "./UpdateDialog";

import { t } from "../../shared/i18n";
/**
 * Left navigation on desktop; icon-only between 701 and 1100 px (labels stay as tooltips and accessible names).
 * The simple interface has a single destination, so it draws no section list: brand, search and «Altro»
 * (device, mode, language, theme and the kill switch live in that menu). The update notice opens the updater
 * in a dialog there (the Impostazioni page is not reachable); in developer mode it links to it.
 */
export function Rail({ current, onOpenPalette, onOpenMenu }: { current?: RouteId; onOpenPalette: () => void; onOpenMenu: () => void }) {
  const { data: system } = useSystem();
  const update = useUpdateStatus().data?.available;
  const [mode] = useUiMode();
  const [updating, setUpdating] = useState(false);
  const simple = mode === "simple";
  const updateLabel = update ? t("shell.aggiornamento_disponibile_versione", { version: update.version }) : "";
  return (
    <nav className="rail" aria-label={t("shell.sezioni")}>
      <div className="brand">
        <div className="brand-mark"><Icon name="bolt" size={18} /></div>
        <div className="brand-text">
          <div className="brand-name">{t("shell.ai_workspace")}</div>
          <div className="brand-sub">{t("shell.locale", { p: system?.version ?? "…" })}</div>
        </div>
      </div>
      {update && (simple ? (
        <button type="button" className="rail-update" aria-haspopup="dialog" onClick={() => setUpdating(true)} title={t("shell.disponibile_la_versione", { version: update.version })} aria-label={updateLabel}>
          <Icon name="download" size={15} />
          <span className="nav-text">{t("shell.aggiornamento", { version: update.version })}</span>
        </button>
      ) : (
        <Link to="/settings" className="rail-update" title={t("shell.disponibile_la_versione", { version: update.version })} aria-label={updateLabel}>
          <Icon name="download" size={15} />
          <span className="nav-text">{t("shell.aggiornamento", { version: update.version })}</span>
        </Link>
      ))}
      {!simple && <DeviceChip />}
      <button type="button" className="rail-search" onClick={onOpenPalette} title={t("shell.cerca_e_comandi_ctrl_k")} aria-label={t("shell.cerca_nome_accessibile")}>
        <Icon name="search" size={16} />
        <span className="nav-text">{t("shell.cerca")}</span>
        <kbd className="nav-text">Ctrl K</kbd>
      </button>
      {simple && (
        <button type="button" className="rail-search rail-menu" aria-haspopup="dialog" onClick={onOpenMenu} title={t("shell.menu")} aria-label={t("shell.menu")}>
          <Icon name="more" size={16} />
          <span className="nav-text">{t("shell.menu")}</span>
        </button>
      )}
      {hasNavigation(ROUTES, mode) && navGroups(GROUPS, ROUTES, mode).map(({ group, label, routes }) => (
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
      {!simple && (
        <div className="rail-foot">
          <KillSwitch />
        </div>
      )}
      {updating && <UpdateDialog onClose={() => setUpdating(false)} />}
    </nav>
  );
}
