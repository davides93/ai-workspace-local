import { useState } from "react";
import { Link } from "../../../app/router";
import { Button, DataTable, EmptyState, Icon, IconButton, Pill, QueryState, StatusPill, type Column, DecisionDialog } from "../../../shared/ui";
import { formatContext } from "../../../shared/lib/format";
import type { InferenceProfile } from "../../../shared/api/types";
import { useAgents, useDeleteProfile, useInstalledModels, useProfiles, useRuntimeList, useSaveProfile } from "../api";
import { agentsUsingProfile } from "../model/agents";
import { ProfileDialog } from "./ProfileDialog";

/** Inference profiles: runtime, model and parameters that agents and workflows pick by name. */
export function ProfilesPanel() {
  const profiles = useProfiles();
  const agents = useAgents();
  const runtimes = useRuntimeList();
  const models = useInstalledModels();
  const save = useSaveProfile();
  const del = useDeleteProfile();
  // `undefined`: closed, `null`: new profile, otherwise the profile being edited.
  const [editing, setEditing] = useState<InferenceProfile | null | undefined>(undefined);
  const [removing, setRemoving] = useState<InferenceProfile | null>(null);
  const rt = (id: string) => runtimes.data?.find((r) => r.id === id);

  const columns: Column<InferenceProfile>[] = [
    { key: "name", header: "Profilo", render: (p) => <><b>{p.name}</b><div className="faint text-xs">{p.description}</div><div className="mono faint text-xs">{p.id}</div></> },
    {
      key: "runtime", header: "Runtime",
      render: (p) => <div className="stack stack-sm"><span className="mono">{rt(p.runtime_id)?.name ?? p.runtime_id}</span>{rt(p.runtime_id) && <StatusPill status={rt(p.runtime_id)!.status} />}</div>,
    },
    { key: "model", header: "Modello", className: "mono", render: (p) => p.model || <Pill tone="warn">da scegliere</Pill> },
    {
      key: "params", header: "Parametri", className: "num",
      render: (p) => <div className="stack stack-sm"><span>T {p.params.temperature ?? "—"}</span><span>{formatContext(p.params.context)} ctx</span><span>{p.params.max_tokens ? `${p.params.max_tokens} max` : "—"}</span></div>,
    },
    {
      key: "used", header: "Usato da",
      render: (p) => {
        const used = agentsUsingProfile(agents.data ?? [], p.id);
        return used.length ? <div className="chips">{used.map((a) => <Link key={a.id} to={`/agents/${encodeURIComponent(a.id)}`} className="chip">{a.name}</Link>)}</div> : <span className="faint text-xs">nessun agente</span>;
      },
    },
    {
      key: "actions", header: <span className="sr-only">Azioni</span>, className: "nowrap",
      render: (p) => {
        const used = agentsUsingProfile(agents.data ?? [], p.id);
        return (
          <div className="row nowrap">
            <IconButton aria-label={`Modifica il profilo ${p.name}`} icon="edit" size={16} onClick={() => setEditing(p)} />
            <IconButton aria-label={used.length ? `Elimina il profilo ${p.name} (usato da ${used.map((a) => a.name).join(", ")})` : `Elimina il profilo ${p.name}`} icon="trash" size={16} disabled={used.length > 0}
              title={used.length ? `Usato da ${used.map((a) => a.name).join(", ")}: assegna loro un altro profilo` : `Elimina ${p.name}`} onClick={() => setRemoving(p)} />
          </div>
        );
      },
    },
  ];

  return (
    <div className="stack stack-lg">
      <div className="row row-between">
        <p className="muted text-sm">Runtime, modello e parametri riutilizzabili da agenti e workflow.</p>
        <Button variant="primary" icon="plus" onClick={() => setEditing(null)}>Nuovo profilo</Button>
      </div>
      <QueryState
        query={profiles} isEmpty={(p) => p.length === 0}
        empty={<EmptyState icon="cube" title="Nessun profilo" description="Un profilo dice quale modello usare e con quali parametri." action={<Button variant="primary" icon="plus" onClick={() => setEditing(null)}>Nuovo profilo</Button>} />}
      >
        {(list) => <DataTable label="Profili di inferenza" columns={columns} rows={list} rowKey={(p) => p.id} className="profiles-table" />}
      </QueryState>
      <p className="faint text-sm row"><Icon name="info" size={14} /> Il modello va scaricato prima: lo trovi in <Link to="/models">Modelli</Link>. Un profilo usato da un agente non si può eliminare.</p>

      {editing !== undefined && runtimes.data && models.data && (
        <ProfileDialog
          profile={editing} profiles={profiles.data ?? []} runtimes={runtimes.data} models={models.data} saving={save.isPending}
          onClose={() => setEditing(undefined)}
          onSave={(p) => save.mutate(p, { onSuccess: () => setEditing(undefined) })}
        />
      )}
      {removing && (
        <DecisionDialog
          title="Elimina il profilo" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setRemoving(null); del.reset(); }}
          onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>Vuoi eliminare il profilo <b>{removing.name}</b>?</p>
          <p className="faint text-sm">Se un workflow lo usa ancora in un passo LLM, l'eliminazione viene rifiutata e vedrai quali.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
