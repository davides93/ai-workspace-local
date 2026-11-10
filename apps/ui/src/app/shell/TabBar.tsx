import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { routeLabel, visibleTabRoutes } from "../navigation";
import { routeHref, ROUTES, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";

/** Bottom bar on phones: Panoramica, Workspace, Modelli, Runtime and "Altro" (every other section, the device and the kill switch). Simple mode leaves Runtime out. */
export function TabBar({ current, onMore, moreOpen }: { current?: RouteId; onMore: () => void; moreOpen: boolean }) {
  const [mode] = useUiMode();
  const tabs = visibleTabRoutes(ROUTES, mode);
  const inTabs = tabs.some((r) => r.id === current);
  return (
    <nav className="tabbar" aria-label="Sezioni principali">
      {tabs.map((r) => (
        <Link key={r.id} to={routeHref(r)} aria-current={current === r.id ? "page" : undefined}>
          <Icon name={r.icon} size={20} />
          {routeLabel(r, mode)}
        </Link>
      ))}
      <button type="button" aria-haspopup="dialog" aria-expanded={moreOpen} aria-current={!inTabs && current ? "page" : undefined} onClick={onMore}>
        <Icon name="more" size={20} />
        Altro
      </button>
    </nav>
  );
}
