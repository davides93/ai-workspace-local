import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApi } from "../../shared/api/context";
import { toast } from "../../shared/lib/toast";
import { useAction } from "../../shared/lib/useAction";
import { qk } from "../../shared/query/keys";
import { Button, Icon, Panel } from "../../shared/ui";

import { t, tNode } from "../../shared/i18n";
/** Shown when this browser talks to another device's AIW over the LAN without a token. */
export function PairingScreen() {
  const api = useApi();
  const qc = useQueryClient();
  const [code, setCode] = useState("");
  const { run, pending } = useAction(async () => {
    if (!api.pair) throw new Error(t("pairing.questo_carrier_non_supporta_il"));
    await api.pair(code);
    toast(t("pairing.dispositivo_accoppiato"));
    await qc.invalidateQueries({ queryKey: qk.system() });
  });
  const valid = code.replace(/\D/g, "").length === 6;
  return (
    <div className="page page-center">
      <Panel className="pairing-card">
        <div className="stack">
          <div className="row"><Icon name="link" size={22} /><h1>{t("pairing.accoppia_questo_dispositivo")}</h1></div>
          <p className="muted">{tNode("shell.pair_help", { devices: <b>{t("pairing.dispositivi")}</b> })}</p>
          <input className="input mono pair-input" id="pair-code" aria-label={t("pairing.codice_di_pairing")} inputMode="numeric" autoComplete="one-time-code" placeholder="000 000" value={code}
            onChange={(e) => setCode(e.target.value)} onKeyDown={(e) => e.key === "Enter" && valid && void run()} />
          <Button variant="primary" disabled={pending || !valid} onClick={() => void run()}>{t("pairing.accoppia")}</Button>
        </div>
      </Panel>
    </div>
  );
}
