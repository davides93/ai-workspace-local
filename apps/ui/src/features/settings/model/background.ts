import type { BackgroundStatus, ProactiveLevel } from "../../../shared/api/types";

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
      return { tone: "ok", text: "Le notifiche sono attive: ti avviso quando c'è una proposta o il riepilogo del mattino.", canAsk: false, inSystemSettings: false };
    case "denied":
      return { tone: "warn", text: "Le notifiche sono spente per questa app. Riaccendile dalle impostazioni del telefono.", canAsk: false, inSystemSettings: true };
    case "not_determined":
      return { tone: "warn", text: "Senza notifiche non posso avvisarti: le proposte le trovi solo aprendo l'app.", canAsk: true, inSystemSettings: false };
    default:
      return { tone: "muted", text: "Le notifiche non sono disponibili su questo dispositivo.", canAsk: false, inSystemSettings: false };
  }
}

/** One line on what the system lets the assistant do while the app is closed. */
export function scheduleLine(status: BackgroundStatus | undefined, level: ProactiveLevel | undefined): string {
  if (level === "off" || level === undefined) return "L'assistente è spento: in background non fa nulla.";
  if (!status?.scheduled) return "I controlli in background non sono ancora in programma: aprendo l'app si preparano da soli.";
  return status.detail ?? "I controlli in background sono in programma: il sistema decide quando farli partire.";
}

/** The three steps of the timed Shortcut on iPhone (the only way to get a check at a precise time). */
export const SHORTCUT_STEPS: readonly string[] = [
  "Apri l'app Comandi e vai su «Automazione», poi tocca + e scegli «Ora del giorno».",
  "Scegli l'orario (per esempio le 8:00), «Ogni giorno» e «Esegui subito».",
  "Come azione cerca AI Workspace e scegli «Riepilogo di oggi» (o «Controlla adesso»).",
];

/** The permission is asked only when the person turns the assistant on, never before. */
export const shouldAskNotifications = (before: unknown, after: unknown): boolean => (before === "off" || before === undefined) && (after === "propose" || after === "act_readonly");

/** Settings that change what the native side schedules (the phone must be told again). */
export const changesSchedule = (key: string): boolean => key.startsWith("proactive.") || key === "app.notifications";
