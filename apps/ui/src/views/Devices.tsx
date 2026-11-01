import { useApp } from "../state/app";
import { Icon, Meter, StatusPill, formatMb } from "../components/ui";
import type { DeviceTier } from "../api/types";

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

      <section className="section" style={{ marginTop: 28 }}>
        <div className="section-head">
          <h2>Dispositivi accoppiati</h2>
          <span className="muted">Condividono runtime e modelli in LAN: il telefono usa la GPU del PC, il PC delega i task che servono sul telefono.</span>
        </div>
        <div className="panel">
          <div className="list">
            {system.peers.map((p) => (
              <div className="list-item" key={p.id}>
                <Icon name="device" />
                <div className="grow">
                  <b>{p.name}</b>
                  <div className="faint mono" style={{ fontSize: "var(--fs-xs)" }}>{p.os} · {p.address}</div>
                </div>
                <div className="chips">{p.shared_runtimes.map((r) => <span className="chip" key={r}>{r}</span>)}</div>
                <StatusPill status={p.status} />
              </div>
            ))}
            {system.peers.length === 0 && <div className="list-item faint">Nessun dispositivo accoppiato.</div>}
          </div>
        </div>
        <div className="panel">
          <div className="panel-body row" style={{ alignItems: "flex-start", gap: 16 }}>
            <Icon name="link" size={22} />
            <div className="grow stack" style={{ gap: 6 }}>
              <b>Accoppia un nuovo dispositivo</b>
              <span className="muted" style={{ fontSize: "var(--fs-sm)" }}>
                Attiva "Accesso dalla LAN" nelle impostazioni, poi apri l'app sull'altro dispositivo, scegli "Accoppia" e inserisci il codice.
                Le richieste dalla LAN richiedono sempre il token di pairing.
              </span>
            </div>
            <span className="mono" style={{ fontSize: "1.3rem", letterSpacing: "0.2em" }} aria-label="Codice di pairing">{system.pairing_code ?? "—"}</span>
          </div>
        </div>
      </section>
    </div>
  );
}
