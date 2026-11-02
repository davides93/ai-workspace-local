import { useState } from "react";
import { Button, Drawer, Icon, Pill } from "../../shared/ui";
import { useApprovals, useRespondApproval } from "./api";
import { RISK_LABEL, RISK_TONE } from "../../shared/lib/risk";

/** Global inbox of pending confirmations (agents, sub-agents and unattended workflows). */
export function ApprovalsCenter() {
  const { data } = useApprovals();
  const respond = useRespondApproval();
  const [open, setOpen] = useState(false);
  const items = data ?? [];

  if (items.length === 0) return null;
  return (
    <>
      <div role="status" aria-live="polite">
        <button type="button" className="banner banner-action" onClick={() => setOpen(true)}>
          <Icon name="shield" size={16} />
          <span><b>{items.length === 1 ? "1 conferma in attesa" : `${items.length} conferme in attesa`}</b> · {items[0].reason}</span>
          <span className="btn sm primary banner-cta">Rivedi</span>
        </button>
      </div>
      {open && (
        <Drawer title="Conferme in attesa" subtitle="Gli agenti e i workflow sono in pausa finché non decidi" onClose={() => setOpen(false)}>
          {items.map((a) => (
            <div key={a.id} className="approval">
              <div className="row">
                <span className="mono">{a.tool}</span>
                <Pill tone={RISK_TONE[a.risk]}>rischio {RISK_LABEL[a.risk]}</Pill>
                <span className="faint mono text-xs ml-auto">{a.run_id}</span>
              </div>
              <span>{a.reason}</span>
              <pre>{JSON.stringify(a.args, null, 2)}</pre>
              <div className="row">
                <Button variant="primary" icon="check" disabled={respond.isPending} onClick={() => respond.mutate({ request: a, decision: "allow" })}>Consenti</Button>
                <Button variant="danger" disabled={respond.isPending} onClick={() => respond.mutate({ request: a, decision: "deny" })}>Nega</Button>
              </div>
            </div>
          ))}
        </Drawer>
      )}
    </>
  );
}
