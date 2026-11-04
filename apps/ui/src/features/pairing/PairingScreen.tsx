import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { useAction } from "../../shared/lib/useAction";
import { qk } from "../../shared/query/keys";
import { Button, Icon, Panel } from "../../shared/ui";

/** Shown when this browser talks to another device's AIW over the LAN without a token. */
export function PairingScreen() {
  const api = useApi();
  const qc = useQueryClient();
  const [code, setCode] = useState("");
  const { run, pending } = useAction(async () => {
    if (!api.pair) throw new Error("Questo carrier non supporta il pairing");
    await api.pair(code);
    toast("Dispositivo accoppiato");
    await qc.invalidateQueries({ queryKey: qk.system() });
  });
  const valid = code.replace(/\D/g, "").length === 6;
  return (
    <div className="page page-center">
      <Panel className="pairing-card">
        <div className="stack">
          <div className="row"><Icon name="link" size={22} /><h1>Accoppia questo dispositivo</h1></div>
          <p className="muted">Sul dispositivo che esegue AI Workspace apri <b>Dispositivi</b> e inserisci qui il codice a 6 cifre. Il codice cambia ogni 10 minuti.</p>
          <input className="input mono pair-input" id="pair-code" aria-label="Codice di pairing" inputMode="numeric" autoComplete="one-time-code" placeholder="000 000" value={code}
            onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === "Enter" && valid && void run()} />
          <Button variant="primary" disabled={pending || !valid} onClick={() => void run()}>Accoppia</Button>
        </div>
      </Panel>
    </div>
  );
}
