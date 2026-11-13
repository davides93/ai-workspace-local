import type { UpdateChannel, UpdateStatus } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
export const CHANNEL_LABEL: Record<UpdateChannel, string> = { get stable() { return t("settings.produzione"); }, get rc() { return t("settings.release_candidate"); }, get beta() { return t("settings.beta"); }, get alpha() { return t("settings.alpha"); } };

/** The next thing the user can do about an update. */
export type UpdateStep = "none" | "download" | "downloading" | "install" | "manual";

export function updateStep(s: Pick<UpdateStatus, "state" | "available" | "file">): UpdateStep {
  if (!s.available) return "none";
  if (s.state === "downloading") return "downloading";
  if (s.file) return "install";
  // A release with no installer for this system is read on its page.
  return s.available.asset ? "download" : "manual";
}

/** Download progress, 0–100 (0 when the size is not known yet). */
export function downloadPercent(s: Pick<UpdateStatus, "received" | "total">): number {
  return s.total > 0 ? Math.min(100, Math.round((s.received / s.total) * 100)) : 0;
}
