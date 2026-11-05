import { useEffect, useRef } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import type { InstallJob, RuntimeInfo } from "../../../shared/api/types";
import { Button, CopyField, Led, Meter } from "../../../shared/ui";
import { useInstallJob, useInstallPlan, useInstallRuntime } from "../api";
import { installProgress } from "../model/runtimes";

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

  if (plan.isError) return <p className="crit-text text-sm" role="alert">Impossibile leggere il piano di installazione: {errorMessage(plan.error)}</p>;
  if (!p) return <p className="faint text-sm">Preparo il piano di installazione…</p>;

  if (!p.automatic) {
    return (
      <section className="stack" aria-label="Installazione">
        <p className="muted text-sm">{p.runtime_name} si installa a mano: AIW non esegue script scaricati da internet.</p>
        {p.hint && <CopyField label="Installazione" value={p.hint} />}
        {p.homepage && <a href={p.homepage} target="_blank" rel="noreferrer">Vai al sito del progetto</a>}
      </section>
    );
  }

  return (
    <section className="stack" aria-label="Installazione automatica">
      <h3>Installazione automatica</h3>
      <ol className="rt-steps text-sm">
        {p.steps.map((s) => (
          <li key={s.command}><b>{s.title}</b> <code className="mono faint">{s.command}</code></li>
        ))}
      </ol>
      {p.note && <p className="faint text-sm">{p.note}</p>}
      {p.prerequisites.filter((x) => !x.ok).map((x) => (
        <p key={x.name} className="crit-text text-sm" role="alert">Manca: {x.name}. {x.hint}</p>
      ))}
      {!running && j?.state !== "done" && (
        <div className="row">
          <Button variant="primary" icon="download" disabled={!ready || install.isPending} onClick={() => install.mutate(r)}>
            {j?.state === "failed" ? "Riprova l'installazione" : `Installa ${r.name}`}
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
          {job.state === "running" ? `Installazione in corso: passo ${Math.max(job.step, 1)} di ${job.steps}…`
            : job.state === "done" ? "Installazione completata: il runtime è pronto."
            : "Installazione non riuscita."}
        </span>
      </div>
      <Meter value={value} max={max} tone={job.state === "failed" ? "crit" : job.state === "done" ? "ok" : undefined} label="Avanzamento dell'installazione" />
      {job.error && <p className="crit-text text-sm" role="alert">{job.error}</p>}
      {text && <pre ref={box} className="mono logbox" tabIndex={0} role="log" aria-label="Output dell'installazione">{text}</pre>}
    </div>
  );
}

