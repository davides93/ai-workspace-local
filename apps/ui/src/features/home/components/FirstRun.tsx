import type { ReactNode } from "react";
import { Link, navigate } from "../../../app/router";
import { defaultAgentId } from "../../../shared/lib/agents";
import { cx } from "../../../shared/lib/cx";
import { formatBytes, formatPercent } from "../../../shared/lib/format";
import { Button, Icon, Meter, Panel } from "../../../shared/ui";
import type { CatalogModel, DownloadProgress, InferenceProfile, LocalModel, RuntimeInfo, SystemInfo } from "../../../shared/api/types";
import { useAgents } from "../../agents/api";
import { useCreateSession } from "../../chat/api";
import { isActiveDownload, useCatalog, useDownloads, usePullModel, useRecommend } from "../../models/api";
import { useRefreshRuntimes } from "../../runtimes/api";
import { useConfig } from "../../settings/api";
import { currentStep, downloadFraction, firstRunModel, type ChecklistId, type ChecklistStep } from "../model/overview";

import { t, t as tr } from "../../../shared/i18n";
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
        actions: live ? <Link to="/runtimes" className="btn sm ghost">{t("home.vedi_i_runtime")}</Link> : (
          <>
            <Button size="sm" variant="primary" icon="refresh" disabled={c.busy.refresh} onClick={c.refresh}>{t("home.rileva_runtime")}</Button>
            <Link to="/runtimes" className="btn sm">{t("home.vedi_i_runtime")}</Link>
          </>
        ),
      };
    }
    case "model": {
      if (done) {
        const n = c.models.length;
        return { detail: `${n} ${n === 1 ? "modello" : "modelli"} sul disco`, actions: <Link to="/models" className="btn sm ghost">{t("home.vedi_i_modelli")}</Link> };
      }
      const { rec, job } = c;
      const where = c.device ? ` per ${c.device.name}` : " per questo dispositivo";
      return {
        detail: job ? (
          <span className="hm-step-progress">
            <span>{t("home.download_di_in_corso", { p: rec?.name ?? job.model_ref, p2: formatPercent(downloadFraction(job.received, job.total)) })}</span>
            <Meter label={t("home.avanzamento_di", { p: rec?.name ?? job.model_ref })} value={job.received} max={job.total} />
          </span>
        ) : rec ? `Consigliato${where}: ${rec.name} (${formatBytes(rec.size_bytes)}). ${rec.fit_detail}` : "Scegli un modello da scaricare: la compatibilità è calcolata sulla memoria e sugli acceleratori del dispositivo.",
        actions: job ? <Link to="/models/downloads" className="btn sm">{t("home.vedi_i_download")}</Link> : (
          <>
            {rec && <Button size="sm" variant="primary" icon="download" disabled={c.busy.pull} onClick={() => c.pull(rec.id)}>{t("home.scarica", { name: rec.name })}</Button>}
            <Link to="/models/discover" className={cx("btn sm", !rec && "primary")}>{rec ? t("home.altri_modelli") : t("home.scopri_modelli")}</Link>
            {rec && <Link to="/models/catalog" className="btn sm ghost">{t("home.catalogo")}</Link>}
          </>
        ),
      };
    }
    case "profile":
      return {
        detail: c.profile ? `${c.profile.name}${c.profile.model ? ` · ${c.profile.model}` : " · ancora senza modello"}` : "Un profilo collega un runtime a un modello e ai suoi parametri.",
        actions: <Link to="/agents" className="btn sm ghost">{t("home.apri_i_profili")}</Link>,
      };
    case "chat":
      return {
        detail: "Una conversazione con un agente: gli strumenti chiedono conferma prima di agire.",
        actions: <Button size="sm" variant="primary" icon="plus" disabled={!c.agentId || c.busy.session} onClick={c.newSession}>{t("home.nuova_sessione")}</Button>,
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
  const recommend = useRecommend();
  const downloads = useDownloads();
  const pull = usePullModel();
  const refresh = useRefreshRuntimes();
  const agents = useAgents();
  const config = useConfig();
  const createSession = useCreateSession();

  const rec = catalog.data && !recommend.isFetching ? firstRunModel(catalog.data, recommend.isError ? null : recommend.data) : undefined;
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
      title={t("home.per_iniziare")} icon="sparkle" className="hm-firstrun"
      actions={
        <>
          <span className="faint text-sm num" aria-label={tr("home.passi_su_completati", { done, length: steps.length })}>{t("home.di", { done, length: steps.length })}</span>
          <Button size="sm" variant="ghost" onClick={onDismiss} title={tr("home.nasconde_questo_elenco")}>{t("home.nascondi")}</Button>
        </>
      }
    >
      <div className="stack stack-lg">
        <Meter label={t("home.passi_completati")} value={done} max={steps.length} tone="ok" />
        <ol className="hm-steps">
          {steps.map((s, i) => {
            const { detail, actions } = stepBody(s.id, ctx, s.done);
            return (
              <li key={s.id} className={cx("hm-step", s.done && "is-done", s.id === current && "is-current")} aria-current={s.id === current ? "step" : undefined}>
                <span className="hm-step-mark" aria-hidden="true">{s.done ? <Icon name="check" size={14} /> : i + 1}</span>
                <div className="hm-step-text">
                  <b><span className="sr-only">{s.done ? tr("home.completato") : t("home.da_fare")}</span>{s.label}</b>
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
