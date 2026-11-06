import type { UseQueryResult } from "@tanstack/react-query";
import { Button, Panel, Pill, QueryState } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import { formatMb } from "../../../shared/lib/format";
import type { TrainDoctor, TrainerInfo } from "../../../shared/api/types";
import { useInstallTrainer } from "../api";
import { METHOD_LABEL } from "../model/training";

const STATE_LABEL: Record<TrainerInfo["state"], string> = { ready: "Pronto", missing: "Da installare", unsupported: "Non supportato" };

function Trainer({ t }: { t: TrainerInfo }) {
  const install = useInstallTrainer();
  return (
    <li className="tr-trainer">
      <div className="tr-trainer-head">
        <b>{t.name}</b>
        <Pill tone={t.state === "ready" ? "ok" : t.state === "missing" ? "warn" : "neutral"}>{STATE_LABEL[t.state]}</Pill>
      </div>
      <span className="muted text-sm">{t.description}</span>
      {t.reason && <span className="faint text-sm">{t.reason}</span>}
      <ul className="tr-methods">
        {t.methods.map((m) => (
          <li key={m.method}>
            <b>{METHOD_LABEL[m.method]}</b>{" "}
            {m.blocked ? <span className="faint">no: {m.blocked}</span> : m.max_params_b ? <span className="muted">fino a ~{m.max_params_b}B</span> : null}
          </li>
        ))}
      </ul>
      {t.state === "missing" && (
        t.install_auto
          ? <Button size="sm" icon="download" onClick={() => install.mutate(t.id)} disabled={install.isPending}>{install.isPending ? "Installazione…" : "Installa"}</Button>
          : <span className="faint text-sm">{t.install_hint}</span>
      )}
      {install.isError && <p className="tr-error" role="alert">{errorMessage(install.error)}</p>}
    </li>
  );
}

/** The device, the trainers it can run and what to do next (`aiw train doctor`). */
export function DoctorPanel({ doctor }: { doctor: UseQueryResult<TrainDoctor> }) {
  return (
    <QueryState query={doctor}>
      {(d) => (
        <Panel title="Questo dispositivo" icon="chip">
          <div className="stack">
            <p className="text-sm">
              <b>{d.device.name}</b> · {d.device.os} {d.device.arch} · classe {d.device.tier.toUpperCase()} · {formatMb(d.device.memory_mb)}
              {d.device.vram_mb > 0 && !d.device.unified_memory ? ` · VRAM ${formatMb(d.device.vram_mb)}` : ""}
              {d.device.unified_memory ? " (memoria unificata)" : ""} · {d.device.accelerators.join(", ")}
            </p>
            {d.can_train.length > 0
              ? <ul className="tr-advice">{d.can_train.map((c) => <li key={c}>{c}</li>)}</ul>
              : <p className="muted text-sm">Qui non si può addestrare.</p>}
            {d.remote.length > 0 && <ul className="tr-advice">{d.remote.map((c) => <li key={c}>{c}</li>)}</ul>}
            {d.advice.map((a) => <p key={a} className="muted text-sm">{a}</p>)}
            <ul className="tr-trainers">{d.trainers.map((t) => <Trainer key={t.id} t={t} />)}</ul>
          </div>
        </Panel>
      )}
    </QueryState>
  );
}
