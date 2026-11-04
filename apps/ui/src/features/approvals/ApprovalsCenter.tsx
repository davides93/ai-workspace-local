import { useEffect, useState } from "react";
import { Icon, Pill } from "../../shared/ui";
import { useApprovals } from "./api";
import { ApprovalDrawer } from "./components/ApprovalDrawer";
import { pendingLabel } from "./model/shortcuts";
import "./approvals.css";

/**
 * Global inbox of pending confirmations (agents, sub-agents and unattended workflows): a banner
 * under the shell header with the count, and the drawer to decide. The banner's live region is
 * always mounted so a new request is announced to screen readers when it appears.
 */
export function ApprovalsCenter() {
  const { data } = useApprovals();
  const items = data ?? [];
  const [open, setOpen] = useState(false);
  const high = items.filter((a) => a.risk === "high").length;

  // The drawer closes with its last request and does not pop up again for the next one.
  useEffect(() => {
    if (items.length === 0) setOpen(false);
  }, [items.length]);

  return (
    <>
      <div role="status" aria-live="polite" aria-atomic="true">
        {items.length > 0 && (
          <button type="button" className="banner banner-action apv-banner" onClick={() => setOpen(true)} aria-haspopup="dialog">
            <Icon name="shield" size={16} />
            <span className="apv-banner-text">
              <b>{pendingLabel(items.length)}</b>
              <span className="apv-banner-reason"> · {items[0].reason}</span>
            </span>
            {high > 0 && <Pill tone="crit">{high === 1 ? "1 ad alto rischio" : `${high} ad alto rischio`}</Pill>}
            <span className="btn sm primary banner-cta" aria-hidden="true">Rivedi</span>
          </button>
        )}
      </div>
      {open && items.length > 0 && <ApprovalDrawer items={items} onClose={() => setOpen(false)} />}
    </>
  );
}
