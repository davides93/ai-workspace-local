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

/** `#/runtimes/:runtimeId?` — the runtime catalog by kind; the drawer opens when the URL names a runtime. */
export function RuntimesView() {
  const rts = useRuntimes({ poll: true });
  const models = useModels();
  const system = useSystem();
  const refresh = useRefreshRuntimes();
  const action = useRuntimeAction();
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
        eyebrow="Inferenza" title="Runtime"
        subtitle={rts.data ? `${counts.running} attivi · ${counts.usable} utilizzabili qui · ${counts.total} nel catalogo` : "Motori che eseguono i modelli"}
        actions={(
          <Button icon="refresh" onClick={() => refresh.mutate()} disabled={refresh.isPending} aria-busy={refresh.isPending}>
            {refresh.isPending ? "Rilevo…" : "Rileva"}
          </Button>
        )}
      />
      <div className="row filters rtv-toolbar">
        <Field label="Cerca" htmlFor="rt-search" className="grow rtv-search">
          <Input id="rt-search" type="search" placeholder="Nome, produttore, acceleratore o formato" value={query} onChange={(e) => setQuery(e.target.value)} />
        </Field>
        <div className="field">
          <span className="field-label" id="rt-scope-label">Mostra</span>
          <Segmented
            label="Quali runtime mostrare" value={scope} onChange={setScope}
            options={[{ id: "here", label: "Su questo device" }, { id: "all", label: "Tutto il catalogo" }]}
          />
        </div>
      </div>

      <QueryState query={rts} isEmpty={(l) => l.length === 0} empty={<EmptyState icon="chip" title="Nessun runtime nel catalogo" description="Premi Rileva per cercare i runtime installati su questo computer." action={<Button icon="refresh" onClick={() => refresh.mutate()}>Rileva</Button>} />}>
        {(list) => {
          const groups = groupByKind(filterRuntimes(list, { scope, query }));
          if (groups.length === 0) {
            return (
              <EmptyState
                icon="search" title="Nessun runtime corrisponde"
                description={scope === "here" ? "I runtime non supportati da questo device sono nascosti." : "Prova con un altro termine di ricerca."}
                action={<>
                  {scope === "here" && <Button onClick={() => setScope("all")}>Mostra tutto il catalogo</Button>}
                  {query && <Button variant="ghost" onClick={() => setQuery("")}>Cancella la ricerca</Button>}
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
        <Icon name="info" size={14} /> Ogni runtime è un manifest JSON: per aggiungerne uno basta creare un file in <code>~/.aiw/runtimes/</code>, compare qui con le sue impostazioni.
      </p>

      {runtimeId && opened && (
        <RuntimeDrawer key={opened.id} runtime={opened} models={models.data ?? []} deviceAccels={deviceAccels} onClose={closeDrawer} />
      )}
      {runtimeId && !opened && rts.data && (
        <Sheet title="Runtime non trovato" onClose={closeDrawer} footer={<Button onClick={closeDrawer}>Torna al catalogo</Button>}>
          <p>Nessun runtime ha l'id <code>{runtimeId}</code>. Forse il manifest è stato rimosso: premi Rileva per aggiornare l'elenco.</p>
        </Sheet>
      )}
    </Page>
  );
}
