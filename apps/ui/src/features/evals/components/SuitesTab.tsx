import type { UseQueryResult } from "@tanstack/react-query";
import { Link, useQueryParams } from "../../../app/router";
import { EmptyState, Icon, MasterDetail, QueryState } from "../../../shared/ui";
import type { EvalSuite } from "../../../shared/api/types";
import { NEW_SUITE, SuiteDetail } from "./SuiteDetail";

const href = (id: string) => `/evals/suites?suite=${encodeURIComponent(id)}`;

/** Suites as links (the open one lives in the URL) and the editor of the open one. */
export function SuitesTab({ suites }: { suites: UseQueryResult<EvalSuite[]> }) {
  const open = useQueryParams().suite;
  const newLink = <Link to={href(NEW_SUITE)} className="btn primary"><Icon name="plus" size={15} /> Nuova suite</Link>;
  return (
    <QueryState
      query={suites} isEmpty={(s) => s.length === 0 && !open}
      empty={<EmptyState icon="check" title="Nessuna suite" description="Una suite raccoglie task (un prompt e, se serve, dei file di partenza) e i grader che dicono se l'agente li ha svolti." action={newLink} />}
    >
      {(all) => {
        const selected = open ?? all[0]?.id;
        return (
          <MasterDetail
            ratio="wide" className="ev-ws" detailOpen={!!open}
            list={
              <div className="panel ev-list">
                <nav className="list" aria-label="Suite di valutazione">
                  {selected === NEW_SUITE && (
                    <div className="list-item" aria-current="true"><Icon name="plus" /><div className="grow"><b>Nuova suite</b><div className="faint text-xs">Non ancora salvata</div></div></div>
                  )}
                  {all.map((s) => (
                    <div key={s.id} className="list-item" aria-current={s.id === selected}>
                      <Link to={href(s.id)} className="list-link">
                        <Icon name="check" />
                        <div className="grow">
                          <b>{s.name}</b>
                          <div className="faint text-xs">{s.tasks.length} {s.tasks.length === 1 ? "task" : "task"} · <span className="mono">{s.id}</span></div>
                        </div>
                      </Link>
                    </div>
                  ))}
                </nav>
                <div className="ev-list-add">{newLink}</div>
              </div>
            }
            detail={selected ? <SuiteDetail key={selected} suiteId={selected} suites={all} /> : null}
          />
        );
      }}
    </QueryState>
  );
}
