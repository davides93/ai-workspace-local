import { useState } from "react";
import { Link } from "../../app/router";
import { useUiMode } from "../../app/useUiMode";
import { Icon, QueryState } from "../../shared/ui";
import { useKbCollections, useKbStatus, useKbSync } from "./api";
import { AddDocuments } from "./components/AddDocuments";
import { CollectionList } from "./components/CollectionList";
import { DocumentList } from "./components/DocumentList";
import { SearchBox } from "./components/SearchBox";
import { KEYWORDS_ONLY_BANNER, coverageText, pickCollection, runningJobs } from "./model/kb";
import "./documents.css";

import { t } from "../../shared/i18n";
/**
 * `#/documents` — "Documenti": the local knowledge base. Collections of files the assistant can search, with
 * what is indexed, a box to try the search and (developer mode) a folder added by path.
 */
export function DocumentsView() {
  const collections = useKbCollections();
  const status = useKbStatus();
  const [uiMode] = useUiMode();
  const [wanted, setWanted] = useState<string | null>(null);
  useKbSync(runningJobs(status.data).length);
  const selectedId = pickCollection(collections.data, wanted);
  const selected = collections.data?.find((c) => c.id === selectedId);
  const keywordsOnly = status.data !== undefined && !status.data.embedding_model;

  return (
    <div className="kb">
      <header className="kb-head">
        <Icon name="file" size={20} />
        <div>
          <h1>{t("documents.documenti")}</h1>
          <p className="muted text-sm">{t("documents.i_file_che_l_assistente")}</p>
        </div>
      </header>
      {keywordsOnly && (
        <div className="banner" role="status">
          <span>{KEYWORDS_ONLY_BANNER}</span>
          <Link to="/models" className="btn btn-sm banner-cta">{t("documents.vai_a_modelli")}</Link>
        </div>
      )}
      <QueryState query={collections}>
        {(list) => (
          <div className="kb-body">
            <aside className="kb-side" aria-label={t("documents.raccolte")}>
              <CollectionList collections={list} selected={selectedId} onSelect={setWanted} />
            </aside>
            <main className="kb-main">
              {selected ? (
                <>
                  <div className="kb-title">
                    <h2>{selected.name}</h2>
                    {selected.description && <p className="muted text-sm">{selected.description}</p>}
                    {coverageText(selected) && <p className="faint text-xs">{coverageText(selected)}</p>}
                  </div>
                  <AddDocuments collectionId={selected.id} jobs={status.data?.jobs ?? []} developer={uiMode === "developer"} />
                  <DocumentList collectionId={selected.id} />
                  <SearchBox collectionName={selected.name} />
                </>
              ) : (
                <p className="muted">{t("documents.crea_la_prima_raccolta_qui")}</p>
              )}
            </main>
          </div>
        )}
      </QueryState>
    </div>
  );
}
