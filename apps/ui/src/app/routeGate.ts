import { isUnauthorized } from "../shared/lib/errors";
import type { UiMode } from "../shared/lib/uiMode";

/** Where every other address leads in the simple interface: the one chat (it reopens the last session). */
export const SIMPLE_HOME = "/chat";

/** The one route the simple interface keeps. */
export const SIMPLE_ROUTE = "chat";

/** What the shell does with the route the address resolves to. */
export type RouteGate = "show" | "wait" | "redirect";

/**
 * What the configuration says about the mode, for `routeGate`: a mode, `undefined` (not answered yet) or `"unreadable"`.
 *
 * - The configuration answered with a mode: that one.
 * - It answered without a valid `ui.mode`, or the request failed for good (react-query reports `error` only once the retry
 *   is over): `"unreadable"`. Nothing is coming that could settle the question, so the gate stops waiting and goes by the
 *   last mode this device knows (the one the shell is already drawn with): in the simple interface every address leads to
 *   the chat, in Developer mode the route that was asked for opens as before. The skeleton does not stay for ever. A later
 *   answer from the configuration takes over as usual.
 * - A 401 is not "unreadable": the pairing screen is up and the configuration will be read once the device is paired, so the
 *   gate keeps waiting (`undefined`).
 * - Not answered yet: `undefined`, the gate waits.
 */
export type ConfigMode = UiMode | "unreadable" | undefined;

export function configModeOf(opts: {
  fromConfig: UiMode | undefined;
  status: "pending" | "error" | "success";
  error?: unknown;
}): ConfigMode {
  const { fromConfig, status, error } = opts;
  if (fromConfig) return fromConfig;
  if (status === "success") return "unreadable";
  if (status === "error" && !isUnauthorized(error)) return "unreadable";
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
 * While the configuration cannot be read (`"unreadable"`) nothing is coming that could say which mode
 * is the right one, so the mode the shell is drawn with decides: the shell of the simple interface has
 * no entry for any other section (and no way back), so there every address leads to the chat; a device
 * that shows Developer mode keeps opening the route that was asked for. (The one case this gets wrong,
 * and it is accepted: Developer mode on a device that never stored a mode, with a configuration that
 * fails for good: the person lands in the chat and loses the address.)
 *
 * - `show`: the route is allowed. The chat always is; every route is in Developer mode while nothing
 *   says otherwise (and also when the configuration cannot be read).
 * - `wait`: the answer is not settled yet (the configuration has not answered, or the shown mode and the
 *   configuration disagree for a moment); show the skeleton, mount nothing, redirect nothing.
 * - `redirect`: simple mode on a route that is not the chat, confirmed by the configuration or, when it
 *   cannot be read, by the mode the device shows.
 */
export function routeGate(opts: { routeId: string | undefined; mode: UiMode; configMode: ConfigMode }): RouteGate {
  const { routeId, mode, configMode } = opts;
  if (routeId === SIMPLE_ROUTE) return "show";
  if (configMode === "unreadable") return mode === "simple" ? "redirect" : "show";
  if (configMode === "simple" && mode === "simple") return "redirect";
  if (mode === "developer" && configMode !== "simple") return "show";
  return "wait";
}
