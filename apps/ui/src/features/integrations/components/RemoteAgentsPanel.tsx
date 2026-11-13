import { useState } from "react";
import { Button, DecisionDialog, Dialog, EmptyState, Field, Icon, Input, Panel, Pill, QueryState } from "../../../shared/ui";
import { SECRET_MASK, type RemoteAgent } from "../../../shared/api/types";
import { errorMessage } from "../../../shared/lib/errors";
import { useDeleteRemoteAgent, useRemoteAgents, useSaveRemoteAgent } from "../api";
import { blankRemote, hasToken, normalizeRemote, remoteIdFor, validateRemote } from "../model/remote";

import { t, t as tr } from "../../../shared/i18n";
/**
 * The agents that run elsewhere (another device, an orchestrator, a cloud agent) and speak A2A. Teams and
 * agents delegate to them as `remote:<id>`. Paired devices are listed too, with their pairing token.
 */
export function RemoteAgentsPanel() {
  const query = useRemoteAgents();
  const [editing, setEditing] = useState<RemoteAgent | null>(null);
  const [deleting, setDeleting] = useState<RemoteAgent | null>(null);
  const del = useDeleteRemoteAgent();
  return (
    <Panel
      icon="link" title={t("integrations.agenti_remoti")}
      actions={<Button size="sm" icon="plus" onClick={() => setEditing(blankRemote())}>{t("integrations.aggiungi")}</Button>}
    >
      <div className="stack">
        <p className="muted text-sm">
          {t("integrations.agenti_a2a_che_lavorano_altrove")}</p>
        <QueryState
          query={query} isEmpty={(l) => l.length === 0}
          empty={<EmptyState icon="link" title={t("integrations.nessun_agente_remoto")} description={t("integrations.aggiungine_uno_oppure_accoppia_un")} />}
        >
          {(list) => (
            <ul className="list int-remote" aria-label={t("integrations.agenti_remoti")}>
              {list.map((r) => (
                <li key={r.id} className="list-item">
                  <Icon name="link" />
                  <div className="grow">
                    <b>{r.name}</b> <code className="faint text-xs">remote:{r.id}</code>
                    <div className="faint ellipsis text-xs">{r.url}{r.skill ? t("integrations.agente", { skill: r.skill }) : ""}{r.description ? ` · ${r.description}` : ""}</div>
                  </div>
                  {r.paired ? <Pill tone="neutral">{t("integrations.dispositivo_accoppiato")}</Pill> : (
                    <>
                      {hasToken(r) && <Pill tone="neutral">{t("integrations.token_impostato")}</Pill>}
                      <Button size="sm" onClick={() => setEditing(r)} aria-label={tr("integrations.modifica_2", { name: r.name })}>{t("integrations.modifica")}</Button>
                      <Button size="sm" variant="ghost" icon="trash" onClick={() => setDeleting(r)} aria-label={t("integrations.elimina", { name: r.name })} />
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}
        </QueryState>
      </div>
      {editing && <RemoteAgentDialog initial={editing} existing={query.data ?? []} onClose={() => setEditing(null)} />}
      {deleting && (
        <DecisionDialog
          title={t("integrations.elimina_l_agente_remoto")} confirmLabel={t("integrations.elimina_2")} icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setDeleting(null); del.reset(); }}
          onConfirm={() => del.mutate(deleting.id, { onSuccess: () => setDeleting(null) })}
        >
          <p>Vuoi eliminare <b>{deleting.name}</b>? L&apos;agente remoto non viene toccato: smetti solo di poterlo usare da qui.</p>
        </DecisionDialog>
      )}
    </Panel>
  );
}

function RemoteAgentDialog({ initial, existing, onClose }: { initial: RemoteAgent; existing: RemoteAgent[]; onClose: () => void }) {
  const isNew = initial.id === "";
  const save = useSaveRemoteAgent();
  const [draft, setDraft] = useState<RemoteAgent>(initial);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<RemoteAgent>) => setDraft((d) => ({ ...d, ...patch }));
  const issues = validateRemote(draft);
  const issue = (field: "name" | "url") => (tried ? issues.find((i) => i.field === field)?.message : undefined);
  const submit = () => {
    setTried(true);
    if (issues.length > 0) return;
    const r = normalizeRemote(isNew ? { ...draft, id: remoteIdFor(draft.name, existing) } : draft);
    save.mutate(r, { onSuccess: onClose });
  };
  return (
    <Dialog
      title={isNew ? t("integrations.nuovo_agente_remoto") : t("integrations.modifica_2", { name: initial.name })}
      onClose={onClose}
      footer={<>
        <Button onClick={onClose}>{t("integrations.annulla")}</Button>
        <Button variant="primary" onClick={submit} disabled={save.isPending}>{t("integrations.salva")}</Button>
      </>}
    >
      <form className="stack" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <Field label={t("integrations.nome")} htmlFor="ra-name" error={issue("name")}>
          <Input id="ra-name" value={draft.name} onChange={(e) => set({ name: e.target.value })} data-autofocus />
        </Field>
        <Field label={t("integrations.indirizzo")} htmlFor="ra-url" error={issue("url")} hint={t("integrations.l_indirizzo_a2a_es_http")}>
          <Input id="ra-url" mono value={draft.url} onChange={(e) => set({ url: e.target.value })} placeholder="http://host:7777" />
        </Field>
        <Field label={t("integrations.token")} htmlFor="ra-token" hint={draft.token === SECRET_MASK ? t("integrations.impostato_lascialo_cosi_per_tenerlo") : t("integrations.il_token_di_accesso_se")}>
          <Input
            id="ra-token" mono type="password" autoComplete="off" value={draft.token}
            onChange={(e) => set({ token: e.target.value })} onFocus={(e) => { if (draft.token === SECRET_MASK) e.currentTarget.select(); }}
          />
        </Field>
        <Field label={t("integrations.agente_da_chiamare")} htmlFor="ra-skill" hint={t("integrations.facoltativo_l_agente_da_usare")}>
          <Input id="ra-skill" value={draft.skill ?? ""} onChange={(e) => set({ skill: e.target.value })} placeholder={t("integrations.operator")} />
        </Field>
        <Field label={t("integrations.descrizione")} htmlFor="ra-desc" hint={t("integrations.aiuta_gli_agenti_a_capire")}>
          <Input id="ra-desc" value={draft.description} onChange={(e) => set({ description: e.target.value })} />
        </Field>
        {save.isError && <p className="field-error" role="alert">{errorMessage(save.error)}</p>}
      </form>
    </Dialog>
  );
}
