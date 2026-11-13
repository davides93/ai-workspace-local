import { useState } from "react";
import { Link } from "../../../app/router";
import { Button, DataTable, EmptyState, Icon, IconButton, Pill, QueryState, StatusPill, type Column, DecisionDialog } from "../../../shared/ui";
import { formatContext } from "../../../shared/lib/format";
import type { InferenceProfile } from "../../../shared/api/types";
import { useAgents, useDeleteProfile, useInstalledModels, useProfiles, useRuntimeList, useSaveProfile } from "../api";
import { agentsUsingProfile } from "../model/agents";
import { ProfileDialog } from "./ProfileDialog";

import { t, tNode, t as tr } from "../../../shared/i18n";
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
    { key: "name", header: tr("agents.profilo"), render: (p) => <><b>{p.name}</b><div className="faint text-xs">{p.description}</div><div className="mono faint text-xs">{p.id}</div></> },
    {
      key: "runtime", header: tr("agents.runtime_2"),
      render: (p) => <div className="stack stack-sm"><span className="mono">{rt(p.runtime_id)?.name ?? p.runtime_id}</span>{rt(p.runtime_id) && <StatusPill status={rt(p.runtime_id)!.status} />}</div>,
    },
    { key: "model", header: tr("agents.modello"), className: "mono", render: (p) => p.model || <Pill tone="warn">{t("agents.da_scegliere")}</Pill> },
    {
      key: "params", header: tr("agents.parametri"), className: "num",
      render: (p) => <div className="stack stack-sm"><span>{t("agents.t", { p: p.params.temperature ?? "—" })}</span><span>{t("agents.ctx", { p: formatContext(p.params.context) })}</span><span>{p.params.max_tokens ? t("agents.max", { max_tokens: p.params.max_tokens }) : "—"}</span></div>,
    },
    {
      key: "used", header: tr("agents.usato_da"),
      render: (p) => {
        const used = agentsUsingProfile(agents.data ?? [], p.id);
        return used.length ? <div className="chips">{used.map((a) => <Link key={a.id} to={`/agents/${encodeURIComponent(a.id)}`} className="chip">{a.name}</Link>)}</div> : <span className="faint text-xs">{t("agents.nessun_agente_2")}</span>;
      },
    },
    {
      key: "actions", header: <span className="sr-only">{t("agents.azioni")}</span>, className: "nowrap",
      render: (p) => {
        const used = agentsUsingProfile(agents.data ?? [], p.id);
        return (
          <div className="row nowrap">
            <IconButton aria-label={t("agents.modifica_il_profilo", { name: p.name })} icon="edit" size={16} onClick={() => setEditing(p)} />
            <IconButton aria-label={used.length ? t("agents.delete_profile_used", { name: p.name, agents: used.map((a) => a.name).join(", ") }) : t("agents.elimina_il_profilo", { name: p.name })} icon="trash" size={16} disabled={used.length > 0}
              title={used.length ? t("agents.profile_used_by", { agents: used.map((a) => a.name).join(", ") }) : t("agents.elimina_2", { name: p.name })} onClick={() => setRemoving(p)} />
          </div>
        );
      },
    },
  ];

  return (
    <div className="stack stack-lg">
      <div className="row row-between">
        <p className="muted text-sm">{t("agents.runtime_modello_e_parametri_riutilizzabili")}</p>
        <Button variant="primary" icon="plus" onClick={() => setEditing(null)}>{t("agents.nuovo_profilo")}</Button>
      </div>
      <QueryState
        query={profiles} isEmpty={(p) => p.length === 0}
        empty={<EmptyState icon="cube" title={t("agents.nessun_profilo")} description={t("agents.un_profilo_dice_quale_modello")} action={<Button variant="primary" icon="plus" onClick={() => setEditing(null)}>{t("agents.nuovo_profilo")}</Button>} />}
      >
        {(list) => <DataTable label={t("agents.profili_di_inferenza")} columns={columns} rows={list} rowKey={(p) => p.id} className="profiles-table" />}
      </QueryState>
      <p className="faint text-sm row"><Icon name="info" size={14} /> {tNode("agents.scegli_un_modello_disponibile_in", { link: <Link to="/models">{t("agents.modelli")}</Link> })}</p>

      {editing !== undefined && runtimes.data && models.data && (
        <ProfileDialog
          profile={editing} profiles={profiles.data ?? []} runtimes={runtimes.data} models={models.data} saving={save.isPending}
          onClose={() => setEditing(undefined)}
          onSave={(p) => save.mutate(p, { onSuccess: () => setEditing(undefined) })}
        />
      )}
      {removing && (
        <DecisionDialog
          title={t("agents.elimina_il_profilo_2")} confirmLabel={t("agents.elimina")} icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setRemoving(null); del.reset(); }}
          onConfirm={() => del.mutate(removing.id, { onSuccess: () => setRemoving(null) })}
        >
          <p>{tNode("agents.confirm_delete_profile", { name: <b>{removing.name}</b> })}</p>
          <p className="faint text-sm">{t("agents.se_un_workflow_lo_usa_2")}</p>
        </DecisionDialog>
      )}
    </div>
  );
}
