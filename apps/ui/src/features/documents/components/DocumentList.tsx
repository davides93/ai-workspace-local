import { useState } from "react";
import { Button, DecisionDialog, EmptyState, Pill, QueryState } from "../../../shared/ui";
import { humanSize } from "../../../shared/lib/attachments";
import type { KbDocument } from "../../../shared/api/types";
import { useDeleteDocument, useKbDocuments } from "../api";
import { documentStatusLabel } from "../model/kb";

/** The documents of a collection with their state; a failed one says why. */
export function DocumentList({ collectionId }: { collectionId: string }) {
  const docs = useKbDocuments(collectionId);
  const del = useDeleteDocument();
  const [confirm, setConfirm] = useState<KbDocument | null>(null);
  return (
    <section className="kb-docs" aria-label="Documenti della raccolta">
      <h3>Documenti</h3>
      <QueryState query={docs}>
        {(list) =>
          list.length === 0 ? (
            <EmptyState icon="file" title="Ancora nessun documento" description="Aggiungi file di testo, Markdown, codice o PDF: li divido in passaggi e l'assistente li cerca quando serve." />
          ) : (
            <ul className="kb-doc-list">
              {list.map((d) => (
                <li key={d.id} className="kb-doc">
                  <div className="kb-doc-main">
                    <b className="kb-doc-name">{d.name}</b>
                    <span className="faint text-xs">{humanSize(d.size)} · {d.chunks} passaggi{d.source !== "caricato" ? ` · ${d.source}` : ""}</span>
                    {d.status === "failed" && d.error && <span className="kb-doc-error text-sm">{d.error}</span>}
                  </div>
                  <Pill tone={d.status === "ready" ? "ok" : d.status === "failed" ? "crit" : "info"}>{documentStatusLabel(d)}</Pill>
                  <Button size="sm" variant="ghost" icon="trash" aria-label={`Elimina ${d.name}`} onClick={() => setConfirm(d)} />
                </li>
              ))}
            </ul>
          )
        }
      </QueryState>
      {confirm && (
        <DecisionDialog
          title="Togliere il documento?"
          confirmLabel="Togli"
          pending={del.isPending}
          error={del.error}
          onCancel={() => setConfirm(null)}
          onConfirm={() => del.mutate(confirm.id, { onSuccess: () => setConfirm(null) })}
        >
          <p>«{confirm.name}» non sarà più cercabile. Il file originale resta dov'è.</p>
        </DecisionDialog>
      )}
    </section>
  );
}
