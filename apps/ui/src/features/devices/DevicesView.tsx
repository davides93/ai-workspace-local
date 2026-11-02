import { formatMb } from "../../shared/lib/format";
import { useSystem } from "../../shared/query/system";
import { Chip, Chips, ErrorState, Icon, KeyValue, Meter, Page, PageHeader, Panel, Pill, Led, SkeletonLines } from "../../shared/ui";
import type { DeviceTier } from "../../shared/api/types";
import { MeshSection } from "./components/MeshSection";

const TIER: Record<DeviceTier, { label: string; models: string }> = {
  xs: { label: "Molto piccolo", models: "Modelli di sistema e fino a 1–2B (Gemma 4 E2B, Qwen3 0.6B)" },
  s: { label: "Piccolo", models: "Fino a circa 4B quantizzati (Gemma 4 E4B, Phi-4 mini)" },
  m: { label: "Medio", models: "7–14B e MoE con pochi parametri attivi" },
  l: { label: "Grande", models: "MoE fino a circa 35B (A3B) e densi fino a 27–32B" },
  xl: { label: "Molto grande", models: "Densi da 70B, MoE da oltre 100B con offload" },
};

export function DevicesView() {
  const q = useSystem();
  const system = q.data;
  if (!system) {
    return (
      <Page>
        {q.isError ? <ErrorState error={q.error} onRetry={() => void q.refetch()} /> : <SkeletonLines lines={4} />}
      </Page>
    );
  }
  const d = system.device;
  const usedPct = 1 - d.available_memory_mb / d.memory_mb;
  return (
    <Page>
      <PageHeader eyebrow="Inferenza" title="Dispositivi" subtitle="Il profilo hardware guida la scelta automatica di runtime e modelli; puoi sempre forzarla." />

      <div className="device-hero">
        <div className="panel">
          <div className="panel-head">
            <Icon name="device" />
            <div className="grow">
              <h2>{d.name}</h2>
              <span className="muted text-sm">{d.os_version} · {d.arch} · {d.form_factor}</span>
            </div>
            <Pill tone="ok"><Led tone="ok" small /> questo device</Pill>
          </div>
          <div className="panel-body stack stack-lg">
            <KeyValue items={[
              ["CPU", `${d.cpu.brand} · ${d.cpu.cores} core / ${d.cpu.threads} thread`],
              ["Istruzioni", <Chips>{d.cpu.features.map((f) => <Chip key={f}>{f}</Chip>)}</Chips>],
              ["Dati", <span className="mono">{system.data_dir}</span>],
              ["Workspace", <span className="mono">{system.workspace_root}</span>],
            ]} />
            <div className="stack stack-sm">
              <div className="row row-between">
                <b className="text-sm">Memoria</b>
                <span className="mono faint num text-xs">{formatMb(d.memory_mb - d.available_memory_mb)} usati di {formatMb(d.memory_mb)}</span>
              </div>
              <Meter label="Memoria in uso" value={d.memory_mb - d.available_memory_mb} max={d.memory_mb} tone={usedPct > 0.85 ? "crit" : usedPct > 0.7 ? "warn" : "ok"} />
            </div>
            <div>
              <b className="text-sm">Acceleratori</b>
              {d.accelerators.map((a, i) => (
                <div className="accel" key={i}>
                  <Chip hot>{a.kind}</Chip>
                  <div className="grow">
                    <div className="ellipsis">{a.name}</div>
                    <div className="faint mono ellipsis text-xs">{[a.vendor, a.driver, a.details].filter(Boolean).join(" · ")}</div>
                  </div>
                  <span className="mono num">{a.memory_mb ? formatMb(a.memory_mb) : ""}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <Panel>
          <div className="stack">
            <span className="eyebrow">Classe del dispositivo</span>
            <div className="row row-baseline">
              <span className="tier">{d.tier.toUpperCase()}</span>
              <b>{TIER[d.tier].label}</b>
            </div>
            <p className="muted">{TIER[d.tier].models}</p>
            <div className="stack stack-sm">
              {(Object.keys(TIER) as DeviceTier[]).map((t) => (
                <div key={t} className="row nowrap">
                  <Chip hot={t === d.tier} className="tier-chip">{t.toUpperCase()}</Chip>
                  <span className={`text-sm ${t === d.tier ? "" : "faint"}`}>{TIER[t].models}</span>
                </div>
              ))}
            </div>
          </div>
        </Panel>
      </div>

      <MeshSection />
    </Page>
  );
}
