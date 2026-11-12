import { Button, Pill } from "../../../shared/ui";
import { useBackgroundActions, useBackgroundStatus } from "../api";
import { isPhoneOs, notificationLine, scheduleLine, SHORTCUT_STEPS } from "../model/background";
import type { ProactiveLevel } from "../../../shared/api/types";

/** «In background» in the proactive section: only on a phone, where the system decides when the assistant may look around. */
export function BackgroundBlock({ os, level }: { os: string | undefined; level: ProactiveLevel | undefined }) {
  const phone = isPhoneOs(os);
  const status = useBackgroundStatus(phone);
  const actions = useBackgroundActions();
  if (!phone || !status.data?.supported) return null;
  const note = notificationLine(status.data);
  return (
    <div className="panel set-bg" data-testid="background-block">
      <h3 className="set-bg-title">In background</h3>
      <p className="set-bg-line">
        <Pill tone={note.tone === "ok" ? "ok" : note.tone === "warn" ? "warn" : "info"}>{note.tone === "ok" ? "Notifiche attive" : "Notifiche"}</Pill>
        <span>{note.text}</span>
      </p>
      <div className="set-bg-actions">
        {note.canAsk && <Button variant="primary" onClick={() => actions.askNotifications.mutate()} disabled={actions.askNotifications.isPending}>Attiva le notifiche</Button>}
        <Button onClick={() => actions.checkNow.mutate()} disabled={actions.checkNow.isPending || level === "off"}>Controlla adesso</Button>
      </div>
      <p className="set-hint">{scheduleLine(status.data, level)}</p>
      {os === "ios" && (
        <>
          <p className="set-hint">Su iPhone è iOS a decidere quando l'app può lavorare in background: può essere meno spesso di quanto ti aspetti. Per un controllo a un'ora precisa usa un Comando:</p>
          <ol className="set-bg-steps">{SHORTCUT_STEPS.map((s) => <li key={s}>{s}</li>)}</ol>
        </>
      )}
    </div>
  );
}
