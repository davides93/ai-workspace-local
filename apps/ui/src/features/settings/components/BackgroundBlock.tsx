import { Button, Pill } from "../../../shared/ui";
import { useBackgroundActions, useBackgroundStatus } from "../api";
import { isPhoneOs, notificationLine, scheduleLine, SHORTCUT_STEPS } from "../model/background";
import type { ProactiveLevel } from "../../../shared/api/types";

import { t } from "../../../shared/i18n";
/** «In background» in the proactive section: only on a phone, where the system decides when the assistant may look around. */
export function BackgroundBlock({ os, level }: { os: string | undefined; level: ProactiveLevel | undefined }) {
  const phone = isPhoneOs(os);
  const status = useBackgroundStatus(phone);
  const actions = useBackgroundActions();
  if (!phone || !status.data?.supported) return null;
  const note = notificationLine(status.data);
  return (
    <div className="panel set-bg" data-testid="background-block">
      <h3 className="set-bg-title">{t("settings.in_background")}</h3>
      <p className="set-bg-line">
        <Pill tone={note.tone === "ok" ? "ok" : note.tone === "warn" ? "warn" : "info"}>{note.tone === "ok" ? t("settings.notifiche_attive") : t("settings.notifiche")}</Pill>
        <span>{note.text}</span>
      </p>
      <div className="set-bg-actions">
        {note.canAsk && <Button variant="primary" onClick={() => actions.askNotifications.mutate()} disabled={actions.askNotifications.isPending}>{t("settings.attiva_le_notifiche")}</Button>}
        <Button onClick={() => actions.checkNow.mutate()} disabled={actions.checkNow.isPending || level === "off"}>{t("settings.controlla_adesso")}</Button>
      </div>
      <p className="set-hint">{scheduleLine(status.data, level)}</p>
      {os === "ios" && (
        <>
          <p className="set-hint">{t("settings.su_iphone_e_ios_a")}</p>
          <ol className="set-bg-steps">{SHORTCUT_STEPS.map((s) => <li key={s}>{s}</li>)}</ol>
        </>
      )}
    </div>
  );
}
