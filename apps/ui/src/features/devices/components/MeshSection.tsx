import { useRef, useState } from "react";
import { useSystem } from "../../../shared/query/system";
import { Section } from "../../../shared/ui";
import type { DiscoveredDevice } from "../../../shared/api/types";
import { useMesh, usePairPeer } from "../api";
import { DiscoveredPanel } from "./DiscoveredPanel";
import { PairForm } from "./PairForm";
import { PairingCard } from "./PairingCard";
import { PeersPanel } from "./PeersPanel";

/** Device mesh: this device's pairing card, devices found on the LAN, pairing by address and the paired devices. */
export function MeshSection() {
  const system = useSystem().data;
  const mesh = useMesh();
  const pairPeer = usePairPeer();
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const codeRef = useRef<HTMLInputElement>(null);
  const peers = mesh.data?.peers ?? system?.peers ?? [];

  return (
    <Section
      className="section-top"
      title="Rete di dispositivi"
      hint={<>
        Un dispositivo accoppiato presta i suoi modelli (runtime <span className="mono">aiw-peer</span>) e i suoi agenti e strumenti (server MCP):
        il telefono usa la GPU del PC, il PC delega i task che servono sul telefono. Le altre app parlano con gli agenti via A2A.
      </>}
    >
      <div className="mesh-grid">
        <PairingCard code={system?.pairing_code} address={mesh.data?.address ?? null} discovery={mesh.data?.discovery} />
        <DiscoveredPanel
          discovered={mesh.data?.discovered ?? []} peers={peers} discoveryOn={!!mesh.data?.discovery} refreshing={mesh.isFetching}
          onRefresh={() => void mesh.refetch()}
          onChoose={(d: DiscoveredDevice) => { setAddress(d.address); codeRef.current?.focus(); }}
        >
          <PairForm
            address={address} code={code} codeRef={codeRef} pending={pairPeer.isPending} onAddress={setAddress} onCode={setCode}
            onSubmit={(input) => pairPeer.mutate(input, { onSuccess: () => { setAddress(""); setCode(""); } })}
          />
        </DiscoveredPanel>
      </div>
      <PeersPanel peers={peers} />
    </Section>
  );
}
