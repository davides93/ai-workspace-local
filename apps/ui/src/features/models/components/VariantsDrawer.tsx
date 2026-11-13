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

import { t, tNode } from "../../../shared/i18n";
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
            <span className="grow">{t("models.repository_gated_per_scaricarlo_serve")}</span>
            <Button size="sm" icon="key" onClick={() => auth.open(model.provider)}>{t("models.imposta_token")}</Button>
          </p>
        )}

        {detail.isPending && <SkeletonLines lines={6} />}
        {detail.isError && (
          <HubError error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} onSetToken={() => auth.open(model.provider)} />
        )}
        {detail.data && (
          <section className="stack stack-lg" aria-label={t("models.varianti")}>
            <div className="row row-between">
              <h3>{total === 1 ? t("models.1_variante") : t("models.varianti_2", { total })}</h3>
              {total > 3 && (
                <Segmented
                  label={t("models.filtro_varianti")} value={fitOnly ? "fit" : "all"} onChange={(v) => setFitOnly(v === "fit")}
                  options={[{ id: "all", label: t("models.tutte_2") }, { id: "fit", label: t("models.che_ci_stanno") }]}
                />
              )}
            </div>
            {total === 0 && <EmptyState icon="cube" title={t("models.nessuna_variante_scaricabile")} description={t("models.il_provider_non_elenca_file")} />}
            {groups.map((g) => {
              const shared = sharedRuntimes(g.variants);
              return (
                <div key={g.format} className="stack">
                  {groups.length > 1 && <h4 className="eyebrow">{t("models.formato_2", { format: g.format })}</h4>}
                  {shared && (
                    <div className="stack stack-sm">
                      <span className="field-label">{t("models.runtime_compatibili_con_tutte_le")}</span>
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
            {hidden > 0 && <p className="faint text-sm">{hidden === 1 ? t("models.una_variante_che_non_ci") : t("models.varianti_che_non_ci_stanno", { hidden })}</p>}
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
        {m.gated && <Pill tone="warn"><Icon name="lock" size={12} /> {" "}{t("models.gated")}</Pill>}
        {m.url && (
          <a className="link-more ml-auto text-sm" href={m.url} target="_blank" rel="noreferrer noopener">
            {tNode("models.pagina_del_modello", { el: <Icon name="external" size={13} />, span: <span className="sr-only">{t("models.si_apre_in_una_nuova")}</span> })}
          </a>
        )}
      </div>
      {m.description && <p className="muted text-sm">{m.description}</p>}
      <ChipRow label={t("models.formati")}>{m.formats.map((f) => <Chip key={f}>.{f}</Chip>)}</ChipRow>
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
