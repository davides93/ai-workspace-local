import { useEffect, useState } from "react";
import { useApp } from "../state/app";
import { Drawer, Icon } from "./ui";
import type { ApprovalRequest } from "../api/types";

/** Global inbox of pending confirmations (agents, sub-agents and unattended workflows). */
export function ApprovalsCenter() {
  const { api, toast } = useApp();
  const [items, setItems] = useState<ApprovalRequest[]>([]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let alive = true;
    const load = () => api.approvals().then((a) => alive && setItems(a)).catch(() => {});
    load();
    const t = setInterval(load, 2500);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [api]);

  const respond = async (a: ApprovalRequest, decision: "allow" | "deny") => {
    try {
      await api.respondApproval(a.id, decision);
      setItems((x) => x.filter((i) => i.id !== a.id));
      toast(decision === "allow" ? "Consentito" : "Negato");
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    }
  };

  if (items.length === 0) return null;
  return (
    <>
      <button className="banner" style={{ border: 0, width: "100%", cursor: "pointer", background: "var(--accent-soft)", color: "var(--fg)" }} onClick={() => setOpen(true)}>
        <Icon name="shield" size={16} />
        <span><b>{items.length === 1 ? "1 conferma in attesa" : `${items.length} conferme in attesa`}</b> · {items[0].reason}</span>
        <span style={{ marginLeft: "auto" }} className="btn sm primary">Rivedi</span>
      </button>
      {open && (
        <Drawer title="Conferme in attesa" subtitle="Gli agenti e i workflow sono in pausa finché non decidi" onClose={() => setOpen(false)}>
          {items.map((a) => (
            <div key={a.id} className="approval">
              <div className="row">
                <span className="mono">{a.tool}</span>
                <span className={`pill ${a.risk === "high" ? "crit" : a.risk === "medium" ? "warn" : "info"}`}>rischio {a.risk === "high" ? "alto" : a.risk === "medium" ? "medio" : "basso"}</span>
                <span className="faint mono" style={{ fontSize: "var(--fs-xs)", marginLeft: "auto" }}>{a.run_id}</span>
              </div>
              <span>{a.reason}</span>
              <pre>{JSON.stringify(a.args, null, 2)}</pre>
              <div className="row">
                <button className="btn primary" onClick={() => respond(a, "allow")}><Icon name="check" size={15} /> Consenti</button>
                <button className="btn danger" onClick={() => respond(a, "deny")}>Nega</button>
              </div>
            </div>
          ))}
        </Drawer>
      )}
    </>
  );
}
