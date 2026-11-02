import { errorMessage } from "../../../shared/lib/errors";
import { Pill, Panel, CopyButton, KeyValue } from "../../../shared/ui";
import { usePairingQr } from "../api";
import { digitsOf, groupCode } from "../model/mesh";

/** This device's side of pairing: the QR for a phone, the 6-digit code and where the device is reachable. */
export function PairingCard({ code, address, discovery }: { code?: string; address: string | null; discovery?: boolean }) {
  const qr = usePairingQr(address);
  return (
    <Panel icon="link" title="Accoppia con questo dispositivo">
      <div className="mesh-me">
        <div className="qr" aria-live="polite">
          {qr.data ? (
            <img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(qr.data.svg)}`} alt="Codice QR per accoppiare un telefono" width={180} height={180} />
          ) : (
            <span className="dev-qr-note" title={qr.error ? errorMessage(qr.error) : undefined}>
              {!address ? "Il QR è disponibile con l'accesso dalla LAN" : qr.error ? "QR non disponibile" : "Genero il QR…"}
            </span>
          )}
        </div>
        <div className="stack min-w-0">
          <span className="eyebrow">Codice di pairing</span>
          <div className="row nowrap">
            <span className="mono pair-code" aria-label={`Codice di pairing ${code ? digitsOf(code).split("").join(" ") : "non disponibile"}`}>{code ? groupCode(code) : "—"}</span>
            {code && <CopyButton text={digitsOf(code)} label="Copia" />}
          </div>
          <span className="muted text-sm">
            Inquadra il QR con la fotocamera del telefono: apre l'app già accoppiata. Da un altro PC inserisci l'indirizzo e il codice.
          </span>
          <KeyValue items={[
            ["Indirizzo", <span className="mono">{address ?? "solo locale"}</span>],
            ["Scoperta", discovery ? <Pill tone="ok">mDNS attivo</Pill> : <Pill>spenta</Pill>],
          ]} />
          {!address && (
            <span className="faint text-xs">
              Attiva «Accesso dalla LAN» nelle Impostazioni (o avvia <span className="mono">aiw serve --lan</span>): le richieste dalla LAN richiedono sempre il token di pairing.
            </span>
          )}
        </div>
      </div>
    </Panel>
  );
}
