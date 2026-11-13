import { Button, EmptyState, Page, PageHeader, Panel, QueryState } from "../../shared/ui";
import { useApproveProposal, useDismissProposal, useProactiveStatus, useProactiveTick, useProposals } from "./api";
import { DoneRow, ProposalCard } from "./components/ProposalCard";
import { decided, latestBriefing, statusLine, waiting } from "./model/proposals";
import "./inbox.css";

import { t } from "../../shared/i18n";
/** `#/inbox` — "Proposte": what the assistant noticed on its own. It never acts alone: Approva starts a normal run. */
export function InboxView() {
  const proposals = useProposals();
  const status = useProactiveStatus();
  const approve = useApproveProposal();
  const dismiss = useDismissProposal();
  const tick = useProactiveTick();
  const busyId = approve.isPending ? approve.variables?.id : dismiss.isPending ? dismiss.variables : undefined;
  const off = status.data?.level === "off";
  return (
    <Page className="ib">
      <PageHeader
        eyebrow={t("nav.group_work")} title={t("inbox.proposte")}
        subtitle={t("inbox.cose_che_l_assistente_ha")}
        actions={!off && <Button size="sm" icon="refresh" disabled={tick.isPending} onClick={() => tick.mutate()}>{t("inbox.controlla_adesso")}</Button>}
      />
      {status.data && <p className="muted text-sm ib-status" role="status">{statusLine(status.data)}</p>}
      <QueryState query={proposals}>
        {(list) => {
          const open = waiting(list);
          const briefing = latestBriefing(list);
          const done = decided(list);
          return (
            <>
              {briefing && (
                <Panel title={briefing.title} icon="bulb" actions={<Button size="sm" variant="ghost" icon="close" disabled={busyId === briefing.id} onClick={() => dismiss.mutate(briefing.id)}>{t("inbox.fatto")}</Button>}>
                  <p className="text-sm">{briefing.noticed}</p>
                </Panel>
              )}
              {open.length === 0 ? (
                <EmptyState
                  icon="check" title={t("inbox.nessuna_proposta_per_ora")}
                  description={off ? t("inbox.accendi_l_assistente_proattivo_dalle") : t("inbox.quando_l_assistente_nota_qualcosa")}
                />
              ) : (
                <div className="stack" aria-label={t("inbox.da_guardare")}>
                  {open.map((p) => (
                    <ProposalCard
                      key={p.id} proposal={p} busy={busyId === p.id}
                      onApprove={(instruction) => approve.mutate({ id: p.id, instruction })} onDismiss={() => dismiss.mutate(p.id)}
                    />
                  ))}
                </div>
              )}
              {done.length > 0 && (
                <Panel title={t("inbox.fatte")} icon="check">
                  <ul className="ib-done-list">{done.slice(0, 20).map((p) => <DoneRow key={p.id} proposal={p} />)}</ul>
                </Panel>
              )}
            </>
          );
        }}
      </QueryState>
    </Page>
  );
}
