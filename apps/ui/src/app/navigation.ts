import type { UiMode } from "../shared/lib/uiMode";
import type { AppRoute, GroupId } from "./routes";

/**
 * Which sections the interface shows in each mode. Rail, tab bar, "Altro" sheet and command palette
 * all go through these functions. Hiding is only about navigation: a hidden route still opens by URL.
 */

type NavRoute = Pick<AppRoute, "id" | "label" | "group" | "simple" | "simpleLabel" | "tab" | "simpleTab">;
type NavGroup = { id: GroupId; label?: string; simpleLabel?: string };

/** Routes shown in `mode`, in table order. */
export function visibleRoutes<R extends Pick<AppRoute, "simple">>(routes: R[], mode: UiMode): R[] {
  return mode === "simple" ? routes.filter((r) => r.simple) : routes;
}

/** Name of a section in `mode` (simple mode may use a plainer word). */
export const routeLabel = (r: Pick<AppRoute, "label" | "simpleLabel">, mode: UiMode): string => (mode === "simple" && r.simpleLabel) || r.label;

export const groupLabel = (g: Pick<NavGroup, "label" | "simpleLabel">, mode: UiMode): string | undefined => (mode === "simple" && g.simpleLabel) || g.label;

/** Position of a route in the phone tab bar in `mode`: simple mode may have its own table (`simpleTab`). */
export const tabPosition = (r: Pick<AppRoute, "tab" | "simpleTab">, mode: UiMode): number | undefined => (mode === "simple" ? r.simpleTab ?? r.tab : r.tab);

/** Phone tab bar entries in `mode`, in tab order (the fifth slot, "Altro", is not a route). */
export function visibleTabRoutes<R extends Pick<AppRoute, "simple" | "tab" | "simpleTab">>(routes: R[], mode: UiMode): R[] {
  return visibleRoutes(routes, mode)
    .filter((r) => tabPosition(r, mode) !== undefined)
    .sort((a, b) => (tabPosition(a, mode) ?? 0) - (tabPosition(b, mode) ?? 0));
}

/** Groups that still have a visible route, each with its routes and the label to print (none for the unlabelled first group). */
export function navGroups<R extends NavRoute, G extends NavGroup>(groups: G[], routes: R[], mode: UiMode): { group: G; label?: string; routes: R[] }[] {
  const shown = visibleRoutes(routes, mode);
  return groups
    .map((group) => ({ group, label: groupLabel(group, mode), routes: shown.filter((r) => r.group === group.id) }))
    .filter((g) => g.routes.length > 0);
}
