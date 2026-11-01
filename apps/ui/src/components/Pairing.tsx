import { useState } from "react";
import { useApp } from "../state/app";
import { Icon } from "./ui";

/** Shown when this browser talks to another device's AIW over the LAN without a token. */
export function PairingScreen() {
  const { api, refreshSystem, toast } = useApp();
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    try {
      await (api as unknown as { pair: (c: string) => Promise<void> }).pair(code);
      toast("Dispositivo accoppiato");
      refreshSystem();
    } catch (e) {
      toast(String((e as Error).message ?? e), "crit");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="page" style={{ display: "grid", placeItems: "center" }}>
      <div className="panel" style={{ maxWidth: 420, width: "100%" }}>
        <div className="panel-body stack" style={{ gap: 14 }}>
          <div className="row"><Icon name="link" size={22} /><h1>Accoppia questo dispositivo</h1></div>
          <p className="muted">Sul dispositivo che esegue AI Workspace apri <b>Dispositivi</b> e inserisci qui il codice a 6 cifre. Il codice cambia ogni 10 minuti.</p>
          <input className="input mono" id="pair-code" aria-label="Codice di pairing" inputMode="numeric" placeholder="000 000" value={code} onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} style={{ fontSize: "1.4rem", letterSpacing: "0.2em", textAlign: "center" }} />
          <button className="btn primary" disabled={busy || code.replace(/\D/g, "").length !== 6} onClick={submit}>Accoppia</button>
        </div>
      </div>
    </div>
  );
}
