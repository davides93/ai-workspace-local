import { useEffect, useRef, useState } from "react";
import { Drawer } from "../../../shared/ui";
import type { ApprovalRequest } from "../../../shared/api/types";
import { useRespondApproval } from "../api";
import { approvalShortcut, isTypingTarget, moveSelection, pendingLabel } from "../model/shortcuts";
import { ApprovalCard } from "./ApprovalCard";

import { t, tNode } from "../../../shared/i18n";
/**
 * Side drawer with every pending confirmation. A allows and N denies the highlighted request
 * (the first one to begin with), ↑/↓ change it, so a queue is cleared from the keyboard.
 */
export function ApprovalDrawer({ items, onClose }: { items: ApprovalRequest[]; onClose: () => void }) {
  const respond = useRespondApproval();
  const [selected, setSelected] = useState(0);
  const index = Math.min(selected, Math.max(0, items.length - 1));
  const busyId = respond.isPending ? respond.variables?.request.id : undefined;

  // The handler always sees the latest list without re-subscribing on every poll.
  const latest = useRef({ items, index, busyId });
  latest.current = { items, index, busyId };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { items: list, index: cur, busyId: busy } = latest.current;
      const cmd = approvalShortcut(e, { count: list.length, typing: isTypingTarget(e.target) });
      if (!cmd) return;
      e.preventDefault();
      if (cmd === "prev" || cmd === "next") return setSelected(moveSelection(cur, list.length, cmd));
      const target = list[cur];
      if (target && !busy) respond.mutate({ request: target, decision: cmd });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // `respond.mutate` is stable, so the listener is attached once.
  }, []);

  return (
    <Drawer
      title={t("approvals.conferme_in_attesa", { length: items.length })}
      subtitle={t("approvals.gli_agenti_e_i_workflow")}
      onClose={onClose}
      footer={
        <p className="apv-hints" aria-label={t("approvals.scorciatoie_da_tastiera")}>
          <span>{tNode("approvals.consenti", { kbd: <kbd>{t("approvals.a")}</kbd> })}</span>
          <span>{tNode("approvals.nega", { kbd: <kbd>{t("approvals.n")}</kbd> })}</span>
          {items.length > 1 && <span>{tNode("approvals.cambia_richiesta", { kbd: <kbd>↑</kbd>, kbd2: <kbd>↓</kbd> })}</span>}
          <span>{tNode("approvals.chiudi", { kbd: <kbd>{t("approvals.esc")}</kbd> })}</span>
        </p>
      }
    >
      <p className="sr-only" role="status">{pendingLabel(items.length)}</p>
      {items.map((a, i) => (
        <ApprovalCard
          key={a.id}
          request={a}
          selected={i === index}
          showShortcuts={i === index}
          pending={busyId === a.id}
          onSelect={() => setSelected(i)}
          onRespond={(decision) => respond.mutate({ request: a, decision })}
        />
      ))}
    </Drawer>
  );
}
