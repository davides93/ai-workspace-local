import { useRef, useState } from "react";
import { toast } from "../../../shared/lib/toast";
import { relTime } from "../../../shared/lib/format";
import { useSystem } from "../../../shared/query/system";
import { Button, Icon, IconButton, Pill, StatusPill } from "../../../shared/ui";
import type { DiscoveredDevice } from "../../../shared/api/types";
import { useMesh, usePairingQr, usePairPeer, useRemovePeer } from "../api";

/** Same normalisation as the engine's peer ids and addresses (`peers.rs`). */
const slug = (s: string) => s.replace(/[^A-Za-z0-9]+/g, "-").toLowerCase().replace(/^-+|-+$/g, "").slice(0, 32);
const normAddress = (a: string) => {
  const s = a.trim().replace(/\/+$/, "");
  const url = /^https?:\/\//.test(s) ? s : `http://${s}`;
  const hostPort = url.split("://")[1] ?? "";
  return hostPort.includes(":") && !hostPort.endsWith("]") ? url : `${url}:7777`;
};

const OS_LABEL: Record<string, string> = { windows: "Windows", macos: "macOS", linux: "Linux", android: "Android", ios: "iOS" };

/** Device mesh: this device's pairing card (code, QR), devices found on the LAN and paired ones. */
export function MeshSection() {
  const system = useSystem().data;
  const mesh = useMesh();
  const address = mesh.data?.address ?? null;
  const qr = usePairingQr(address);
  const pairPeer = usePairPeer();
  const removePeer = useRemovePeer();
  const [target, setTarget] = useState("");
  const [code, setCode] = useState("");
  const codeRef = useRef<HTMLInputElement>(null);

  const peers = mesh.data?.peers ?? system?.peers ?? [];
  const paired = (d: DiscoveredDevice) => peers.some((p) => normAddress(p.address) === normAddress(d.address) || p.id === slug(d.id));
  const discovered = mesh.data?.discovered ?? [];

  const choose = (d: DiscoveredDevice) => {
    setTarget(d.address);
    codeRef.current?.focus();
  };
  const pair = () => {
    const c = code.replace(/\D/g, "");
    if (!target.trim()) return toast("Indica l'indirizzo del dispositivo (host:porta)", "crit");
    if (c.length !== 6) return toast("Il codice di pairing è di 6 cifre", "crit");
    pairPeer.mutate({ address: target.trim(), code: c }, { onSuccess: () => { setTarget(""); setCode(""); } });
  };

  return (
    <section className="section section-top">
      <div className="section-head">
        <h2>Rete di dispositivi</h2>
        <span className="muted">
          Un dispositivo accoppiato presta i suoi modelli (runtime <span className="mono">aiw-peer</span>) e i suoi agenti e strumenti (server MCP):
          il telefono usa la GPU del PC, il PC delega i task che servono sul telefono. Le altre app parlano con gli agenti via A2A.
        </span>
      </div>

      <div className="mesh-grid">
        <div className="panel">
          <div className="panel-head">
            <Icon name="link" />
            <div className="grow"><h2>Accoppia con questo dispositivo</h2></div>
          </div>
          <div className="panel-body mesh-me">
            <div className="qr" aria-live="polite">
              {qr.data ? (
                <img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(qr.data.svg)}`} alt="QR per accoppiare un telefono" width={180} height={180} />
              ) : (
                <span className="faint">{address ? (qr.error ? "QR non disponibile" : "Genero il QR…") : "QR disponibile con l'accesso dalla LAN"}</span>
              )}
            </div>
            <div className="stack min-w-0">
              <span className="eyebrow">Codice di pairing</span>
              <span className="mono pair-code" aria-label="Codice di pairing">{system?.pairing_code ?? "—"}</span>
              <span className="muted text-sm">
                Inquadra il QR con la fotocamera del telefono: apre l'app già accoppiata. Da un altro PC inserisci l'indirizzo e il codice.
              </span>
              <dl className="kv">
                <dt>Indirizzo</dt><dd className="mono">{address ?? "solo locale"}</dd>
                <dt>Scoperta</dt><dd>{mesh.data?.discovery ? <Pill tone="ok">mDNS attivo</Pill> : <Pill>spenta</Pill>}</dd>
              </dl>
              {!address && (
                <span className="faint text-xs">
                  Attiva "Accesso dalla LAN" nelle preferenze (o avvia <span className="mono">aiw serve --lan</span>): le richieste dalla LAN richiedono sempre il token di pairing.
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="panel">
          <div className="panel-head">
            <Icon name="globe" />
            <div className="grow"><h2>Nella rete</h2></div>
            <IconButton aria-label="Aggiorna" icon="refresh" size={16} onClick={() => void mesh.refetch()} />
          </div>
          <div className="list">
            {discovered.map((d) => (
              <div className="list-item" key={d.id}>
                <Icon name="device" />
                <div className="grow">
                  <b className="ellipsis block">{d.name}</b>
                  <div className="faint mono ellipsis text-xs">{OS_LABEL[d.os] ?? d.os} · {d.tier.toUpperCase()} · {d.address} · {relTime(d.last_seen)}</div>
                </div>
                {paired(d) ? <Pill tone="ok">accoppiato</Pill> : <Button size="sm" onClick={() => choose(d)}>Accoppia</Button>}
              </div>
            ))}
            {discovered.length === 0 && (
              <div className="list-item faint text-sm">
                {mesh.data?.discovery ? "Nessun altro dispositivo AI Workspace trovato in questa rete." : "La scoperta automatica funziona con l'accesso dalla LAN attivo. Puoi sempre accoppiare per indirizzo."}
              </div>
            )}
          </div>
          <form className="panel-body pair-form" onSubmit={(e) => { e.preventDefault(); pair(); }}>
            <div className="field grow">
              <label htmlFor="peer-address">Indirizzo</label>
              <input id="peer-address" className="input mono" placeholder="192.168.1.20:7777" value={target} onChange={(e) => setTarget(e.target.value)} autoComplete="off" />
            </div>
            <div className="field">
              <label htmlFor="peer-code">Codice</label>
              <input id="peer-code" ref={codeRef} className="input mono" inputMode="numeric" placeholder="123456" maxLength={7} value={code} onChange={(e) => setCode(e.target.value)} autoComplete="one-time-code" />
            </div>
            <Button variant="primary" type="submit" disabled={pairPeer.isPending}>{pairPeer.isPending ? "Accoppio…" : "Accoppia"}</Button>
          </form>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <Icon name="device" />
          <div className="grow"><h2>Dispositivi accoppiati</h2></div>
        </div>
        <div className="list">
          {peers.map((p) => (
            <div className="list-item" key={p.id}>
              <Icon name="device" />
              <div className="grow">
                <b>{p.name}</b>
                <div className="faint mono ellipsis text-xs">{OS_LABEL[p.os] ?? p.os} · {p.address}</div>
              </div>
              <div className="chips peer-models">{p.shared_runtimes.slice(0, 4).map((r) => <span className="chip" key={r}>{r}</span>)}{p.shared_runtimes.length > 4 && <span className="chip">+{p.shared_runtimes.length - 4}</span>}</div>
              <StatusPill status={p.status} />
              <IconButton aria-label={`Rimuovi ${p.name}`} icon="trash" size={16} onClick={() => removePeer.mutate({ id: p.id, name: p.name })} />
            </div>
          ))}
          {peers.length === 0 && <div className="list-item faint">Nessun dispositivo accoppiato.</div>}
        </div>
      </div>
    </section>
  );
}
