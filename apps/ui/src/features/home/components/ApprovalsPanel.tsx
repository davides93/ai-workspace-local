import { Panel, Pill, QueryState } from "../../../shared/ui";
import { useApprovals, useRespondApproval } from "../../approvals/api";
import { ApprovalCard } from "../../approvals/components/ApprovalCard";

import { t } from "../../../shared/i18n";
const SHOWN = 3;

/** Confirmations the agents are waiting for; the same card as in the chat and in the drawer. */
export function ApprovalsPanel() {
  const approvals = useApprovals();
  const respond = useRespondApproval();
  const items = approvals.data ?? [];
  const busyId = respond.isPending ? respond.variables?.request.id : undefined;
  return (
    <Panel title={t("home.conferme_in_attesa")} icon="shield" actions={approvals.data && <Pill tone={items.length ? "warn" : "neutral"}>{items.length}</Pill>}>
      <QueryState query={approvals}>
        {() =>
          items.length === 0 ? (
            <p className="muted text-sm">{t("home.nessuna_conferma_richiesta_gli_agenti")}</p>
          ) : (
            <div className="stack">
              {items.slice(0, SHOWN).map((a) => (
                <ApprovalCard key={a.id} request={a} variant="compact" pending={busyId === a.id} onRespond={(decision) => respond.mutate({ request: a, decision })} />
              ))}
              {items.length > SHOWN && <span className="faint text-sm">{t("home.e_altre_aprile_dal_banner", { p: items.length - SHOWN })}</span>}
            </div>
          )
        }
      </QueryState>
    </Panel>
  );
}
