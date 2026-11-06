import { errorMessage } from "../../../shared/lib/errors";
import { formatBytes } from "../../../shared/lib/format";
import { Button, Led, Markdown, Meter, Pill, QueryState, Section, Field, Select } from "../../../shared/ui";
import { useCheckUpdate, useDownloadUpdate, useInstallUpdate, useUpdateStatus } from "../updateApi";
import { useSetConfig } from "../api";
import { CHANNEL_LABEL, downloadPercent, updateStep } from "../model/update";

/** Self-update from the GitHub releases: version, channel, check, verified download and install. */
export function UpdatePanel() {
  const status = useUpdateStatus();
  const setConfig = useSetConfig();
  const check = useCheckUpdate();
  const download = useDownloadUpdate();
  const install = useInstallUpdate();

  return (
    <Section title="Aggiornamenti" hint="Controlla quali versioni sono disponibili per questo dispositivo.">
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
                    {s.configured === "auto" && " (segue la versione installata)"}
                    {s.checked_at && ` · ultimo controllo ${new Date(s.checked_at).toLocaleString("it-IT")}`}
                  </span>
                </div>
                <Field label="Canale per questo dispositivo" htmlFor="update-channel">
                  <Select id="update-channel" value={s.configured} disabled={busy || s.state === "downloading"}
                    onChange={(event) => setConfig.mutate(
                      { layer: "device", key: "update.channel", value: event.target.value, title: "Canale di aggiornamento" },
                      { onSuccess: () => check.mutate() },
                    )}>
                    <option value="auto">Automatico</option>
                    <option value="stable">Stabile</option>
                    <option value="beta">Beta</option>
                    <option value="alpha">Alpha</option>
                    {s.configured === "rc" && <option value="rc">Release candidate</option>}
                  </Select>
                </Field>
                <Button icon="refresh" onClick={() => check.mutate()} disabled={busy || s.state === "downloading"}>
                  {check.isPending || s.state === "checking" ? "Controllo…" : "Controlla ora"}
                </Button>
              </div>

              {step === "none" && s.checked_at && !error && (
                <p className="upd-line text-sm"><Led tone="ok" small /> Sei aggiornato: nessuna versione più recente nel canale {CHANNEL_LABEL[s.channel]}.</p>
              )}

              {s.available && (
                <div className="upd-release stack">
                  <div className="row row-between">
                    <b>Versione {s.available.version}</b>
                    <Pill tone={s.available.channel === "stable" ? "ok" : "warn"}>{CHANNEL_LABEL[s.available.channel]}</Pill>
                  </div>
                  {s.available.notes && <Markdown text={s.available.notes} className="upd-notes" />}
                  {step === "download" && (
                    <div className="row">
                      <Button variant="primary" icon="download" onClick={() => download.mutate()} disabled={busy}>
                        Scarica e verifica{s.available.asset ? ` (${formatBytes(s.available.asset.size)})` : ""}
                      </Button>
                      <span className="faint text-xs">Il file viene controllato con lo SHA-256 pubblicato nella release.</span>
                    </div>
                  )}
                  {step === "downloading" && (
                    <div className="stack" aria-live="polite">
                      <span className="text-sm">Scarico {s.available.asset?.name}: {formatBytes(s.received)} di {formatBytes(s.total)}</span>
                      <Meter value={s.received} max={Math.max(s.total, 1)} label="Avanzamento del download" />
                      <span className="faint text-xs">{downloadPercent(s)}%</span>
                    </div>
                  )}
                  {step === "install" && (
                    <div className="stack">
                      <p className="text-sm"><Led tone="ok" small /> Installer scaricato e verificato. {s.message ?? ""}</p>
                      <div className="row">
                        <Button variant="primary" icon="play" onClick={() => install.mutate()} disabled={busy}>Installa l'aggiornamento</Button>
                        <span className="faint text-xs mono ellipsis" title={s.file ?? undefined}>{s.file}</span>
                      </div>
                    </div>
                  )}
                  {step === "manual" && (
                    <p className="text-sm">Questa release non ha un installer per questo sistema: <a href={s.available.page} target="_blank" rel="noreferrer">aprila su GitHub</a>.</p>
                  )}
                </div>
              )}
              {!!s.releases?.length && (
                <details>
                  <summary>Release nel canale {CHANNEL_LABEL[s.channel]} ({s.releases.length})</summary>
                  <ul>
                    {s.releases.map((release) => (
                      <li key={release.version}>
                        <a href={release.page} target="_blank" rel="noreferrer">{release.version} · Note di rilascio</a>
                        {" · "}{release.version === s.current ? "Installata" : release.asset ? "Installer per questo dispositivo" : "Installer assente"}
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
