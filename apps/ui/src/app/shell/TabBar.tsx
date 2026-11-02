import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { routeHref, TAB_ROUTES, type RouteId } from "../routes";

/** Bottom bar on phones: Panoramica, Workspace, Modelli, Runtime and "Altro" (every other section, the device and the kill switch). */
export function TabBar({ current, onMore, moreOpen }: { current?: RouteId; onMore: () => void; moreOpen: boolean }) {
  const inTabs = TAB_ROUTES.some((r) => r.id === current);
  return (
    <nav className="tabbar" aria-label="Sezioni principali">
      {TAB_ROUTES.map((r) => (
        <Link key={r.id} to={routeHref(r)} aria-current={current === r.id ? "page" : undefined}>
          <Icon name={r.icon} size={20} />
          {r.label}
        </Link>
      ))}
      <button type="button" aria-haspopup="dialog" aria-expanded={moreOpen} aria-current={!inTabs && current ? "page" : undefined} onClick={onMore}>
        <Icon name="more" size={20} />
        Altro
      </button>
    </nav>
  );
}
