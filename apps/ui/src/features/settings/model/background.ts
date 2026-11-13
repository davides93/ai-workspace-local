import type { BackgroundStatus, ProactiveLevel } from "../../../shared/api/types";

import { t, tList } from "../../../shared/i18n";
/** iPhone, iPad and Android: where the system, not the app, decides when the assistant gets to look around. */
export const isPhoneOs = (os: string | undefined): os is "ios" | "android" => os === "ios" || os === "android";

export interface NotificationLine {
  tone: "ok" | "warn" | "muted";
  text: string;
  /** The button «Attiva le notifiche» has something to do. */
  canAsk: boolean;
  /** The permission was refused: only the system settings can change it. */
  inSystemSettings: boolean;
}

/** What the notification permission means for the person, in plain words. */
export function notificationLine(status: BackgroundStatus | undefined): NotificationLine {
  switch (status?.notifications) {
    case "granted":
      return { tone: "ok", text: t("settings.le_notifiche_sono_attive_ti"), canAsk: false, inSystemSettings: false };
    case "denied":
      return { tone: "warn", text: t("settings.le_notifiche_sono_spente_per"), canAsk: false, inSystemSettings: true };
    case "not_determined":
      return { tone: "warn", text: t("settings.senza_notifiche_non_posso_avvisarti"), canAsk: true, inSystemSettings: false };
    default:
      return { tone: "muted", text: t("settings.le_notifiche_non_sono_disponibili"), canAsk: false, inSystemSettings: false };
  }
}

/** One line on what the system lets the assistant do while the app is closed. */
export function scheduleLine(status: BackgroundStatus | undefined, level: ProactiveLevel | undefined): string {
  if (level === "off" || level === undefined) return t("settings.l_assistente_e_spento_in");
  if (!status?.scheduled) return t("settings.i_controlli_in_background_non");
  return status.detail ?? t("settings.i_controlli_in_background_sono");
}

/** The three steps of the timed Shortcut on iPhone (the only way to get a check at a precise time). */
export const SHORTCUT_STEPS: readonly string[] = tList(["settings.apri_l_app_comandi_e", "settings.scegli_l_orario_per_esempio", "settings.come_azione_cerca_ai_workspace"]);

/** The permission is asked only when the person turns the assistant on, never before. */
export const shouldAskNotifications = (before: unknown, after: unknown): boolean => (before === "off" || before === undefined) && (after === "propose" || after === "act_readonly");

/** Settings that change what the native side schedules (the phone must be told again). */
export const changesSchedule = (key: string): boolean => key.startsWith("proactive.") || key === "app.notifications";
