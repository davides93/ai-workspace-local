import { useSystem } from "../../shared/query/system";
import { Button, Sheet } from "../../shared/ui";
import "./browser.css";
import { minutesLeft, useViewDecision, useViewRequests } from "./view";

import { t } from "../../shared/i18n";
/**
 * On the computer: a paired phone asks to look at the browser of the agent. Nothing is shown to it until the person says yes here;
 * the yes is for ten minutes, for that phone, and shows images only. While a phone looks, a bar says so with «Basta».
 * Not asked on a screen that talks to another computer (that is the phone), and silent when this engine has no such request.
 */
export function ViewRequestPrompt() {
  const { data: system } = useSystem();
  const asking = system !== undefined && system.mode !== "daemon";
  const q = useViewRequests(asking);
  const { decide, revoke } = useViewDecision();
  const first = q.data?.requests[0];
  const viewers = q.data?.viewers ?? [];
  return (
    <>
      {first && (
        <Sheet
          title={t("browser.ask_title")}
          onClose={() => decide.mutate({ id: first.id, approve: false })}
          footer={
            <>
              <Button variant="primary" disabled={decide.isPending} onClick={() => decide.mutate({ id: first.id, approve: true })}>{t("browser.ask_allow")}</Button>
              <Button disabled={decide.isPending} onClick={() => decide.mutate({ id: first.id, approve: false })}>{t("browser.ask_deny")}</Button>
            </>
          }
        >
          <div className="stack">
            <p>{t("browser.ask_body", { device: first.device })}</p>
            <p className="muted">{t("browser.ask_hint", { left: minutesLeft(first.expires_in_s) })}</p>
          </div>
        </Sheet>
      )}
      {viewers.length > 0 && (
        <div className="view-bar" role="status">
          <span>{t("browser.viewing_now", { devices: viewers.join(", ") })}</span>
          <Button size="sm" variant="danger" disabled={revoke.isPending} onClick={() => revoke.mutate()}>{t("browser.viewing_stop")}</Button>
        </div>
      )}
    </>
  );
}
