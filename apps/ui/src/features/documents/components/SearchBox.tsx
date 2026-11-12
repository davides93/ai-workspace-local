import { useState } from "react";
import { Button, Field, Input } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import { useKbSearch } from "../api";

/** Tries the search the assistant uses: shows the passages found, with the document they come from. */
export function SearchBox({ collectionName }: { collectionName?: string }) {
  const search = useKbSearch();
  const [q, setQ] = useState("");
  const result = search.data;
  return (
    <section className="kb-search" aria-label="Cerca nei documenti">
      <h3>Prova a cercare</h3>
      <form className="kb-search-form" onSubmit={(e) => { e.preventDefault(); if (q.trim()) search.mutate({ query: q.trim(), collection: collectionName, max_results: 5 }); }}>
        <Field label="Cosa cerchi?" htmlFor="kb-q" className="grow">
          <Input id="kb-q" type="search" value={q} placeholder="Ad esempio: quanto costa l'affitto" onChange={(e) => setQ(e.target.value)} />
        </Field>
        <Button type="submit" icon="search" disabled={!q.trim() || search.isPending}>Cerca</Button>
      </form>
      {search.isError && <p className="kb-error text-sm" role="alert">{errorMessage(search.error)}</p>}
      {result && result.hits.length === 0 && <p className="muted text-sm">Non ho trovato nulla in questi documenti.</p>}
      {result && result.hits.length > 0 && (
        <ol className="kb-hits">
          {result.hits.map((h) => (
            <li key={h.chunk_id} className="kb-hit">
              <p className="kb-hit-title"><b>{h.document_name}</b> <span className="faint text-xs">parte {h.part} · {h.collection_name}</span></p>
              <p className="kb-hit-text text-sm">{h.text}</p>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
