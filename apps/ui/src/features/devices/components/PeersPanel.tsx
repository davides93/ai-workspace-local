import { useState } from "react";
import type { PeerDevice } from "../../../shared/api/types";
import { Button, Chip, Dialog, Icon, IconButton, Panel, StatusPill } from "../../../shared/ui";
import { useRemovePeer } from "../api";
import { OS_LABEL } from "../model/mesh";

const MAX_SHARED = 4;

/** Paired devices: what each shares and its state; removing asks for confirmation. */
export function PeersPanel({ peers }: { peers: PeerDevice[] }) {
  const remove = useRemovePeer();
  const [removing, setRemoving] = useState<PeerDevice | null>(null);
  return (
    <Panel icon="device" title="Dispositivi accoppiati" padded={false}>
      <ul className="list dev-list" aria-label="Dispositivi accoppiati">
        {peers.map((p) => (
          <li className="list-item" key={p.id}>
            <Icon name="device" />
            <div className="grow">
              <b>{p.name}</b>
              <div className="faint mono ellipsis text-xs">{OS_LABEL[p.os] ?? p.os} · {p.address}</div>
            </div>
            <div className="chips peer-models" aria-label={`Runtime condivisi da ${p.name}`}>
              {p.shared_runtimes.slice(0, MAX_SHARED).map((r) => <Chip key={r}>{r}</Chip>)}
              {p.shared_runtimes.length > MAX_SHARED && <Chip>+{p.shared_runtimes.length - MAX_SHARED}</Chip>}
            </div>
            <StatusPill status={p.status} />
            <IconButton aria-label={`Rimuovi ${p.name}`} icon="trash" size={16} onClick={() => setRemoving(p)} />
          </li>
        ))}
        {peers.length === 0 && <li className="list-item faint text-sm">Nessun dispositivo accoppiato.</li>}
      </ul>
      {removing && (
        <Dialog
          title="Rimuovi il dispositivo" size="sm" onClose={() => setRemoving(null)}
          footer={<>
            <Button onClick={() => setRemoving(null)}>Annulla</Button>
            <Button variant="danger" icon="trash" disabled={remove.isPending} onClick={() => remove.mutate(removing, { onSettled: () => setRemoving(null) })}>Rimuovi</Button>
          </>}
        >
          <p>Vuoi rimuovere <b>{removing.name}</b>? I suoi modelli e agenti non saranno più disponibili qui; potrai accoppiarlo di nuovo con il codice.</p>
        </Dialog>
      )}
    </Panel>
  );
}
