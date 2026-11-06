import { useState } from "react";
import { Button, DecisionDialog, Dialog, EmptyState, Field, Icon, Input, Panel, Pill, QueryState } from "../../../shared/ui";
import { SECRET_MASK, type RemoteAgent } from "../../../shared/api/types";
import { errorMessage } from "../../../shared/lib/errors";
import { useDeleteRemoteAgent, useRemoteAgents, useSaveRemoteAgent } from "../api";
import { blankRemote, hasToken, normalizeRemote, remoteIdFor, validateRemote } from "../model/remote";

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
      icon="link" title="Agenti remoti"
      actions={<Button size="sm" icon="plus" onClick={() => setEditing(blankRemote())}>Aggiungi</Button>}
    >
      <div className="stack">
        <p className="muted text-sm">
          Agenti A2A che lavorano altrove: li scegli come membri di un team o li lasci usare agli altri agenti come subagenti. Si parla solo con gli indirizzi
          registrati qui, e ogni delega finisce nel registro delle attività.
        </p>
        <QueryState
          query={query} isEmpty={(l) => l.length === 0}
          empty={<EmptyState icon="link" title="Nessun agente remoto" description="Aggiungine uno, oppure accoppia un altro dispositivo dalla pagina Dispositivi." />}
        >
          {(list) => (
            <ul className="list int-remote" aria-label="Agenti remoti">
              {list.map((r) => (
                <li key={r.id} className="list-item">
                  <Icon name="link" />
                  <div className="grow">
                    <b>{r.name}</b> <code className="faint text-xs">remote:{r.id}</code>
                    <div className="faint ellipsis text-xs">{r.url}{r.skill ? ` · agente ${r.skill}` : ""}{r.description ? ` · ${r.description}` : ""}</div>
                  </div>
                  {r.paired ? <Pill tone="neutral">dispositivo accoppiato</Pill> : (
                    <>
                      {hasToken(r) && <Pill tone="neutral">token impostato</Pill>}
                      <Button size="sm" onClick={() => setEditing(r)} aria-label={`Modifica ${r.name}`}>Modifica</Button>
                      <Button size="sm" variant="ghost" icon="trash" onClick={() => setDeleting(r)} aria-label={`Elimina ${r.name}`} />
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
          title="Elimina l'agente remoto" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error}
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
      title={isNew ? "Nuovo agente remoto" : `Modifica ${initial.name}`}
      onClose={onClose}
      footer={<>
        <Button onClick={onClose}>Annulla</Button>
        <Button variant="primary" onClick={submit} disabled={save.isPending}>Salva</Button>
      </>}
    >
      <form className="stack" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <Field label="Nome" htmlFor="ra-name" error={issue("name")}>
          <Input id="ra-name" value={draft.name} onChange={(e) => set({ name: e.target.value })} data-autofocus />
        </Field>
        <Field label="Indirizzo" htmlFor="ra-url" error={issue("url")} hint="L'indirizzo A2A (es. http://192.168.1.20:7777) o quello della sua Agent Card.">
          <Input id="ra-url" mono value={draft.url} onChange={(e) => set({ url: e.target.value })} placeholder="http://host:7777" />
        </Field>
        <Field label="Token" htmlFor="ra-token" hint={draft.token === SECRET_MASK ? "Impostato: lascialo così per tenerlo, scrivine un altro per cambiarlo." : "Il token di accesso, se l'agente lo richiede."}>
          <Input
            id="ra-token" mono type="password" autoComplete="off" value={draft.token}
            onChange={(e) => set({ token: e.target.value })} onFocus={(e) => { if (draft.token === SECRET_MASK) e.currentTarget.select(); }}
          />
        </Field>
        <Field label="Agente da chiamare" htmlFor="ra-skill" hint="Facoltativo: l'agente da usare su quel dispositivo (vuoto: quello predefinito).">
          <Input id="ra-skill" value={draft.skill ?? ""} onChange={(e) => set({ skill: e.target.value })} placeholder="operator" />
        </Field>
        <Field label="Descrizione" htmlFor="ra-desc" hint="Aiuta gli agenti a capire quando delegargli del lavoro.">
          <Input id="ra-desc" value={draft.description} onChange={(e) => set({ description: e.target.value })} />
        </Field>
        {save.isError && <p className="field-error" role="alert">{errorMessage(save.error)}</p>}
      </form>
    </Dialog>
  );
}
