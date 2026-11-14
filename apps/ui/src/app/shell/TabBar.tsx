import { Icon } from "../../shared/ui";
import { Link } from "../router";
import { hasNavigation, routeLabel, visibleTabRoutes } from "../navigation";
import { routeHref, ROUTES, type RouteId } from "../routes";
import { useUiMode } from "../useUiMode";

import { t } from "../../shared/i18n";
/** Bottom bar on phones: Panoramica, Workspace, Modelli, Runtime and "Altro" (every other section, the device and the kill switch). The simple interface has one destination, so it has no bar (the chat header has a «Menu» button). */
export function TabBar({ current, onMore, moreOpen }: { current?: RouteId; onMore: () => void; moreOpen: boolean }) {
  const [mode] = useUiMode();
  if (!hasNavigation(ROUTES, mode)) return null;
  const tabs = visibleTabRoutes(ROUTES, mode);
  const inTabs = tabs.some((r) => r.id === current);
  return (
    <nav className="tabbar" aria-label={t("shell.sezioni_principali")}>
      {tabs.map((r) => (
        <Link key={r.id} to={routeHref(r)} aria-current={current === r.id ? "page" : undefined}>
          <Icon name={r.icon} size={20} />
          {routeLabel(r, mode)}
        </Link>
      ))}
      <button type="button" aria-haspopup="dialog" aria-expanded={moreOpen} aria-current={!inTabs && current ? "page" : undefined} onClick={onMore}>
        <Icon name="more" size={20} />
        {t("shell.more")}
      </button>
    </nav>
  );
}
