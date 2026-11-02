import { useMemo, useState } from "react";
import { Button, Chip, Drawer, EmptyState, Icon, Pill, Segmented, SkeletonLines } from "../../../shared/ui";
import { formatCompact, formatParams, relTime } from "../../../shared/lib/format";
import type { RemoteModel } from "../../../shared/api/types";
import { useDownloads, useModelProviders, useRemoteModel } from "../api";
import { providerName, pullRevision } from "../model/providers";
import { findJob, groupVariants, sharedRuntimes, visibleVariants } from "../model/variants";
import { CapabilityChips, ChipRow, RuntimeChips } from "./common";
import { HubError } from "./HubError";
import { useProviderAuthDialog } from "./ProviderAuthDialog";
import { VariantRow } from "./VariantRow";

/** Side panel with every variant of a remote model: what it weighs, whether it fits, who can run it, and Scarica per row. */
export function VariantsDrawer({ model, onClose }: { model: RemoteModel; onClose: () => void }) {
  const detail = useRemoteModel(model.provider, model.id);
  const downloads = useDownloads();
  const providers = useModelProviders();
  const auth = useProviderAuthDialog();
  const [fitOnly, setFitOnly] = useState(false);

  // The detail call knows more than the search hit (and is the only source for an exact name typed by hand).
  const info = detail.data?.model ?? model;
  const provider = providers.data?.find((p) => p.id === model.provider);
  const groups = useMemo(() => groupVariants(visibleVariants(detail.data?.variants ?? [], fitOnly)), [detail.data, fitOnly]);
  const total = detail.data?.variants.length ?? 0;
  const hidden = total - groups.reduce((n, g) => n + g.variants.length, 0);
  const revision = pullRevision(model.provider, detail.data?.revision);

  return (
    <>
      <Drawer title={info.name} subtitle={`${providerName(model.provider, providers.data)} · ${info.id}`} onClose={onClose}>
        <ModelSummary model={info} />
        {info.gated && !provider?.auth_configured && !detail.isError && (
          <p className="mh-alert warn" role="note">
            <Icon name="lock" size={15} />
            <span className="grow">Repository gated: per scaricarlo serve un token con accesso a questo modello.</span>
            <Button size="sm" icon="key" onClick={() => auth.open(model.provider)}>Imposta token</Button>
          </p>
        )}

        {detail.isPending && <SkeletonLines lines={6} />}
        {detail.isError && (
          <HubError error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} onSetToken={() => auth.open(model.provider)} />
        )}
        {detail.data && (
          <section className="stack stack-lg" aria-label="Varianti">
            <div className="row row-between">
              <h3>{total === 1 ? "1 variante" : `${total} varianti`}</h3>
              {total > 3 && (
                <Segmented
                  label="Filtro varianti" value={fitOnly ? "fit" : "all"} onChange={(v) => setFitOnly(v === "fit")}
                  options={[{ id: "all", label: "Tutte" }, { id: "fit", label: "Che ci stanno" }]}
                />
              )}
            </div>
            {total === 0 && <EmptyState icon="cube" title="Nessuna variante scaricabile" description="Il provider non elenca file utilizzabili per questo modello." />}
            {groups.map((g) => {
              const shared = sharedRuntimes(g.variants);
              return (
                <div key={g.format} className="stack">
                  {groups.length > 1 && <h4 className="eyebrow">Formato .{g.format}</h4>}
                  {shared && (
                    <div className="stack stack-sm">
                      <span className="field-label">Runtime compatibili con tutte le varianti</span>
                      <RuntimeChips runtimes={shared} explain />
                    </div>
                  )}
                  <ul className="mh-variants">
                    {g.variants.map((v) => (
                      <VariantRow
                        key={v.id} variant={v} provider={model.provider} modelId={info.id} modelName={info.name} revision={revision}
                        job={findJob(downloads.data ?? [], { provider: model.provider, model: info.id, variant: v.id })}
                        showRuntimes={!shared}
                        onSetToken={() => auth.open(model.provider)}
                      />
                    ))}
                  </ul>
                </div>
              );
            })}
            {hidden > 0 && <p className="faint text-sm">{hidden === 1 ? "Una variante che non ci sta è nascosta." : `${hidden} varianti che non ci stanno sono nascoste.`}</p>}
          </section>
        )}
      </Drawer>
      {auth.dialog}
    </>
  );
}

function ModelSummary({ model: m }: { model: RemoteModel }) {
  return (
    <div className="stack">
      <div className="row">
        {m.publisher && <span className="muted text-sm">{m.publisher}</span>}
        {m.license && <Chip>{m.license}</Chip>}
        {m.gated && <Pill tone="warn"><Icon name="lock" size={12} /> Gated</Pill>}
        {m.url && (
          <a className="link-more ml-auto text-sm" href={m.url} target="_blank" rel="noreferrer noopener">
            Pagina del modello <Icon name="external" size={13} />
            <span className="sr-only"> (si apre in una nuova scheda)</span>
          </a>
        )}
      </div>
      {m.description && <p className="muted text-sm">{m.description}</p>}
      <ChipRow label="Formati">{m.formats.map((f) => <Chip key={f}>.{f}</Chip>)}</ChipRow>
      <CapabilityChips items={m.capabilities} />
      <p className="faint text-xs num">
        {[
          m.downloads !== undefined && `${formatCompact(m.downloads)} download`,
          m.likes !== undefined && m.likes > 0 && `${formatCompact(m.likes)} like`,
          m.params_b !== undefined && `${formatParams(m.params_b)} parametri`,
          m.updated_at && `aggiornato ${relTime(m.updated_at)}`,
        ].filter(Boolean).join(" · ")}
      </p>
    </div>
  );
}
