import { Link, navigate } from "../../../app/router";
import { Button, Icon, Panel } from "../../../shared/ui";
import { formatBytes } from "../../../shared/lib/format";
import { useCatalog, usePullModel } from "../../models/api";
import { useRefreshRuntimes } from "../../runtimes/api";
import { recommendedModel, type ChecklistStep } from "../model/overview";

/** First-run checklist (DESIGN §5.1): runtime → recommended model → profile → first chat. */
export function FirstRun({ steps }: { steps: ChecklistStep[] }) {
  const catalog = useCatalog();
  const refresh = useRefreshRuntimes();
  const pull = usePullModel();
  const rec = catalog.data ? recommendedModel(catalog.data) : undefined;
  const doneCount = steps.filter((s) => s.done).length;

  const action = (s: ChecklistStep) => {
    switch (s.id) {
      case "runtime":
        return (
          <>
            <Button size="sm" icon="refresh" onClick={() => refresh.mutate()} disabled={refresh.isPending}>Rileva runtime</Button>
            <Link to="/runtimes" className="btn sm ghost">Vedi i runtime</Link>
          </>
        );
      case "model":
        return rec ? (
          <Button size="sm" variant="primary" icon="download" disabled={pull.isPending} onClick={() => pull.mutate(rec.id, { onSuccess: () => navigate("/models/downloads") })}>
            Scarica {rec.name} ({formatBytes(rec.size_bytes)})
          </Button>
        ) : (
          <Link to="/models/discover" className="btn sm">Scopri modelli</Link>
        );
      case "profile":
        return <Link to="/agents" className="btn sm">Apri i profili</Link>;
      case "chat":
        return <Link to="/chat" className="btn sm">Apri il Workspace</Link>;
    }
  };

  return (
    <Panel title="Per iniziare" icon="sparkle" className="first-run" actions={<span className="faint text-sm">{doneCount} di {steps.length}</span>}>
      <p className="muted text-sm mb-3">Quattro passi per avere un assistente locale che funziona: ognuno si completa da qui.</p>
      <ol className="checklist">
        {steps.map((s, i) => (
          <li key={s.id} className={s.done ? "done" : ""}>
            <span className="check-mark" aria-hidden="true">{s.done ? <Icon name="check" size={14} /> : i + 1}</span>
            <span className="grow">
              <span className="sr-only">{s.done ? "Completato: " : "Da fare: "}</span>
              {s.label}
            </span>
            {!s.done && <span className="row">{action(s)}</span>}
          </li>
        ))}
      </ol>
    </Panel>
  );
}
