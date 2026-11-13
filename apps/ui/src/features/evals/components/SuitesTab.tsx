import type { UseQueryResult } from "@tanstack/react-query";
import { Link, useQueryParams } from "../../../app/router";
import { EmptyState, Icon, MasterDetail, QueryState } from "../../../shared/ui";
import type { EvalSuite } from "../../../shared/api/types";
import { NEW_SUITE, SuiteDetail } from "./SuiteDetail";

import { t } from "../../../shared/i18n";
const href = (id: string) => `/evals/suites?suite=${encodeURIComponent(id)}`;

/** Suites as links (the open one lives in the URL) and the editor of the open one. */
export function SuitesTab({ suites }: { suites: UseQueryResult<EvalSuite[]> }) {
  const open = useQueryParams().suite;
  const newLink = <Link to={href(NEW_SUITE)} className="btn primary"><Icon name="plus" size={15} /> {t("evals.nuova_suite")}</Link>;
  return (
    <QueryState
      query={suites} isEmpty={(s) => s.length === 0 && !open}
      empty={<EmptyState icon="check" title={t("evals.nessuna_suite")} description={t("evals.una_suite_raccoglie_task_un")} action={newLink} />}
    >
      {(all) => {
        const selected = open ?? all[0]?.id;
        return (
          <MasterDetail
            ratio="wide" className="ev-ws" detailOpen={!!open}
            list={
              <div className="panel ev-list">
                <nav className="list" aria-label={t("evals.suite_di_valutazione")}>
                  {selected === NEW_SUITE && (
                    <div className="list-item" aria-current="true"><Icon name="plus" /><div className="grow"><b>{t("evals.nuova_suite")}</b><div className="faint text-xs">{t("evals.non_ancora_salvata")}</div></div></div>
                  )}
                  {all.map((s) => (
                    <div key={s.id} className="list-item" aria-current={s.id === selected}>
                      <Link to={href(s.id)} className="list-link">
                        <Icon name="check" />
                        <div className="grow">
                          <b>{s.name}</b>
                          <div className="faint text-xs">{t("evals.task_count", { n: s.tasks.length })} · <span className="mono">{s.id}</span></div>
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
