import { Button, EmptyState, Page, PageHeader, Panel, QueryState } from "../../shared/ui";
import { useApproveProposal, useDismissProposal, useProactiveStatus, useProactiveTick, useProposals } from "./api";
import { DoneRow, ProposalCard } from "./components/ProposalCard";
import { decided, latestBriefing, statusLine, waiting } from "./model/proposals";
import "./inbox.css";

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
        eyebrow="Lavoro" title="Proposte"
        subtitle="Cose che l'assistente ha notato mentre non c'eri. Non fa nulla da solo: parte solo se approvi."
        actions={!off && <Button size="sm" icon="refresh" disabled={tick.isPending} onClick={() => tick.mutate()}>Controlla adesso</Button>}
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
                <Panel title={briefing.title} icon="bulb" actions={<Button size="sm" variant="ghost" icon="close" disabled={busyId === briefing.id} onClick={() => dismiss.mutate(briefing.id)}>Fatto</Button>}>
                  <p className="text-sm">{briefing.noticed}</p>
                </Panel>
              )}
              {open.length === 0 ? (
                <EmptyState
                  icon="check" title="Nessuna proposta per ora"
                  description={off ? "Accendi l'assistente proattivo dalle impostazioni e comincerà a guardarsi intorno." : "Quando l'assistente nota qualcosa di utile lo trovi qui."}
                />
              ) : (
                <div className="stack" aria-label="Da guardare">
                  {open.map((p) => (
                    <ProposalCard
                      key={p.id} proposal={p} busy={busyId === p.id}
                      onApprove={(instruction) => approve.mutate({ id: p.id, instruction })} onDismiss={() => dismiss.mutate(p.id)}
                    />
                  ))}
                </div>
              )}
              {done.length > 0 && (
                <Panel title="Fatte" icon="check">
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
