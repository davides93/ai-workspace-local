import { useMemo, useState } from "react";
import { navigate, setQuery, useQueryParams } from "../../../app/router";
import { Button, EmptyState, Field, Icon, Input, QueryState, Select } from "../../../shared/ui";
import { formatBytes } from "../../../shared/lib/format";
import type { LocalModel } from "../../../shared/api/types";
import { useModels } from "../api";
import { hiddenCount, runnableOnly } from "../model/adviser";
import { filterInstalled, installedFormats, originCounts, summarizeInstalled } from "../model/filters";
import { ORIGINS, ORIGIN_ORDER } from "../model/origin";
import { useQueryText } from "../useQueryText";
import { DeleteModelDialog } from "./DeleteModelDialog";
import { InstalledCard } from "./InstalledCard";
import { ShowAllToggle, useRunnableFilter } from "./ShowAll";
import { UseInProfileDialog } from "./UseInProfileDialog";

import { t } from "../../../shared/i18n";
/** Models on this device: AIW's own downloads plus the ones other apps already have, read in place. */
export function InstalledTab() {
  const models = useModels();
  return (
    <QueryState
      query={models}
      isEmpty={(l) => l.length === 0}
      empty={
        <EmptyState
          icon="cube" title={t("models.nessun_modello_installato")}
          description={t("models.scarica_un_modello_adatto_a")}
          action={<><Button variant="primary" icon="download" onClick={() => navigate("/models/catalog")}>{t("models.apri_il_catalogo")}</Button><Button icon="search" onClick={() => navigate("/models/discover")}>{t("models.scopri_modelli")}</Button></>}
        />
      }
    >
      {(list) => <InstalledList list={list} />}
    </QueryState>
  );
}

function InstalledList({ list: all }: { list: LocalModel[] }) {
  const query = useQueryParams();
  // Simple mode: only the models that can run on this device, until "Mostra tutti".
  const { simple, showAll } = useRunnableFilter();
  const list = simple ? runnableOnly(all, showAll) : all;
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
      <h2 className="sr-only">{t("models.modelli_installati")}</h2>
      {summary.externalCount > 0 && <ExternalNotice summary={summary} counts={counts} />}

      <div className="mh-toolbar" role="search">
        <Field label={t("models.cerca_tra_gli_installati")} htmlFor="inst-q" className="mh-search">
          <Input id="inst-q" type="search" placeholder={t("models.nome_formato_quantizzazione")} value={text} onChange={(e) => setText(e.target.value)} />
        </Field>
        <Field label={t("models.origine")} htmlFor="inst-origin">
          <Select id="inst-origin" value={origin} onChange={(e) => setQuery({ origin: e.target.value })}>
            <option value="">{t("models.tutte", { length: list.length })}</option>
            {ORIGIN_ORDER.filter((o) => counts[o]).map((o) => <option key={o} value={o}>{ORIGINS[o].label} ({counts[o]})</option>)}
          </Select>
        </Field>
        <Field label={t("models.formato")} htmlFor="inst-format">
          <Select id="inst-format" value={format} onChange={(e) => setQuery({ format: e.target.value })}>
            <option value="">{t("models.tutti")}</option>
            {formats.map((f) => <option key={f} value={f}>.{f}</option>)}
          </Select>
        </Field>
        {simple && hiddenCount(all) > 0 && <ShowAllToggle showAll={showAll} hidden={hiddenCount(all)} />}
        {filtered && <Button variant="ghost" size="sm" icon="close" onClick={clear}>{t("models.azzera_filtri")}</Button>}
      </div>
      <p className="mh-count" role="status" aria-live="polite">
        {t("models.nella_cartella_di_aiw", { p: shown.length === list.length ? `${list.length} modelli` : `${shown.length} di ${list.length} modelli`, p2: formatBytes(summary.ownBytes) })}</p>

      {shown.length === 0 ? (
        <EmptyState icon="search" title={t("models.nessun_modello_corrisponde")} description={t("models.cambia_la_ricerca_o_togli")} action={<Button onClick={clear}>{t("models.azzera_filtri")}</Button>} />
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
    <aside className="panel notice mh-notice" aria-label={t("models.modelli_di_altre_app")}>
      <Icon name="info" size={18} />
      <p className="text-sm">
        <b>{t("models.di_altre_app", { externalCount: summary.externalCount, p: summary.externalCount === 1 ? "modello" : "modelli" })}</b> ({formatBytes(summary.externalBytes)}): {parts.join(", ")}.
        {" "}AIW li legge dove si trovano, senza copiarli né modificarli, e li usa nei profili come gli altri. Per liberare spazio eliminali dall'app che li ha scaricati.
      </p>
    </aside>
  );
}
