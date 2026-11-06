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

/** Makes a profile that loads the adapter on its base model, for the formats a runtime can load. */
function UseButton({ a }: { a: AdapterInfo }) {
  const save = useSaveProfile();
  const runtime = adapterRuntime(a);
  if (!runtime) {
    return <span className="faint text-xs">Per usarlo convertilo in GGUF (convert_lora_to_gguf.py di llama.cpp)</span>;
  }
  return (
    <>
      <Button
        size="sm" disabled={save.isPending}
        onClick={() =>
          save.mutate(
            { id: `ft-${a.id}`, name: a.name, description: `${a.base_model} con l'adapter ${a.name}`, runtime_id: runtime, model: a.base_model, params: { adapters: [{ path: a.path }] } },
            { onSuccess: () => navigate("/models/profiles") },
          )}
      >
        Usa in un profilo
      </Button>
      {save.isError && <span className="field-error" role="alert">{errorMessage(save.error)}</span>}
    </>
  );
}

/** The adapters AIW trained; each one is a folder a profile can name. */
export function AdaptersTab({ adapters }: { adapters: UseQueryResult<AdapterInfo[]> }) {
  const [removing, setRemoving] = useState<AdapterInfo | null>(null);
  const del = useDeleteAdapter();
  const columns: Column<AdapterInfo>[] = [
    { key: "name", header: "Adapter", render: (a) => <><b>{a.name}</b><div className="faint text-xs mono">{a.id}</div></> },
    { key: "base", header: "Modello base", render: (a) => <span className="mono text-sm">{a.base_model}</span> },
    { key: "method", header: "Metodo", nowrap: true, render: (a) => `${METHOD_LABEL[a.method]} · ${a.trainer}` },
    { key: "metrics", header: "Metriche", render: (a) => <span className="text-sm">{metricsLine(a.metrics) || "—"}</span> },
    { key: "when", header: "Creato", nowrap: true, render: (a) => <span title={formatDateTime(a.created_at)}>{relTime(a.created_at)}</span> },
    {
      key: "act", header: "", render: (a) => (
        <div className="tr-actions">
          <UseButton a={a} />
          <CopyButton text={a.path} label="Copia il percorso" />
          <Button size="sm" variant="ghost" icon="trash" aria-label={`Elimina ${a.name}`} onClick={() => setRemoving(a)} />
        </div>
      ),
    },
  ];
  return (
    <Section title="Adapter" hint="Un adapter va caricato insieme al suo modello base, da un profilo di inferenza.">
      <QueryState
        query={adapters} isEmpty={(a) => a.length === 0}
        empty={<EmptyState icon="cube" title="Nessun adapter" description="Quando un addestramento finisce, l'adapter compare qui." action={<Link to="/training/jobs" className="btn primary">Vai agli addestramenti</Link>} />}
      >
        {(all) => <DataTable label="Adapter addestrati" columns={columns} rows={all} rowKey={(a) => a.id} />}
      </QueryState>
      {removing && (
        <DecisionDialog
          title="Eliminare l'adapter?" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error ?? undefined}
          onCancel={() => { setRemoving(null); del.reset(); }} onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>«{removing.name}» e i suoi file vengono cancellati. Se un profilo lo usa, tocca prima toglierlo dal profilo.</p>
        </DecisionDialog>
      )}
    </Section>
  );
}
