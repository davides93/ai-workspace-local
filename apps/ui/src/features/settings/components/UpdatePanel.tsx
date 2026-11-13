import { errorMessage } from "../../../shared/lib/errors";
import { currentLocaleTag, formatBytes } from "../../../shared/lib/format";
import { Button, Led, Markdown, Meter, Pill, QueryState, Section, Field, Select } from "../../../shared/ui";
import { useCheckUpdate, useDownloadUpdate, useInstallUpdate, useUpdateStatus } from "../updateApi";
import { useSetConfig } from "../api";
import { CHANNEL_LABEL, downloadPercent, updateStep } from "../model/update";

import { t, tNode } from "../../../shared/i18n";
/** Self-update from the GitHub releases: version, channel, check, verified download and install. */
export function UpdatePanel() {
  const status = useUpdateStatus();
  const setConfig = useSetConfig();
  const check = useCheckUpdate();
  const download = useDownloadUpdate();
  const install = useInstallUpdate();

  return (
    <Section title={t("settings.aggiornamenti")} hint={t("settings.controlla_quali_versioni_sono_disponibili")}>
      <QueryState query={status}>
        {(s) => {
          const step = updateStep(s);
          const busy = setConfig.isPending || check.isPending || download.isPending || install.isPending || s.state === "checking";
          const error = s.error ?? (check.error && errorMessage(check.error)) ?? (download.error && errorMessage(download.error)) ?? (install.error && errorMessage(install.error));
          return (
            <div className="panel upd">
              <div className="upd-head">
                <div className="stack">
                  <span>Versione installata <b className="mono">{s.current}</b></span>
                  <span className="faint text-sm">
                    Canale <Pill tone="info">{CHANNEL_LABEL[s.channel]}</Pill>
                    {s.configured === "auto" && t("settings.segue_la_versione_installata")}
                    {s.checked_at && t("settings.ultimo_controllo", { p: new Date(s.checked_at).toLocaleString(currentLocaleTag()) })}
                  </span>
                </div>
                <Field label={t("settings.canale_per_questo_dispositivo")} htmlFor="update-channel">
                  <Select id="update-channel" value={s.configured} disabled={busy || s.state === "downloading"}
                    onChange={(event) => setConfig.mutate(
                      { layer: "device", key: "update.channel", value: event.target.value, title: t("settings.canale_di_aggiornamento") },
                      { onSuccess: () => check.mutate() },
                    )}>
                    <option value="auto">{t("settings.automatico")}</option>
                    <option value="stable">{t("settings.stabile")}</option>
                    <option value="beta">{t("settings.beta")}</option>
                    <option value="alpha">{t("settings.alpha")}</option>
                    {s.configured === "rc" && <option value="rc">{t("settings.release_candidate")}</option>}
                  </Select>
                </Field>
                <Button icon="refresh" onClick={() => check.mutate()} disabled={busy || s.state === "downloading"}>
                  {check.isPending || s.state === "checking" ? t("settings.controllo") : t("settings.controlla_ora")}
                </Button>
              </div>

              {step === "none" && s.checked_at && !error && (
                <p className="upd-line text-sm"><Led tone="ok" small /> {" "}{t("settings.sei_aggiornato_nessuna_versione_piu", { p: CHANNEL_LABEL[s.channel] })}</p>
              )}

              {s.available && (
                <div className="upd-release stack">
                  <div className="row row-between">
                    <b>{t("settings.versione", { version: s.available.version })}</b>
                    <Pill tone={s.available.channel === "stable" ? "ok" : "warn"}>{CHANNEL_LABEL[s.available.channel]}</Pill>
                  </div>
                  {s.available.notes && <Markdown text={s.available.notes} className="upd-notes" />}
                  {step === "download" && (
                    <div className="row">
                      <Button variant="primary" icon="download" onClick={() => download.mutate()} disabled={busy}>
                        {t("settings.scarica_e_verifica", { p: s.available.asset ? ` (${formatBytes(s.available.asset.size)})` : "" })}
                      </Button>
                      <span className="faint text-xs">{t("settings.il_file_viene_controllato_con")}</span>
                    </div>
                  )}
                  {step === "downloading" && (
                    <div className="stack" aria-live="polite">
                      <span className="text-sm">{t("settings.scarico_di", { name: s.available.asset?.name ?? "", p: formatBytes(s.received), p2: formatBytes(s.total) })}</span>
                      <Meter value={s.received} max={Math.max(s.total, 1)} label={t("settings.avanzamento_del_download")} />
                      <span className="faint text-xs">{downloadPercent(s)}%</span>
                    </div>
                  )}
                  {step === "install" && (
                    <div className="stack">
                      <p className="text-sm"><Led tone="ok" small /> {" "}{t("settings.installer_scaricato_e_verificato", { p: s.message ?? "" })}</p>
                      <div className="row">
                        <Button variant="primary" icon="play" onClick={() => install.mutate()} disabled={busy}>{t("settings.installa_l_aggiornamento")}</Button>
                        <span className="faint text-xs mono ellipsis" title={s.file ?? undefined}>{s.file}</span>
                      </div>
                    </div>
                  )}
                  {step === "manual" && (
                    <p className="text-sm">{tNode("settings.questa_release_non_ha_un", { a: <a href={s.available.page} target="_blank" rel="noreferrer">{t("settings.aprila_su_github")}</a> })}</p>
                  )}
                </div>
              )}
              {!!s.releases?.length && (
                <details>
                  <summary>{t("settings.release_nel_canale", { p: CHANNEL_LABEL[s.channel], length: s.releases.length })}</summary>
                  <ul>
                    {s.releases.map((release) => (
                      <li key={release.version}>
                        <a href={release.page} target="_blank" rel="noreferrer">{t("settings.note_di_rilascio", { version: release.version })}</a>
                        {" · "}{release.version === s.current ? t("settings.installata") : release.asset ? t("settings.installer_per_questo_dispositivo") : t("settings.installer_assente")}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              {error && <p className="crit-text text-sm" role="alert">{error}</p>}
            </div>
          );
        }}
      </QueryState>
    </Section>
  );
}
