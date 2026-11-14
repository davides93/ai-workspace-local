import { t, type MessageKey } from "../shared/i18n";
import type { UiMode } from "../shared/lib/uiMode";
import type { AppRoute, GroupId } from "./routes";

/**
 * Which sections the interface shows in each mode. Rail, tab bar, "Altro" sheet and command palette
 * all go through these functions. Simple mode keeps a single route (the chat): there is nothing to navigate
 * between, so the rail, the tab bar and the sheet draw no section list (`hasNavigation`), and the shell sends
 * every other address to the chat (`routeGate`). Developer mode shows every route.
 */

type NavRoute = Pick<AppRoute, "id" | "label" | "group" | "simple" | "simpleLabel" | "tab">;
type NavGroup = { id: GroupId; label?: MessageKey; simpleLabel?: MessageKey };

/** Routes shown in `mode`, in table order. */
export function visibleRoutes<R extends Pick<AppRoute, "simple">>(routes: R[], mode: UiMode): R[] {
  return mode === "simple" ? routes.filter((r) => r.simple) : routes;
}

/** Name of a section in `mode` (simple mode may use a plainer word). */
export const routeLabel = (r: Pick<AppRoute, "label" | "simpleLabel">, mode: UiMode): string => t((mode === "simple" && r.simpleLabel) || r.label);

export const groupLabel = (g: Pick<NavGroup, "label" | "simpleLabel">, mode: UiMode): string | undefined => (() => { const k = (mode === "simple" && g.simpleLabel) || g.label; return k ? t(k) : undefined; })();

/** More than one destination in `mode`: only then do the rail, the tab bar and the sheet list sections. */
export const hasNavigation = <R extends Pick<AppRoute, "simple">>(routes: R[], mode: UiMode): boolean => visibleRoutes(routes, mode).length > 1;

/** Phone tab bar entries in `mode`, in tab order (the fifth slot, "Altro", is not a route). Empty when there is nothing to navigate between. */
export function visibleTabRoutes<R extends Pick<AppRoute, "simple" | "tab">>(routes: R[], mode: UiMode): R[] {
  if (!hasNavigation(routes, mode)) return [];
  return visibleRoutes(routes, mode)
    .filter((r) => r.tab !== undefined)
    .sort((a, b) => (a.tab ?? 0) - (b.tab ?? 0));
}

/** Groups that still have a visible route, each with its routes and the label to print (none for the unlabelled first group). */
export function navGroups<R extends NavRoute, G extends NavGroup>(groups: G[], routes: R[], mode: UiMode): { group: G; label?: string; routes: R[] }[] {
  const shown = visibleRoutes(routes, mode);
  return groups
    .map((group) => ({ group, label: groupLabel(group, mode), routes: shown.filter((r) => r.group === group.id) }))
    .filter((g) => g.routes.length > 0);
}
