/** Address of the local daemon when the page itself is not served by it. */
export const DEFAULT_BASE_URL = "http://127.0.0.1:7777";

/**
 * Base URL other programs use to reach this workspace (OpenAI gateway, MCP, A2A, webhooks).
 * An http(s) page is served by the daemon itself, so its origin is the address; inside the
 * desktop shell (origin `tauri://…` or `http://tauri.localhost`) and for file pages it is the
 * default local daemon address.
 */
export function serviceBaseUrl(opts: { native?: boolean; protocol?: string; origin?: string } = {}): string {
  const protocol = opts.protocol ?? (typeof location !== "undefined" ? location.protocol : "");
  const origin = opts.origin ?? (typeof location !== "undefined" ? location.origin : "");
  if (opts.native) return DEFAULT_BASE_URL;
  return protocol === "http:" || protocol === "https:" ? origin : DEFAULT_BASE_URL;
}
