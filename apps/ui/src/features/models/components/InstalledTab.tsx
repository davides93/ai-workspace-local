import { useMemo, useState } from "react";
import { navigate, setQuery, useQueryParams } from "../../../app/router";
import { Button, EmptyState, Field, Icon, Input, QueryState, Select } from "../../../shared/ui";
import { formatBytes } from "../../../shared/lib/format";
import type { LocalModel } from "../../../shared/api/types";
import { useModels } from "../api";
import { filterInstalled, installedFormats, originCounts, summarizeInstalled } from "../model/filters";
import { ORIGINS, ORIGIN_ORDER } from "../model/origin";
import { useQueryText } from "../useQueryText";
import { DeleteModelDialog } from "./DeleteModelDialog";
import { InstalledCard } from "./InstalledCard";
import { UseInProfileDialog } from "./UseInProfileDialog";

/** Models on this device: AIW's own downloads plus the ones other apps already have, read in place. */
export function InstalledTab() {
  const models = useModels();
  return (
    <QueryState
      query={models}
      isEmpty={(l) => l.length === 0}
      empty={
        <EmptyState
          icon="cube" title="Nessun modello installato"
          description="Scarica un modello adatto a questo device dal catalogo consigliato o cercalo su Hugging Face, ModelScope e Ollama. I modelli già presenti in Ollama, LM Studio e nella cache di Hugging Face compaiono qui da soli."
          action={<><Button variant="primary" icon="download" onClick={() => navigate("/models/catalog")}>Apri il catalogo</Button><Button icon="search" onClick={() => navigate("/models/discover")}>Scopri modelli</Button></>}
        />
      }
    >
      {(list) => <InstalledList list={list} />}
    </QueryState>
  );
}

function InstalledList({ list }: { list: LocalModel[] }) {
  const query = useQueryParams();
  const [text, setText] = useQueryText("q");
  const origin = query.origin ?? "";
  const format = query.format ?? "";
  const [use, setUse] = useState<LocalModel | null>(null);
  const [del, setDel] = useState<LocalModel | null>(null);

  const counts = useMemo(() => originCounts(list), [list]);
  const formats = useMemo(() => installedFormats(list), [list]);
  const summary = useMemo(() => summarizeInstalled(list), [list]);
  const shown = useMemo(() => filterInstalled(list, { q: text, origin, format }), [list, text, origin, format]);
  const filtered = !!(text || origin || format);

  const clear = () => {
    setText("");
    setQuery({ q: "", origin: "", format: "" });
  };

  return (
    <div className="stack stack-lg">
      <h2 className="sr-only">Modelli installati</h2>
      {summary.externalCount > 0 && <ExternalNotice summary={summary} counts={counts} />}

      <div className="mh-toolbar" role="search">
        <Field label="Cerca tra gli installati" htmlFor="inst-q" className="mh-search">
          <Input id="inst-q" type="search" placeholder="Nome, formato, quantizzazione…" value={text} onChange={(e) => setText(e.target.value)} />
        </Field>
        <Field label="Origine" htmlFor="inst-origin">
          <Select id="inst-origin" value={origin} onChange={(e) => setQuery({ origin: e.target.value })}>
            <option value="">Tutte ({list.length})</option>
            {ORIGIN_ORDER.filter((o) => counts[o]).map((o) => <option key={o} value={o}>{ORIGINS[o].label} ({counts[o]})</option>)}
          </Select>
        </Field>
        <Field label="Formato" htmlFor="inst-format">
          <Select id="inst-format" value={format} onChange={(e) => setQuery({ format: e.target.value })}>
            <option value="">Tutti</option>
            {formats.map((f) => <option key={f} value={f}>.{f}</option>)}
          </Select>
        </Field>
        {filtered && <Button variant="ghost" size="sm" icon="close" onClick={clear}>Azzera filtri</Button>}
      </div>
      <p className="mh-count" role="status" aria-live="polite">
        {shown.length === list.length ? `${list.length} modelli` : `${shown.length} di ${list.length} modelli`} · {formatBytes(summary.ownBytes)} nella cartella di AIW
      </p>

      {shown.length === 0 ? (
        <EmptyState icon="search" title="Nessun modello corrisponde" description="Cambia la ricerca o togli un filtro." action={<Button onClick={clear}>Azzera filtri</Button>} />
      ) : (
        <div className="mh-grid">
          {shown.map((m) => <InstalledCard key={m.id} model={m} onUse={() => setUse(m)} onDelete={() => setDel(m)} />)}
        </div>
      )}
      {use && <UseInProfileDialog model={use} onClose={() => setUse(null)} />}
      {del && <DeleteModelDialog model={del} onClose={() => setDel(null)} />}
    </div>
  );
}

/** Explains the read-only sources: AIW uses those files where they are, never copies or deletes them. */
function ExternalNotice({ summary, counts }: { summary: ReturnType<typeof summarizeInstalled>; counts: Record<string, number> }) {
  const parts = (["ollama", "lmstudio", "hf-cache"] as const).filter((o) => counts[o]).map((o) => `${ORIGINS[o].label} (${counts[o]})`);
  return (
    <aside className="panel notice mh-notice" aria-label="Modelli di altre app">
      <Icon name="info" size={18} />
      <p className="text-sm">
        <b>{summary.externalCount} {summary.externalCount === 1 ? "modello" : "modelli"} di altre app</b> ({formatBytes(summary.externalBytes)}): {parts.join(", ")}.
        {" "}AIW li legge dove si trovano, senza copiarli né modificarli, e li usa nei profili come gli altri. Per liberare spazio eliminali dall'app che li ha scaricati.
      </p>
    </aside>
  );
}
