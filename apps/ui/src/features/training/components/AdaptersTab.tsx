import { useState } from "react";
import type { UseQueryResult } from "@tanstack/react-query";
import { Link, navigate } from "../../../app/router";
import { Button, CopyButton, DataTable, DecisionDialog, EmptyState, QueryState, Section, type Column } from "../../../shared/ui";
import { formatDateTime, relTime } from "../../../shared/lib/format";
import { errorMessage } from "../../../shared/lib/errors";
import type { AdapterInfo } from "../../../shared/api/types";
import { useSaveProfile } from "../../agents/api";
import { useDeleteAdapter } from "../api";
import { adapterRuntime, METHOD_LABEL, metricsLine } from "../model/training";

import { t } from "../../../shared/i18n";
/** Makes a profile that loads the adapter on its base model, for the formats a runtime can load. */
function UseButton({ a }: { a: AdapterInfo }) {
  const save = useSaveProfile();
  const runtime = adapterRuntime(a);
  if (!runtime) {
    return <span className="faint text-xs">{t("training.per_usarlo_convertilo_in_gguf")}</span>;
  }
  return (
    <>
      <Button
        size="sm" disabled={save.isPending}
        onClick={() =>
          save.mutate(
            { id: `ft-${a.id}`, name: a.name, description: t("training.adapter_profile_description", { base: a.base_model, name: a.name }), runtime_id: runtime, model: a.base_model, params: { adapters: [{ path: a.path }] } },
            { onSuccess: () => navigate("/models/profiles") },
          )}
      >
        {t("training.usa_in_un_profilo")}</Button>
      {save.isError && <span className="field-error" role="alert">{errorMessage(save.error)}</span>}
    </>
  );
}

/** The adapters AIW trained; each one is a folder a profile can name. */
export function AdaptersTab({ adapters }: { adapters: UseQueryResult<AdapterInfo[]> }) {
  const [removing, setRemoving] = useState<AdapterInfo | null>(null);
  const del = useDeleteAdapter();
  const columns: Column<AdapterInfo>[] = [
    { key: "name", header: t("training.adapter"), render: (a) => <><b>{a.name}</b><div className="faint text-xs mono">{a.id}</div></> },
    { key: "base", header: t("training.modello_base"), render: (a) => <span className="mono text-sm">{a.base_model}</span> },
    { key: "method", header: t("training.metodo"), nowrap: true, render: (a) => `${METHOD_LABEL[a.method]} · ${a.trainer}` },
    { key: "metrics", header: t("training.metriche"), render: (a) => <span className="text-sm">{metricsLine(a.metrics) || "—"}</span> },
    { key: "when", header: t("training.creato"), nowrap: true, render: (a) => <span title={formatDateTime(a.created_at)}>{relTime(a.created_at)}</span> },
    {
      key: "act", header: "", render: (a) => (
        <div className="tr-actions">
          <UseButton a={a} />
          <CopyButton text={a.path} label={t("training.copia_il_percorso")} />
          <Button size="sm" variant="ghost" icon="trash" aria-label={t("training.elimina", { name: a.name })} onClick={() => setRemoving(a)} />
        </div>
      ),
    },
  ];
  return (
    <Section title={t("training.adapter")} hint={t("training.un_adapter_va_caricato_insieme")}>
      <QueryState
        query={adapters} isEmpty={(a) => a.length === 0}
        empty={<EmptyState icon="cube" title={t("training.nessun_adapter")} description={t("training.quando_un_addestramento_finisce_l")} action={<Link to="/training/jobs" className="btn primary">{t("training.vai_agli_addestramenti")}</Link>} />}
      >
        {(all) => <DataTable label={t("training.adapter_addestrati")} columns={columns} rows={all} rowKey={(a) => a.id} />}
      </QueryState>
      {removing && (
        <DecisionDialog
          title={t("training.eliminare_l_adapter")} confirmLabel={t("training.elimina_2")} icon="trash" pending={del.isPending} error={del.error ?? undefined}
          onCancel={() => { setRemoving(null); del.reset(); }} onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>{t("training.e_i_suoi_file_vengono", { name: removing.name })}</p>
        </DecisionDialog>
      )}
    </Section>
  );
}
