import { isUnauthorized } from "../shared/lib/errors";
import type { UiMode } from "../shared/lib/uiMode";

/** Where every other address leads in the simple interface: the one chat (it reopens the last session). */
export const SIMPLE_HOME = "/chat";

/** The one route the simple interface keeps. */
export const SIMPLE_ROUTE = "chat";

/** What the shell does with the route the address resolves to. */
export type RouteGate = "show" | "wait" | "redirect";

/**
 * The mode the configuration confirms, for `routeGate` (`status` is the one of the configuration's query).
 *
 * - The configuration answered with a mode: that one.
 * - It answered without a valid `ui.mode`, or the request failed for good (react-query reports `error` only once the retry
 *   is over): nothing is coming that could settle the question, so the mode kept on this device stands in for it. Without
 *   this the route would be held back (`wait`) for ever and the person would see a skeleton, on `#/` as well. A device
 *   that never kept a mode shows `simple`, so the address leads to the chat, which always opens and has the menu to go
 *   on from.
 * - A 401 is not "unreadable": the pairing screen is up and the configuration will be read once the device is paired.
 * - Not answered yet: `undefined`, the gate waits.
 */
export function confirmedUiMode(opts: {
  fromConfig: UiMode | undefined;
  status: "pending" | "error" | "success";
  error?: unknown;
  stored: UiMode;
}): UiMode | undefined {
  const { fromConfig, status, error, stored } = opts;
  if (fromConfig) return fromConfig;
  if (status === "success") return stored;
  if (status === "error" && !isUnauthorized(error)) return stored;
  return undefined;
}

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
