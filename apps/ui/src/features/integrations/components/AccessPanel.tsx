import { Link } from "../../../app/router";
import { Icon, Panel, Pill, Segmented } from "../../../shared/ui";

import { t, tNode } from "../../../shared/i18n";
export type Audience = "local" | "lan";

/** Who calls the endpoints, and what that client needs: nothing from this computer, the pairing token from the LAN. */
export function AccessPanel({ audience, onAudience, lanAddress }: {
  audience: Audience;
  onAudience: (a: Audience) => void;
  /** LAN address of this device, `null` when the server listens on this computer only. */
  lanAddress: string | null | undefined;
}) {
  return (
    <Panel
      icon="lock" title={t("integrations.accesso_e_sicurezza")}
      actions={(
        <Segmented
          label={t("integrations.da_dove_si_collega_il")} value={audience} onChange={onAudience}
          options={[{ id: "local", label: t("integrations.da_questo_computer") }, { id: "lan", label: t("integrations.da_un_altro_dispositivo") }]}
        />
      )}
    >
      <div className="stack">
        {audience === "lan" && !lanAddress && (
          <p className="int-warn text-sm">
            <Icon name="alert" size={15} />
            <span>{tNode("integrations.il_server_ascolta_solo_su", { link: <Link to="/settings">{t("integrations.impostazioni")}</Link>, code: <code>{t("integrations.aiw_serve_lan")}</code> })}</span>
          </p>
        )}
        <div className="int-notes">
          <section className="int-note" aria-label={t("integrations.da_questo_computer")}>
            <h3><Icon name="device" size={16} /> {" "}{t("integrations.da_questo_computer")}</h3>
            <p className="muted text-sm">
              {tNode("integrations.script_curl_e_sdk_rispondono", { code: <code>{t("integrations.host")}</code>, code2: <code>{t("integrations.origin")}</code>, code3: <code>127.0.0.1</code> })}</p>
          </section>
          <section className="int-note" aria-label={t("integrations.pagine_web_nel_browser")}>
            <h3><Icon name="globe" size={16} /> {" "}{t("integrations.pagine_web_nel_browser")}</h3>
            <p className="muted text-sm">
              {tNode("integrations.anche_le_richieste_da_localhost", { code: <code>{t("integrations.origin")}</code>, code2: <code>{t("integrations.localhost")}</code>, code3: <code>127.0.0.1</code> })}</p>
          </section>
          <section className="int-note" aria-label={t("integrations.da_altri_dispositivi")}>
            <h3>
              <Icon name="link" size={16} /> Da altri dispositivi
              {lanAddress ? <Pill tone="ok">{t("integrations.lan_attiva")}</Pill> : <Pill>{t("integrations.solo_locale")}</Pill>}
            </h3>
            <p className="muted text-sm">
              {tNode("integrations.servono_l_accesso_dalla_lan", { code: <code>{t("integrations.authorization_bearer")}</code>, code2: <code>{t("integrations.x_api_key")}</code>, link: <Link to="/devices">{t("integrations.dispositivi")}</Link> })}</p>
          </section>
        </div>
      </div>
    </Panel>
  );
}
