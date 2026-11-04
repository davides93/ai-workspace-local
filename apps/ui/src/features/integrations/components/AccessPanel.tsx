import { Link } from "../../../app/router";
import { Icon, Panel, Pill, Segmented } from "../../../shared/ui";

export type Audience = "local" | "lan";

/** Who calls the endpoints, and what that client needs: nothing from this computer, the pairing token from the LAN. */
export function AccessPanel({ audience, onAudience, lanAddress }: {
  audience: Audience;
  onAudience: (a: Audience) => void;
  /** LAN address of this device, `null` when the server listens on this computer only. */
  lanAddress: string | null | undefined;
}) {
  return (
    <Panel
      icon="lock" title="Accesso e sicurezza"
      actions={(
        <Segmented
          label="Da dove si collega il client" value={audience} onChange={onAudience}
          options={[{ id: "local", label: "Da questo computer" }, { id: "lan", label: "Da un altro dispositivo" }]}
        />
      )}
    >
      <div className="stack">
        {audience === "lan" && !lanAddress && (
          <p className="int-warn text-sm">
            <Icon name="alert" size={15} />
            <span>Il server ascolta solo su questo computer. Attiva «Accesso dalla LAN» nelle <Link to="/settings">Impostazioni</Link> (o avvia <code>aiw serve --lan</code>): gli esempi qui sotto usano un indirizzo da sostituire.</span>
          </p>
        )}
        <div className="int-notes">
          <section className="int-note" aria-label="Da questo computer">
            <h3><Icon name="device" size={16} /> Da questo computer</h3>
            <p className="muted text-sm">
              Script, curl e SDK rispondono senza token. Il server controlla anche <code>Host</code> e <code>Origin</code>: una pagina web di un altro sito che prova a chiamare <code>127.0.0.1</code> riceve 403.
            </p>
          </section>
          <section className="int-note" aria-label="Pagine web nel browser">
            <h3><Icon name="globe" size={16} /> Pagine web nel browser</h3>
            <p className="muted text-sm">
              Anche le richieste da localhost passano dal controllo dell'<code>Origin</code> (protegge da CSRF e DNS rebinding). Sono ammessi l'app desktop, <code>localhost</code> e <code>127.0.0.1</code> su qualsiasi porta e l'indirizzo LAN di questo dispositivo.
              Con il token la richiesta è autorizzata, ma una pagina di un'altra origine non ne legge la risposta (CORS). I webhook accettano ogni origine: il loro segreto è la credenziale.
            </p>
          </section>
          <section className="int-note" aria-label="Da altri dispositivi">
            <h3>
              <Icon name="link" size={16} /> Da altri dispositivi
              {lanAddress ? <Pill tone="ok">LAN attiva</Pill> : <Pill>solo locale</Pill>}
            </h3>
            <p className="muted text-sm">
              Servono l'accesso dalla LAN e il token del pairing, da inviare come <code>Authorization: Bearer</code> oppure <code>x-api-key</code>.
              Il token si ottiene accoppiando il dispositivo da <Link to="/devices">Dispositivi</Link>.
            </p>
          </section>
        </div>
      </div>
    </Panel>
  );
}
