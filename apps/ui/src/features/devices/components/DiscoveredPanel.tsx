import type { DiscoveredDevice, PeerDevice } from "../../../shared/api/types";
import { relTime } from "../../../shared/lib/format";
import { Button, Icon, IconButton, Panel, Pill } from "../../../shared/ui";
import { isPaired, OS_LABEL } from "../model/mesh";

/** Devices found on the LAN by mDNS; "Accoppia" fills the address in the form below. */
export function DiscoveredPanel({ discovered, peers, discoveryOn, refreshing, onRefresh, onChoose, children }: {
  discovered: DiscoveredDevice[];
  peers: PeerDevice[];
  discoveryOn: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onChoose: (d: DiscoveredDevice) => void;
  /** The pair-by-address form. */
  children: React.ReactNode;
}) {
  return (
    <Panel
      icon="globe" title="Nella rete" padded={false}
      actions={<IconButton aria-label="Cerca di nuovo nella rete" icon="refresh" size={16} onClick={onRefresh} disabled={refreshing} />}
    >
      <ul className="list dev-list" aria-label="Dispositivi trovati nella rete">
        {discovered.map((d) => (
          <li className="list-item" key={d.id}>
            <Icon name="device" />
            <div className="grow">
              <b className="ellipsis block">{d.name}</b>
              <div className="faint mono ellipsis text-xs">{OS_LABEL[d.os] ?? d.os} · {d.tier.toUpperCase()} · {d.address} · {relTime(d.last_seen)}</div>
            </div>
            {isPaired(peers, d) ? <Pill tone="ok">accoppiato</Pill> : <Button size="sm" aria-label={`Accoppia ${d.name}`} onClick={() => onChoose(d)}>Accoppia</Button>}
          </li>
        ))}
        {discovered.length === 0 && (
          <li className="list-item faint text-sm">
            {discoveryOn ? "Nessun altro dispositivo AI Workspace trovato in questa rete." : "La scoperta automatica funziona con l'accesso dalla LAN attivo. Puoi sempre accoppiare per indirizzo."}
          </li>
        )}
      </ul>
      {children}
    </Panel>
  );
}
