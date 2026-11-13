import { useState } from "react";
import { Button, DecisionDialog, EmptyState, Pill, QueryState } from "../../../shared/ui";
import { humanSize } from "../../../shared/lib/attachments";
import type { KbDocument } from "../../../shared/api/types";
import { useDeleteDocument, useKbDocuments } from "../api";
import { documentStatusLabel } from "../model/kb";

import { t } from "../../../shared/i18n";
/** The documents of a collection with their state; a failed one says why. */
export function DocumentList({ collectionId }: { collectionId: string }) {
  const docs = useKbDocuments(collectionId);
  const del = useDeleteDocument();
  const [confirm, setConfirm] = useState<KbDocument | null>(null);
  return (
    <section className="kb-docs" aria-label={t("documents.documenti_della_raccolta")}>
      <h3>{t("documents.documenti")}</h3>
      <QueryState query={docs}>
        {(list) =>
          list.length === 0 ? (
            <EmptyState icon="file" title={t("documents.ancora_nessun_documento")} description={t("documents.aggiungi_file_di_testo_markdown")} />
          ) : (
            <ul className="kb-doc-list">
              {list.map((d) => (
                <li key={d.id} className="kb-doc">
                  <div className="kb-doc-main">
                    <b className="kb-doc-name">{d.name}</b>
                    <span className="faint text-xs">{t("documents.passaggi", { p: humanSize(d.size), chunks: d.chunks, p2: d.source !== "caricato" ? ` · ${d.source}` : "" })}</span>
                    {d.status === "failed" && d.error && <span className="kb-doc-error text-sm">{d.error}</span>}
                  </div>
                  <Pill tone={d.status === "ready" ? "ok" : d.status === "failed" ? "crit" : "info"}>{documentStatusLabel(d)}</Pill>
                  <Button size="sm" variant="ghost" icon="trash" aria-label={t("documents.elimina_3", { name: d.name })} onClick={() => setConfirm(d)} />
                </li>
              ))}
            </ul>
          )
        }
      </QueryState>
      {confirm && (
        <DecisionDialog
          title={t("documents.togliere_il_documento")}
          confirmLabel={t("documents.togli")}
          pending={del.isPending}
          error={del.error}
          onCancel={() => setConfirm(null)}
          onConfirm={() => del.mutate(confirm.id, { onSuccess: () => setConfirm(null) })}
        >
          <p>{t("documents.non_sara_piu_cercabile_il", { name: confirm.name })}</p>
        </DecisionDialog>
      )}
    </section>
  );
}
