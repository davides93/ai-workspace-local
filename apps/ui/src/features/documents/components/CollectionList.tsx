import { useState } from "react";
import { Button, DecisionDialog, EmptyState, Field, Icon, Input } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { KbCollection } from "../../../shared/api/types";
import { useCreateCollection, useDeleteCollection } from "../api";
import { collectionCounts } from "../model/kb";

interface Props {
  collections: KbCollection[];
  selected: string | null;
  onSelect: (id: string) => void;
}

/** The collections on the left (on top on phones): pick one, make a new one, delete one. */
export function CollectionList({ collections, selected, onSelect }: Props) {
  const create = useCreateCollection();
  const del = useDeleteCollection();
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState<KbCollection | null>(null);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    create.mutate({ name }, { onSuccess: (c) => { setName(""); onSelect(c.id); } });
  };
  const current = collections.find((c) => c.id === selected);

  return (
    <div className="kb-cols">
      <h2>Raccolte</h2>
      {collections.length === 0 ? (
        <EmptyState icon="file" title="Nessuna raccolta" description="Crea una raccolta e aggiungi i tuoi documenti: l'assistente potrà cercarci dentro." />
      ) : (
        <ul className="kb-col-list" aria-label="Raccolte">
          {collections.map((c) => (
            <li key={c.id}>
              <button type="button" className={`kb-col${c.id === selected ? " is-on" : ""}`} aria-pressed={c.id === selected} onClick={() => onSelect(c.id)}>
                <Icon name="file" size={16} />
                <span className="kb-col-name">{c.name}</span>
                <span className="faint text-xs">{collectionCounts(c)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <form className="kb-new" onSubmit={submit}>
        <Field label="Nuova raccolta" htmlFor="kb-new-name" error={create.isError ? errorMessage(create.error) : undefined}>
          <Input id="kb-new-name" value={name} maxLength={80} placeholder="Ad esempio: Casa" onChange={(e) => setName(e.target.value)} />
        </Field>
        <Button type="submit" variant="primary" icon="plus" disabled={!name.trim() || create.isPending}>Crea</Button>
      </form>
      {current && (
        <Button variant="ghost" icon="trash" onClick={() => setConfirm(current)}>Elimina «{current.name}»</Button>
      )}
      {confirm && (
        <DecisionDialog
          title="Eliminare la raccolta?"
          confirmLabel="Elimina"
          pending={del.isPending}
          error={del.error}
          onCancel={() => setConfirm(null)}
          onConfirm={() => del.mutate(confirm.id, { onSuccess: () => setConfirm(null) })}
        >
          <p>«{confirm.name}» e i suoi {collectionCounts(confirm)} escono dall'archivio dell'assistente. I file originali sul tuo computer non vengono toccati.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
