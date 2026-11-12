import { describe, expect, it } from "vitest";
import type { BackgroundStatus } from "../../../shared/api/types";
import { changesSchedule, isPhoneOs, notificationLine, scheduleLine, SHORTCUT_STEPS, shouldAskNotifications } from "./background";

const status = (over: Partial<BackgroundStatus> = {}): BackgroundStatus => ({ platform: "ios", supported: true, notifications: "granted", scheduled: true, ...over });

describe("background block", () => {
  it("is for phones and tablets only", () => {
    expect(isPhoneOs("ios") && isPhoneOs("android")).toBe(true);
    for (const os of ["windows", "macos", "linux", "", undefined]) expect(isPhoneOs(os)).toBe(false);
  });

  it("says what the notification permission means, and when the button has something to do", () => {
    expect(notificationLine(status({ notifications: "granted" }))).toMatchObject({ tone: "ok", canAsk: false, inSystemSettings: false });
    expect(notificationLine(status({ notifications: "not_determined" }))).toMatchObject({ tone: "warn", canAsk: true });
    expect(notificationLine(status({ notifications: "denied" }))).toMatchObject({ tone: "warn", canAsk: false, inSystemSettings: true });
    expect(notificationLine(status({ notifications: "unsupported" }))).toMatchObject({ tone: "muted", canAsk: false });
    expect(notificationLine(undefined).canAsk).toBe(false);
    expect(notificationLine(status({ notifications: "denied" })).text).toMatch(/impostazioni del telefono/);
  });

  it("describes the schedule honestly: off, not yet scheduled, or the system's own promise", () => {
    expect(scheduleLine(status(), "off")).toMatch(/spento/);
    expect(scheduleLine(status(), undefined)).toMatch(/spento/);
    expect(scheduleLine(status({ scheduled: false }), "propose")).toMatch(/non sono ancora in programma/);
    expect(scheduleLine(status({ detail: "iOS decide quando." }), "propose")).toBe("iOS decide quando.");
  });

  it("gives the three steps of the timed Shortcut, in plain Italian", () => {
    expect(SHORTCUT_STEPS).toHaveLength(3);
    expect(SHORTCUT_STEPS.join(" ")).toMatch(/Automazione.*Ora del giorno.*Riepilogo di oggi/);
  });

  it("asks for the permission only when the assistant goes from off to on", () => {
    expect(shouldAskNotifications("off", "propose")).toBe(true);
    expect(shouldAskNotifications(undefined, "act_readonly")).toBe(true);
    expect(shouldAskNotifications("propose", "act_readonly")).toBe(false);
    expect(shouldAskNotifications("propose", "off")).toBe(false);
    expect(shouldAskNotifications("off", "off")).toBe(false);
  });

  it("tells the phone again when a proactive setting changes", () => {
    for (const k of ["proactive.level", "proactive.interval_minutes", "proactive.briefing_time", "proactive.only_on_power", "app.notifications"]) expect(changesSchedule(k)).toBe(true);
    for (const k of ["ui.mode", "memory.enabled", "connectors.mail.enabled"]) expect(changesSchedule(k)).toBe(false);
  });
});
