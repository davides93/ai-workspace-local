import { Button, Chip, Icon, Meter, Pill } from "../../../shared/ui";
import { formatBytes, formatPercent, formatSpeed } from "../../../shared/lib/format";
import { toast } from "../../../shared/lib/toast";
import type { DownloadProgress, ModelVariant } from "../../../shared/api/types";
import { useCancelDownload, usePullModel } from "../api";
import { etaSeconds, formatEta, progressRatio } from "../model/downloads";
import { FILE_ROLE_LABEL, fileCountLabel } from "../model/variants";
import { FitPill, RuntimeChips } from "./common";
import { InlineError } from "./HubError";

/** One downloadable variant of a remote model: size, fit for this device, files, runtimes and its own download button. */
export function VariantRow({ variant: v, provider, modelId, modelName, revision, job, showRuntimes, onSetToken }: {
  variant: ModelVariant;
  provider: string;
  modelId: string;
  modelName: string;
  revision?: string;
  /** False when the drawer already lists the runtimes shared by every variant. */
  showRuntimes: boolean;
  /** The active download of this very variant, when there is one. */
  job?: DownloadProgress;
  onSetToken: () => void;
}) {
  const pull = usePullModel({ handled: true });
  const cancel = useCancelDownload();
  const blocked = v.fit === "no";
  const start = () =>
    pull.mutate(
      { provider, model: modelId, variant: v.id, revision },
      { onSuccess: () => toast(`Download avviato: ${modelName} · ${v.label}`) },
    );

  return (
    <li className="mh-variant" data-recommended={v.recommended || undefined}>
      <div className="mh-variant-head">
        <b className="mono ellipsis" title={v.label}>{v.label}</b>
        <span className="mh-badges">
          {v.recommended && <Pill tone="run" title="La scelta migliore per questo device">Consigliata</Pill>}
          {v.installed && <Pill tone="ok"><Icon name="check" size={12} /> Installata</Pill>}
          <FitPill fit={v.fit} detail={v.fit_detail} />
        </span>
      </div>

      <p className="mh-variant-meta num">
        <b>{formatBytes(v.size_bytes)}</b> · {fileCountLabel(v.files)}
        {v.quant && !v.label.startsWith(v.quant) && <> · <Chip>{v.quant}</Chip></>}
      </p>
      {v.fit_detail && <p className="faint text-xs">{v.fit_detail}</p>}
      {showRuntimes && <RuntimeChips runtimes={v.runtimes} explain />}

      {v.files.length > 1 && (
        <details className="mh-files">
          <summary>Elenco dei file ({v.files.length})</summary>
          <ul>
            {v.files.map((f) => (
              <li key={f.path}>
                <span className="mono ellipsis" title={f.path}>{f.path}</span>
                <Chip>{FILE_ROLE_LABEL[f.role] ?? f.role}</Chip>
                <span className="num faint">{formatBytes(f.size_bytes)}</span>
              </li>
            ))}
          </ul>
        </details>
      )}

      {job ? (
        <div className="mh-variant-progress">
          <Meter label={`Download di ${v.label}`} value={job.received} max={job.total} />
          <div className="row row-between text-xs faint num">
            <span>
              {job.status === "queued" ? "In coda" : job.status === "verifying" ? "Verifica sha256…" : formatPercent(progressRatio(job))}
              {job.status === "running" && job.speed_bps > 0 && ` · ${formatSpeed(job.speed_bps)}`}
              {job.status === "running" && etaSeconds(job) !== null && ` · mancano ${formatEta(etaSeconds(job))}`}
            </span>
            <Button size="sm" variant="ghost" onClick={() => cancel.mutate(job.id)} disabled={cancel.isPending}>Annulla</Button>
          </div>
        </div>
      ) : (
        !v.installed && (
          <div className="mh-variant-action">
            <Button
              variant={v.recommended ? "primary" : "default"} size="sm" icon="download"
              disabled={blocked || pull.isPending} onClick={start}
              title={blocked ? "Questa variante non ci sta su questo device" : undefined}
            >
              {pull.isPending ? "Avvio…" : `Scarica ${formatBytes(v.size_bytes)}`}
            </Button>
            {blocked && <span className="faint text-xs">Non ci sta su questo device</span>}
          </div>
        )
      )}
      {pull.isError && <InlineError error={pull.error} onSetToken={onSetToken} />}
    </li>
  );
}
