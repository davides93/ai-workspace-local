import { errorMessage } from "../../shared/lib/errors";
import { useMemo, useState } from "react";
import { navigate, useParams } from "../../app/router";
import { useSystem } from "../../shared/query/system";
import { Button, EmptyState, Field, Icon, Input, Page, PageHeader, QueryState, Section, Segmented, Sheet } from "../../shared/ui";
import { useModels } from "../models/api";
import { useRefreshRuntimes, useRuntimeAction, useRuntimes } from "./api";
import { RuntimeCard } from "./components/RuntimeCard";
import { RuntimeDrawer } from "./components/RuntimeDrawer";
import { filterRuntimes, groupByKind, KIND_META, runtimeCounts, type Scope } from "./model/runtimes";
import "./runtimes.css";

import { t, tNode } from "../../shared/i18n";
/** `#/runtimes/:runtimeId?` — the runtime catalog by kind; the drawer opens when the URL names a runtime. */
export function RuntimesView() {
  const rts = useRuntimes({ poll: true });
  const models = useModels();
  const system = useSystem();
  const refresh = useRefreshRuntimes();
  const action = useRuntimeAction(true);
  const [scope, setScope] = useState<Scope>("here");
  const [query, setQuery] = useState("");
  const runtimeId = useParams().runtimeId;

  const all = rts.data ?? [];
  const opened = runtimeId ? all.find((r) => r.id === runtimeId) : undefined;
  const deviceAccels = useMemo(() => new Set<string>(system.data?.device.accelerators.map((a) => a.kind) ?? []), [system.data]);
  const counts = runtimeCounts(all);
  const busyId = action.isPending ? action.variables?.runtime.id : undefined;
  const closeDrawer = () => navigate("/runtimes");

  return (
    <Page>
      <PageHeader
        eyebrow={t("nav.group_inference")} title={t("runtimes.runtime")}
        subtitle={rts.data ? t("runtimes.attivi_utilizzabili_qui_nel_catalogo", { running: counts.running, usable: counts.usable, total: counts.total }) : t("runtimes.motori_che_eseguono_i_modelli")}
        actions={(
          <Button icon="refresh" onClick={() => refresh.mutate()} disabled={refresh.isPending} aria-busy={refresh.isPending}>
            {refresh.isPending ? t("runtimes.rilevo") : t("runtimes.rileva")}
          </Button>
        )}
      />
      <div className="row filters rtv-toolbar">
        <Field label={t("runtimes.cerca")} htmlFor="rt-search" className="grow rtv-search">
          <Input id="rt-search" type="search" placeholder={t("runtimes.nome_produttore_acceleratore_o_formato")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </Field>
        <div className="field">
          <span className="field-label" id="rt-scope-label">{t("runtimes.mostra")}</span>
          <Segmented
            label={t("runtimes.quali_runtime_mostrare")} value={scope} onChange={setScope}
            options={[{ id: "here", label: t("runtimes.su_questo_device") }, { id: "all", label: t("runtimes.tutto_il_catalogo") }]}
          />
        </div>
      </div>

      <QueryState query={rts} isEmpty={(l) => l.length === 0} empty={<EmptyState icon="chip" title={t("runtimes.nessun_runtime_nel_catalogo")} description={t("runtimes.premi_rileva_per_cercare_i")} action={<Button icon="refresh" onClick={() => refresh.mutate()}>{t("runtimes.rileva")}</Button>} />}>
        {(list) => {
          const groups = groupByKind(filterRuntimes(list, { scope, query }));
          if (groups.length === 0) {
            return (
              <EmptyState
                icon="search" title={t("runtimes.nessun_runtime_corrisponde")}
                description={scope === "here" ? t("runtimes.i_runtime_non_supportati_da") : t("runtimes.prova_con_un_altro_termine")}
                action={<>
                  {scope === "here" && <Button onClick={() => setScope("all")}>{t("runtimes.mostra_tutto_il_catalogo")}</Button>}
                  {query && <Button variant="ghost" onClick={() => setQuery("")}>{t("runtimes.cancella_la_ricerca")}</Button>}
                </>}
              />
            );
          }
          return groups.map(({ kind, items }) => (
            <Section key={kind} title={KIND_META[kind].title} hint={KIND_META[kind].desc}>
              <div className="grid">
                {items.map((r) => (
                  <RuntimeCard
                    key={r.id} runtime={r} deviceAccels={deviceAccels} busy={busyId === r.id}
                    error={action.isError && action.variables.runtime.id === r.id ? errorMessage(action.error) : undefined}
                    onAct={(a) => action.mutate({ runtime: r, action: a })}
                    onOpen={() => navigate(`/runtimes/${encodeURIComponent(r.id)}`)}
                  />
                ))}
              </div>
            </Section>
          ));
        }}
      </QueryState>

      <p className="faint text-sm rtv-note">
        <Icon name="info" size={14} /> {tNode("runtimes.ogni_runtime_e_un_manifest", { code: <code>~/.aiw/runtimes/</code> })}</p>

      {runtimeId && opened && (
        <RuntimeDrawer key={opened.id} runtime={opened} models={models.data ?? []} deviceAccels={deviceAccels} onClose={closeDrawer} />
      )}
      {runtimeId && !opened && rts.data && (
        <Sheet title={t("runtimes.runtime_non_trovato")} onClose={closeDrawer} footer={<Button onClick={closeDrawer}>{t("runtimes.torna_al_catalogo")}</Button>}>
          <p>{tNode("runtimes.no_runtime_with_id", { id: <code>{runtimeId}</code> })}</p>
        </Sheet>
      )}
    </Page>
  );
}
