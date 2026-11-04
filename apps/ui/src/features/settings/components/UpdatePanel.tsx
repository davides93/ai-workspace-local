import { errorMessage } from "../../../shared/lib/errors";
import { formatBytes } from "../../../shared/lib/format";
import { Button, Led, Markdown, Meter, Pill, QueryState, Section } from "../../../shared/ui";
import { useCheckUpdate, useDownloadUpdate, useInstallUpdate, useUpdateStatus } from "../updateApi";
import { CHANNEL_LABEL, downloadPercent, updateStep } from "../model/update";

/** Self-update from the GitHub releases: version, channel, check, verified download and install. */
export function UpdatePanel() {
  const status = useUpdateStatus();
  const check = useCheckUpdate();
  const download = useDownloadUpdate();
  const install = useInstallUpdate();

  return (
    <Section title="Aggiornamenti" hint="Dalle release del repository pubblico su GitHub. Il canale si sceglie nelle impostazioni qui sotto.">
      <QueryState query={status}>
        {(s) => {
          const step = updateStep(s);
          const busy = check.isPending || download.isPending || install.isPending || s.state === "checking";
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
              {error && <p className="crit-text text-sm" role="alert">{error}</p>}
            </div>
          );
        }}
      </QueryState>
    </Section>
  );
}
