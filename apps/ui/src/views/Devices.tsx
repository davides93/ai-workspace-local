import { useRef, useState } from "react";
import { useApp } from "../state/app";
import { Icon, Meter, StatusPill, formatMb, relTime, useAsync } from "../components/ui";
import type { DeviceTier, DiscoveredDevice, PeerDevice } from "../api/types";

const TIER: Record<DeviceTier, { label: string; models: string }> = {
  xs: { label: "Molto piccolo", models: "Modelli di sistema e fino a 1–2B (Gemma 4 E2B, Qwen3 0.6B)" },
  s: { label: "Piccolo", models: "Fino a circa 4B quantizzati (Gemma 4 E4B, Phi-4 mini)" },
  m: { label: "Medio", models: "7–14B e MoE con pochi parametri attivi" },
  l: { label: "Grande", models: "MoE fino a circa 35B (A3B) e densi fino a 27–32B" },
  xl: { label: "Molto grande", models: "Densi da 70B, MoE da oltre 100B con offload" },
};

export function DevicesView() {
  const { system } = useApp();
  if (!system) return <div className="page muted">Rilevamento hardware…</div>;
  const d = system.device;
  const usedPct = 1 - d.available_memory_mb / d.memory_mb;
  return (
    <div className="page">
      <div className="page-head">
        <div className="titles">
          <span className="eyebrow">Inferenza</span>
          <h1>Dispositivi</h1>
          <span className="muted">Il profilo hardware guida la scelta automatica di runtime e modelli; puoi sempre forzarla.</span>
        </div>
      </div>

      <div className="device-hero">
        <div className="panel">
          <div className="panel-head">
            <Icon name="device" />
            <div className="grow">
              <h2>{d.name}</h2>
              <span className="muted" style={{ fontSize: "var(--fs-sm)" }}>{d.os_version} · {d.arch} · {d.form_factor}</span>
            </div>
            <span className="pill ok"><span className="led ok" style={{ width: 7, height: 7, boxShadow: "none" }} /> questo device</span>
          </div>
          <div className="panel-body stack" style={{ gap: 14 }}>
            <dl className="kv">
              <dt>CPU</dt><dd>{d.cpu.brand} · {d.cpu.cores} core / {d.cpu.threads} thread</dd>
              <dt>Istruzioni</dt><dd><div className="chips">{d.cpu.features.map((f) => <span key={f} className="chip">{f}</span>)}</div></dd>
              <dt>Dati</dt><dd className="mono">{system.data_dir}</dd>
              <dt>Workspace</dt><dd className="mono">{system.workspace_root}</dd>
            </dl>
            <div className="stack" style={{ gap: 6 }}>
              <div className="row" style={{ justifyContent: "space-between" }}>
                <b style={{ fontSize: "var(--fs-sm)" }}>Memoria</b>
                <span className="mono faint num" style={{ fontSize: "var(--fs-xs)" }}>{formatMb(d.memory_mb - d.available_memory_mb)} usati di {formatMb(d.memory_mb)}</span>
              </div>
              <Meter value={d.memory_mb - d.available_memory_mb} max={d.memory_mb} tone={usedPct > 0.85 ? "crit" : usedPct > 0.7 ? "warn" : "ok"} />
            </div>
            <div>
              <b style={{ fontSize: "var(--fs-sm)" }}>Acceleratori</b>
              {d.accelerators.map((a, i) => (
                <div className="accel" key={i}>
                  <span className="chip hot">{a.kind}</span>
                  <div className="grow" style={{ minWidth: 0 }}>
                    <div className="ellipsis">{a.name}</div>
                    <div className="faint mono ellipsis" style={{ fontSize: "var(--fs-xs)" }}>{[a.vendor, a.driver, a.details].filter(Boolean).join(" · ")}</div>
                  </div>
                  <span className="mono num">{a.memory_mb ? formatMb(a.memory_mb) : ""}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className="panel">
          <div className="panel-body stack" style={{ gap: 12 }}>
            <span className="eyebrow">Classe del dispositivo</span>
            <div className="row" style={{ alignItems: "baseline", gap: 14 }}>
              <span className="tier">{d.tier.toUpperCase()}</span>
              <b>{TIER[d.tier].label}</b>
            </div>
            <p className="muted">{TIER[d.tier].models}</p>
            <div className="stack" style={{ gap: 6 }}>
              {(Object.keys(TIER) as DeviceTier[]).map((t) => (
                <div key={t} className="row" style={{ flexWrap: "nowrap" }}>
                  <span className={`chip ${t === d.tier ? "hot" : ""}`} style={{ width: 34, justifyContent: "center" }}>{t.toUpperCase()}</span>
                  <span className={t === d.tier ? "" : "faint"} style={{ fontSize: "var(--fs-sm)" }}>{TIER[t].models}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      <MeshSection />
    </div>
  );
}

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
function MeshSection() {
  const { api, system, toast, refreshSystem } = useApp();
  const mesh = useAsync(() => api.mesh(), [api], 8000);
  const address = mesh.data?.address ?? null;
  const qr = useAsync(() => (address ? api.pairingQr() : Promise.resolve(null)), [api, address, system?.pairing_code]);
  const [target, setTarget] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const codeRef = useRef<HTMLInputElement>(null);

  const peers = mesh.data?.peers ?? system?.peers ?? [];
  const paired = (d: DiscoveredDevice) => peers.some((p) => normAddress(p.address) === normAddress(d.address) || p.id === slug(d.id));
  const discovered = mesh.data?.discovered ?? [];

  const choose = (d: DiscoveredDevice) => {
    setTarget(d.address);
    codeRef.current?.focus();
  };
  const pair = async () => {
    const c = code.replace(/\D/g, "");
    if (!target.trim()) return toast("Indica l'indirizzo del dispositivo (host:porta)", "crit");
    if (c.length !== 6) return toast("Il codice di pairing è di 6 cifre", "crit");
    setBusy(true);
    try {
      const p = await api.pairPeer(target.trim(), c);
      toast(`Accoppiato con ${p.name}: i suoi modelli e agenti sono disponibili qui`);
      setTarget("");
      setCode("");
      mesh.reload();
      refreshSystem();
    } catch (e) {
      toast(String((e as Error)?.message ?? e), "crit");
    } finally {
      setBusy(false);
    }
  };
  const remove = async (p: PeerDevice) => {
    try {
      await api.removePeer(p.id);
      toast(`${p.name} rimosso`);
      mesh.reload();
      refreshSystem();
    } catch (e) {
      toast(String((e as Error)?.message ?? e), "crit");
    }
  };

  return (
    <section className="section" style={{ marginTop: 28 }}>
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
                <span className="faint">{address ? (qr.error ?? "Genero il QR…") : "QR disponibile con l'accesso dalla LAN"}</span>
              )}
            </div>
            <div className="stack" style={{ gap: 8, minWidth: 0 }}>
              <span className="eyebrow">Codice di pairing</span>
              <span className="mono pair-code" aria-label="Codice di pairing">{system?.pairing_code ?? "—"}</span>
              <span className="muted" style={{ fontSize: "var(--fs-sm)" }}>
                Inquadra il QR con la fotocamera del telefono: apre l'app già accoppiata. Da un altro PC inserisci l'indirizzo e il codice.
              </span>
              <dl className="kv">
                <dt>Indirizzo</dt><dd className="mono">{address ?? "solo locale"}</dd>
                <dt>Scoperta</dt><dd>{mesh.data?.discovery ? <span className="pill ok">mDNS attivo</span> : <span className="pill">spenta</span>}</dd>
              </dl>
              {!address && (
                <span className="faint" style={{ fontSize: "var(--fs-xs)" }}>
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
            <button className="icon-btn" aria-label="Aggiorna" onClick={mesh.reload}><Icon name="refresh" size={16} /></button>
          </div>
          <div className="list">
            {discovered.map((d) => {
              const done = paired(d);
              return (
                <div className="list-item" key={d.id}>
                  <Icon name="device" />
                  <div className="grow" style={{ minWidth: 0 }}>
                    <b className="ellipsis" style={{ display: "block" }}>{d.name}</b>
                    <div className="faint mono ellipsis" style={{ fontSize: "var(--fs-xs)" }}>{OS_LABEL[d.os] ?? d.os} · {d.tier.toUpperCase()} · {d.address} · {relTime(d.last_seen)}</div>
                  </div>
                  {done ? <span className="pill ok">accoppiato</span> : <button className="btn sm" onClick={() => choose(d)}>Accoppia</button>}
                </div>
              );
            })}
            {discovered.length === 0 && (
              <div className="list-item faint" style={{ fontSize: "var(--fs-sm)" }}>
                {mesh.data?.discovery ? "Nessun altro dispositivo AI Workspace trovato in questa rete." : "La scoperta automatica funziona con l'accesso dalla LAN attivo. Puoi sempre accoppiare per indirizzo."}
              </div>
            )}
          </div>
          <form className="panel-body pair-form" onSubmit={(e) => { e.preventDefault(); void pair(); }}>
            <div className="field grow">
              <label htmlFor="peer-address">Indirizzo</label>
              <input id="peer-address" className="input mono" placeholder="192.168.1.20:7777" value={target} onChange={(e) => setTarget(e.target.value)} autoComplete="off" />
            </div>
            <div className="field">
              <label htmlFor="peer-code">Codice</label>
              <input id="peer-code" ref={codeRef} className="input mono" inputMode="numeric" placeholder="123456" maxLength={7} value={code} onChange={(e) => setCode(e.target.value)} autoComplete="one-time-code" />
            </div>
            <button className="btn primary" type="submit" disabled={busy}>{busy ? "Accoppio…" : "Accoppia"}</button>
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
              <div className="grow" style={{ minWidth: 0 }}>
                <b>{p.name}</b>
                <div className="faint mono ellipsis" style={{ fontSize: "var(--fs-xs)" }}>{OS_LABEL[p.os] ?? p.os} · {p.address}</div>
              </div>
              <div className="chips peer-models">{p.shared_runtimes.slice(0, 4).map((r) => <span className="chip" key={r}>{r}</span>)}{p.shared_runtimes.length > 4 && <span className="chip">+{p.shared_runtimes.length - 4}</span>}</div>
              <StatusPill status={p.status} />
              <button className="icon-btn" aria-label={`Rimuovi ${p.name}`} onClick={() => void remove(p)}><Icon name="trash" size={16} /></button>
            </div>
          ))}
          {peers.length === 0 && <div className="list-item faint">Nessun dispositivo accoppiato.</div>}
        </div>
      </div>
    </section>
  );
}
