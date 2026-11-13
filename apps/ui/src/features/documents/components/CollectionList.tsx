import { useState } from "react";
import { Button, DecisionDialog, EmptyState, Field, Icon, Input } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { KbCollection } from "../../../shared/api/types";
import { useCreateCollection, useDeleteCollection } from "../api";
import { collectionCounts } from "../model/kb";

import { t } from "../../../shared/i18n";
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
      <h2>{t("documents.raccolte")}</h2>
      {collections.length === 0 ? (
        <EmptyState icon="file" title={t("documents.nessuna_raccolta")} description={t("documents.crea_una_raccolta_e_aggiungi")} />
      ) : (
        <ul className="kb-col-list" aria-label={t("documents.raccolte")}>
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
        <Field label={t("documents.nuova_raccolta")} htmlFor="kb-new-name" error={create.isError ? errorMessage(create.error) : undefined}>
          <Input id="kb-new-name" value={name} maxLength={80} placeholder={t("documents.ad_esempio_casa")} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Button type="submit" variant="primary" icon="plus" disabled={!name.trim() || create.isPending}>{t("documents.crea")}</Button>
      </form>
      {current && (
        <Button variant="ghost" icon="trash" onClick={() => setConfirm(current)}>{t("documents.elimina", { name: current.name })}</Button>
      )}
      {confirm && (
        <DecisionDialog
          title={t("documents.eliminare_la_raccolta")}
          confirmLabel={t("documents.elimina_2")}
          pending={del.isPending}
          error={del.error}
          onCancel={() => setConfirm(null)}
          onConfirm={() => del.mutate(confirm.id, { onSuccess: () => setConfirm(null) })}
        >
          <p>{t("documents.e_i_suoi_escono_dall", { name: confirm.name, p: collectionCounts(confirm) })}</p>
        </DecisionDialog>
      )}
    </div>
  );
}
