import { useEffect, useRef } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import type { InstallJob, RuntimeInfo } from "../../../shared/api/types";
import { Button, CopyField, Led, Meter } from "../../../shared/ui";
import { useInstallJob, useInstallPlan, useInstallRuntime } from "../api";
import { installProgress } from "../model/runtimes";

import { t } from "../../../shared/i18n";
/** Installation of a runtime from inside the app: what will be run, one button, live progress.
 *  Runtimes without a safe recipe (installers that are scripts from the internet) only show
 *  their manual instructions. */
export function RuntimeInstall({ runtime: r }: { runtime: RuntimeInfo }) {
  const plan = useInstallPlan(r.id);
  const job = useInstallJob(r.id);
  const install = useInstallRuntime();
  const p = plan.data;
  const j = job.data ?? null;
  const running = j?.state === "running" || install.isPending;
  const ready = !!p?.automatic && p.prerequisites.every((x) => x.ok);

  if (r.status !== "installable" && !j && !install.isPending && !install.isError) return null;
  if (r.status !== "installable" && j && j.state === "done") return <InstallProgress job={j} />;
  if (plan.isError) return <p className="crit-text text-sm" role="alert">{t("runtimes.impossibile_leggere_il_piano_di", { error: errorMessage(plan.error) })}</p>;
  if (!p) return <p className="faint text-sm">{t("runtimes.preparo_il_piano_di_installazione")}</p>;

  if (!p.automatic) {
    return (
      <section className="stack" aria-label={t("runtimes.installazione")}>
        <p className="muted text-sm">{t("runtimes.si_installa_a_mano_aiw", { runtime_name: p.runtime_name })}</p>
        {p.hint && <CopyField label={t("runtimes.installazione")} value={p.hint} />}
        {p.homepage && <a href={p.homepage} target="_blank" rel="noreferrer">{t("runtimes.vai_al_sito_del_progetto")}</a>}
      </section>
    );
  }

  return (
    <section className="stack" aria-label={t("runtimes.installazione_automatica")}>
      <h3>{t("runtimes.installazione_automatica")}</h3>
      <details>
      <summary>{t("runtimes.passaggi_di_installazione")}</summary>
      <ol className="rt-steps text-sm">
        {p.steps.map((s) => (
          <li key={s.command}><b>{s.title}</b> <code className="mono faint">{s.command}</code></li>
        ))}
      </ol>
      </details>
      {p.note && <p className="faint text-sm">{p.note}</p>}
      {p.prerequisites.filter((x) => !x.ok).map((x) => (
        <p key={x.name} className="crit-text text-sm" role="alert">{t("runtimes.manca", { name: x.name, hint: x.hint ?? "" })}</p>
      ))}
      {!running && j?.state !== "done" && r.status === "installable" && (
        <div className="row">
          <Button variant="primary" icon="download" disabled={!ready || install.isPending} onClick={() => install.mutate(r)}>
            {j?.state === "failed" ? t("runtimes.riprova_l_installazione") : r.id === "localai" ? t("runtimes.installa_e_avvia_localai") : t("runtimes.installa_2", { name: r.name })}
          </Button>
        </div>
      )}
      {install.isError && <p className="crit-text text-sm" role="alert">{errorMessage(install.error)}</p>}
      {j && <InstallProgress job={j} />}
    </section>
  );
}

/** Progress bar and output of an installation. */
export function InstallProgress({ job }: { job: InstallJob }) {
  const box = useRef<HTMLPreElement>(null);
  const text = job.log.slice(-200).join("\n");
  useEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text]);
  const { value, max } = installProgress(job);
  return (
    <div className="stack" aria-live="polite">
      <div className="row">
        <Led tone={job.state === "failed" ? "crit" : job.state === "done" ? "ok" : "run"} small />
        <span className="text-sm">
          {job.state === "running" ? t("runtimes.installazione_in_corso_passo_di", { step: Math.max(job.step, 1), steps: job.steps }) : job.state === "done" ? t("runtimes.installazione_completata_il_runtime_e") : t("runtimes.installazione_non_riuscita")}
        </span>
      </div>
      <Meter value={value} max={max} tone={job.state === "failed" ? "crit" : job.state === "done" ? "ok" : undefined} label={t("runtimes.avanzamento_dell_installazione")} />
      {job.error && <p className="crit-text text-sm" role="alert">{job.error}</p>}
      {text && <details>
        <summary>{t("runtimes.log_di_installazione")}</summary>
        <pre ref={box} className="mono logbox" tabIndex={0} role="log" aria-label={t("runtimes.output_dell_installazione")}>{text}</pre>
      </details>}
    </div>
  );
}

