import { Pill, Toggle } from "../../../shared/ui";
import { t } from "../../../shared/i18n";
import { useConfig, useSetConfig } from "../api";
import { isLocalOnly, localOnlyWrites, privacyLines } from "../model/privacy";

/**
 * «Privacy e controllo»: what stays on the device, what can leave it, and one switch, «Solo locale», that turns off the
 * web search, the automatic update checks and the web reading of the proactive assistant (settings that exist already).
 * It says what it does not stop: model downloads on request, the agent's browser, channels, MCP servers the person connected.
 */
export function PrivacyPanel() {
  const cfg = useConfig();
  const setConfig = useSetConfig();
  const eff = cfg.data?.effective;
  if (!eff) return null;
  const on = isLocalOnly(eff);
  const lines = privacyLines(eff);
  const apply = (next: boolean) => {
    for (const [key, value] of localOnlyWrites(next, eff)) setConfig.mutate({ layer: "global", key, value, title: key });
  };
  return (
    <section className="panel set-privacy" aria-labelledby="privacy-title" data-testid="privacy-panel">
      <div className="set-privacy-head">
        <div className="set-mode-text">
          <b id="privacy-title">{t("settings.privacy_e_controllo")}</b>
          <span className="faint text-sm">{t("settings.privacy_solo_locale_dettaglio")}</span>
        </div>
        <Toggle checked={on} onChange={apply} label={t("settings.privacy_solo_locale")} disabled={setConfig.isPending} />
      </div>
      <p className="set-privacy-state"><Pill tone={on ? "ok" : "neutral"}>{on ? t("settings.privacy_solo_locale_attivo") : t("settings.privacy_solo_locale_spento")}</Pill></p>
      <ul className="set-privacy-list" aria-label={t("settings.privacy_cosa_esce")}>
        {lines.map((l) => (
          <li key={l.id} data-leaves={l.leaves ? "yes" : "no"}>{l.text}</li>
        ))}
      </ul>
      <p className="set-hint">{t("settings.privacy_non_blocca")}</p>
    </section>
  );
}
