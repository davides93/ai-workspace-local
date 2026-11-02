import type { ReactNode } from "react";
import { Link, navigate } from "../../../app/router";
import { defaultAgentId } from "../../../shared/lib/agents";
import { cx } from "../../../shared/lib/cx";
import { formatBytes, formatPercent } from "../../../shared/lib/format";
import { Button, Icon, Meter, Panel } from "../../../shared/ui";
import type { CatalogModel, DownloadProgress, InferenceProfile, LocalModel, RuntimeInfo, SystemInfo } from "../../../shared/api/types";
import { useAgents } from "../../agents/api";
import { useCreateSession } from "../../chat/api";
import { isActiveDownload, useCatalog, useDownloads, usePullModel } from "../../models/api";
import { useRefreshRuntimes } from "../../runtimes/api";
import { useConfig } from "../../settings/api";
import { currentStep, downloadFraction, recommendedModel, type ChecklistId, type ChecklistStep } from "../model/overview";

interface StepContext {
  runtimes: RuntimeInfo[];
  models: LocalModel[];
  profile?: InferenceProfile;
  device?: SystemInfo["device"];
  rec?: CatalogModel;
  job?: DownloadProgress;
  agentId: string;
  busy: { refresh: boolean; pull: boolean; session: boolean };
  refresh: () => void;
  pull: (id: string) => void;
  newSession: () => void;
}

/** What each step says (once done, or why it is still open) and what to do about it. */
function stepBody(id: ChecklistId, c: StepContext, done: boolean): { detail: ReactNode; actions: ReactNode } {
  switch (id) {
    case "runtime": {
      const live = c.runtimes.find((r) => r.status === "running") ?? c.runtimes.find((r) => r.status === "available");
      return {
        detail: live ? `${live.name}: ${live.status === "running" ? "in esecuzione" : "rilevato, parte al primo uso"}` : "Nessun runtime è pronto: rileva quelli installati o aggiungine uno.",
        actions: live ? <Link to="/runtimes" className="btn sm ghost">Vedi i runtime</Link> : (
          <>
            <Button size="sm" variant="primary" icon="refresh" disabled={c.busy.refresh} onClick={c.refresh}>Rileva runtime</Button>
            <Link to="/runtimes" className="btn sm">Vedi i runtime</Link>
          </>
        ),
      };
    }
    case "model": {
      if (done) {
        const n = c.models.length;
        return { detail: `${n} ${n === 1 ? "modello" : "modelli"} sul disco`, actions: <Link to="/models" className="btn sm ghost">Vedi i modelli</Link> };
      }
      const { rec, job } = c;
      const where = c.device ? ` per ${c.device.name}` : " per questo dispositivo";
      return {
        detail: job ? (
          <span className="hm-step-progress">
            <span>Download di {rec?.name ?? job.model_ref} in corso · {formatPercent(downloadFraction(job.received, job.total))}</span>
            <Meter label={`Avanzamento di ${rec?.name ?? job.model_ref}`} value={job.received} max={job.total} />
          </span>
        ) : rec ? `Consigliato${where}: ${rec.name} (${formatBytes(rec.size_bytes)}). ${rec.fit_detail}` : "Scegli un modello da scaricare: la compatibilità è calcolata sulla memoria e sugli acceleratori del dispositivo.",
        actions: job ? <Link to="/models/downloads" className="btn sm">Vedi i download</Link> : (
          <>
            {rec && <Button size="sm" variant="primary" icon="download" disabled={c.busy.pull} onClick={() => c.pull(rec.id)}>Scarica {rec.name}</Button>}
            <Link to="/models/discover" className={cx("btn sm", !rec && "primary")}>{rec ? "Altri modelli" : "Scopri modelli"}</Link>
            {rec && <Link to="/models/catalog" className="btn sm ghost">Catalogo</Link>}
          </>
        ),
      };
    }
    case "profile":
      return {
        detail: c.profile ? `${c.profile.name}${c.profile.model ? ` · ${c.profile.model}` : " · ancora senza modello"}` : "Un profilo collega un runtime a un modello e ai suoi parametri.",
        actions: <Link to="/agents" className="btn sm ghost">Apri i profili</Link>,
      };
    case "chat":
      return {
        detail: "Una conversazione con un agente: gli strumenti chiedono conferma prima di agire.",
        actions: <Button size="sm" variant="primary" icon="plus" disabled={!c.agentId || c.busy.session} onClick={c.newSession}>Nuova sessione</Button>,
      };
  }
}

/** First-run checklist (DESIGN §5.1): runtime → model recommended for this device → profile → first session. */
export function FirstRun({ steps, runtimes, models, profile, device, onDismiss }: {
  steps: ChecklistStep[];
  runtimes: RuntimeInfo[];
  models: LocalModel[];
  profile?: InferenceProfile;
  device?: SystemInfo["device"];
  onDismiss: () => void;
}) {
  const catalog = useCatalog();
  const downloads = useDownloads();
  const pull = usePullModel();
  const refresh = useRefreshRuntimes();
  const agents = useAgents();
  const config = useConfig();
  const createSession = useCreateSession();

  const rec = catalog.data ? recommendedModel(catalog.data) : undefined;
  const agentId = defaultAgentId(agents.data ?? [], config.data?.effective["agent.default"]);
  const ctx: StepContext = {
    runtimes, models, profile, device, rec, agentId,
    job: rec ? downloads.data?.find((d) => d.model_id === rec.id && isActiveDownload(d)) : undefined,
    busy: { refresh: refresh.isPending, pull: pull.isPending, session: createSession.isPending },
    refresh: () => refresh.mutate(),
    pull: (id) => pull.mutate(id, { onSuccess: () => navigate("/models/downloads") }),
    newSession: () => createSession.mutate({ agent_id: agentId }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) }),
  };

  const done = steps.filter((s) => s.done).length;
  const current = currentStep(steps);
  return (
    <Panel
      title="Per iniziare" icon="sparkle" className="hm-firstrun"
      actions={
        <>
          <span className="faint text-sm num" aria-label={`${done} passi su ${steps.length} completati`}>{done} di {steps.length}</span>
          <Button size="sm" variant="ghost" onClick={onDismiss} title="Nasconde questo elenco">Nascondi</Button>
        </>
      }
    >
      <div className="stack stack-lg">
        <Meter label="Passi completati" value={done} max={steps.length} tone="ok" />
        <ol className="hm-steps">
          {steps.map((s, i) => {
            const { detail, actions } = stepBody(s.id, ctx, s.done);
            return (
              <li key={s.id} className={cx("hm-step", s.done && "is-done", s.id === current && "is-current")} aria-current={s.id === current ? "step" : undefined}>
                <span className="hm-step-mark" aria-hidden="true">{s.done ? <Icon name="check" size={14} /> : i + 1}</span>
                <div className="hm-step-text">
                  <b><span className="sr-only">{s.done ? "Completato: " : "Da fare: "}</span>{s.label}</b>
                  <span className="hm-step-detail">{detail}</span>
                </div>
                <div className="hm-step-actions">{actions}</div>
              </li>
            );
          })}
        </ol>
      </div>
    </Panel>
  );
}
