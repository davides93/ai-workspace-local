import type { UiMode } from "../shared/lib/uiMode";

/** Where every other address leads in the simple interface: the one chat (it reopens the last session). */
export const SIMPLE_HOME = "/chat";

/** The one route the simple interface keeps. */
export const SIMPLE_ROUTE = "chat";

/** What the shell does with the route the address resolves to. */
export type RouteGate = "show" | "wait" | "redirect";

/**
 * Whether to open the route as it is, to hold back (skeleton) or to send the person to the chat.
 *
 * The simple interface has a single route, so every other address (old hashes, a mistyped one and the
 * "not found" page included: `routeId` is `undefined` then) goes to the chat. The redirect needs the
 * *configuration* to say `simple` (`ui.mode` is the source of truth): the value kept on the device
 * starts as `simple` when there is none, and a person in Developer mode who opens `#/runtimes` on a
 * device that never stored a mode must not be thrown out while the configuration is still loading.
 * It also needs the mode the shell shows to have caught up with the configuration (it follows it one
 * render later), otherwise the redirect would fire on the stale value.
 *
 * - `show`: the route is allowed. The chat always is; every route is in Developer mode while nothing
 *   says otherwise.
 * - `wait`: the answer is not settled yet (the configuration has not answered, or the shown mode and the
 *   configuration disagree for a moment); show the skeleton, mount nothing, redirect nothing.
 * - `redirect`: simple mode, confirmed by the configuration, on a route that is not the chat.
 */
export function routeGate(opts: { routeId: string | undefined; mode: UiMode; configMode: UiMode | undefined }): RouteGate {
  const { routeId, mode, configMode } = opts;
  if (routeId === SIMPLE_ROUTE) return "show";
  if (configMode === "simple" && mode === "simple") return "redirect";
  if (mode === "developer" && configMode !== "simple") return "show";
  return "wait";
}
