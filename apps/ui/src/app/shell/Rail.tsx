import { useUpdateStatus } from "../../features/settings/updateApi";
import { useSystem } from "../../shared/query/system";
import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { navGroups, routeLabel } from "../navigation";
import { GROUPS, ROUTES, routeHref, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";
import { DeviceChip } from "./DeviceChip";
import { KillSwitch } from "./KillSwitch";

/** Left navigation on desktop; icon-only between 701 and 1100 px (labels stay as tooltips and accessible names). */
export function Rail({ current, onOpenPalette }: { current?: RouteId; onOpenPalette: () => void }) {
  const { data: system } = useSystem();
  const update = useUpdateStatus().data?.available;
  const [mode, setMode] = useUiMode();
  return (
    <nav className="rail" aria-label="Sezioni">
      <div className="brand">
        <div className="brand-mark"><Icon name="bolt" size={18} /></div>
        <div className="brand-text">
          <div className="brand-name">AI Workspace</div>
          <div className="brand-sub">locale · {system?.version ?? "…"}</div>
        </div>
      </div>
      {update && (
        <Link to="/settings" className="rail-update" title={`Disponibile la versione ${update.version}`} aria-label={`Aggiornamento disponibile: versione ${update.version}`}>
          <Icon name="download" size={15} />
          <span className="nav-text">Aggiornamento {update.version}</span>
        </Link>
      )}
      <DeviceChip />
      <button type="button" className="rail-search" onClick={onOpenPalette} title="Cerca e comandi (Ctrl+K)" aria-label="Cerca e comandi (Ctrl+K)">
        <Icon name="search" size={16} />
        <span className="nav-text">Cerca…</span>
        <kbd className="nav-text">Ctrl K</kbd>
      </button>
      {navGroups(GROUPS, ROUTES, mode).map(({ group, label, routes }) => (
        <div className="nav-group" key={group.id}>
          {label ? <div className="nav-label eyebrow">{label}</div> : <div className="nav-label-spacer" />}
          {routes.map((r) => (
            <Link key={r.id} to={routeHref(r)} className="nav-item" aria-current={current === r.id ? "page" : undefined} title={routeLabel(r, mode)}>
              <Icon name={r.icon} />
              <span className="nav-text">{routeLabel(r, mode)}</span>
            </Link>
          ))}
        </div>
      ))}
      <div className="rail-foot">
        {mode === "simple" && (
          <button type="button" className="rail-more nav-text" onClick={() => setMode("developer")}>Mostra tutte le sezioni</button>
        )}
        <KillSwitch />
      </div>
    </nav>
  );
}
