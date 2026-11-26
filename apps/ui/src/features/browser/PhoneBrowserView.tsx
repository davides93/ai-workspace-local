import { useCallback } from "react";
import { Button, ErrorState, Sheet } from "../../shared/ui";
import "./browser.css";
import { minutesLeft, useViewFrames, useViewRequest, useViewStatus } from "./view";

import { t } from "../../shared/i18n";
/**
 * «Il computer dell'agente» on a phone paired with the computer: ask to look, the computer says yes (a short, single-use yes), then the
 * page as an image, refreshed every second. Looking only: nothing here clicks, types, pauses the agent or takes the browser, and the
 * phone cannot answer for the computer. Nothing of what is shown is kept once the sheet is closed.
 */
export function PhoneBrowserView({ onClose }: { onClose: () => void }) {
  const status = useViewStatus(true);
  const request = useViewRequest();
  const s = status.data;
  const granted = s?.state === "granted";
  const ended = useCallback(() => void status.refetch(), [status]);
  const frame = useViewFrames(granted, ended);
  return (
    <Sheet title={t("browser.view_title")} onClose={onClose}>
      <div className="stack stack-lg">
        {status.isError && <ErrorState error={status.error} title={t("browser.view_unavailable")} onRetry={() => void status.refetch()} />}
        {(!s || s.state === "none") && !status.isError && (
          <div className="stack">
            <p className="muted">{t("browser.view_intro")}</p>
            <Button variant="primary" icon="globe" disabled={request.isPending} onClick={() => request.mutate()}>{t("browser.view_ask")}</Button>
          </div>
        )}
        {s?.state === "pending" && (
          <p role="status" className="stack">
            <b>{t("browser.view_pending")}</b>
            <span className="muted">{t("browser.view_pending_hint", { left: minutesLeft(s.seconds_left) })}</span>
          </p>
        )}
        {granted && (
          <div className="stack">
            <p className="muted" role="status">{t("browser.view_granted", { left: minutesLeft(s.seconds_left) })}</p>
            {frame ? (
              <img className="view-frame" src={frame.image} width={frame.width} height={frame.height} alt={t("browser.view_frame_alt")} />
            ) : (
              <p className="muted">{t("browser.view_no_browser")}</p>
            )}
            <p className="muted text-xs">{t("browser.view_only")}</p>
          </div>
        )}
      </div>
    </Sheet>
  );
}
