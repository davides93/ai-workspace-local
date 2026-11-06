import { useState } from "react";
import { navigate } from "../../../app/router";
import { Button, DecisionDialog, EmptyState, Field, Icon, Panel, Textarea } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { EvalSuite } from "../../../shared/api/types";
import { useDeleteEvalSuite, useSaveEvalSuite } from "../api";
import { blankSuite, parseSuiteText, suiteText } from "../model/evals";
import { RunDialog } from "./RunDialog";

export const NEW_SUITE = "new";

/** Free id for a new suite: `nuova-suite`, `nuova-suite-2`… */
function freeId(suites: EvalSuite[]): string {
  let id = "nuova-suite";
  for (let n = 2; suites.some((s) => s.id === id); n++) id = `nuova-suite-${n}`;
  return id;
}

/** The open suite: its tasks at a glance, the JSON to edit, run and delete. The server has the last word on what is valid. */
export function SuiteDetail({ suiteId, suites }: { suiteId: string; suites: EvalSuite[] }) {
  const isNew = suiteId === NEW_SUITE;
  const saved = isNew ? undefined : suites.find((s) => s.id === suiteId);
  const initial = isNew ? suiteText(blankSuite(freeId(suites))) : saved ? suiteText(saved) : "";
  const [text, setText] = useState(initial);
  const [localError, setLocalError] = useState<string | undefined>();
  const save = useSaveEvalSuite();
  const remove = useDeleteEvalSuite();
  const [running, setRunning] = useState(false);
  const [deleting, setDeleting] = useState(false);

  if (!isNew && !saved) {
    return <EmptyState icon="check" title="Suite non trovata" description="Potrebbe essere stata eliminata da un altro dispositivo." />;
  }
  const dirty = text !== initial;
  const parsed = parseSuiteText(text);
  const shown = parsed.suite ?? saved;
  const submit = () => {
    setLocalError(parsed.error);
    if (!parsed.suite) return;
    save.mutate(parsed.suite, {
      onSuccess: (s) => {
        setText(suiteText(s));
        navigate(`/evals/suites?suite=${encodeURIComponent(s.id)}`, { replace: true });
      },
    });
  };
  const error = localError ?? (save.isError ? errorMessage(save.error) : undefined);

  return (
    <div className="stack stack-lg ev-detail">
      <Panel
        title={saved?.name ?? "Nuova suite"} icon="check"
        actions={
          <>
            {saved && <Button variant="primary" icon="play" onClick={() => setRunning(true)} disabled={dirty} title={dirty ? "Salva le modifiche prima di eseguire" : undefined}>Esegui</Button>}
            {saved && <Button variant="ghost" icon="trash" onClick={() => setDeleting(true)}>Elimina</Button>}
          </>
        }
      >
        {saved?.description && <p className="muted text-sm">{saved.description}</p>}
        <ol className="ev-tasks" aria-label="Task della suite">
          {(shown?.tasks ?? []).map((t) => (
            <li key={t.id}>
              <b className="mono text-sm">{t.id}</b>
              <span className="faint text-xs">{Array.isArray(t.graders) ? t.graders.map((g) => g.kind).join(", ") : ""}</span>
            </li>
          ))}
        </ol>
      </Panel>

      <Panel title="Definizione (JSON)" icon="file">
        <Field
          label="Suite" htmlFor="ev-json" error={error}
          hint="Id, nome, task con prompt, file di partenza e grader (file_exists, file_contains, output_matches, json_schema, judge). Il server controlla agenti e profili."
        >
          <Textarea
            id="ev-json" className="mono ev-json" rows={18} spellCheck={false} value={text}
            onChange={(e) => { setText(e.target.value); setLocalError(undefined); save.reset(); }}
          />
        </Field>
        <p className="faint text-xs row"><Icon name="shield" size={13} /> I grader <code>command</code> eseguono comandi con i tuoi permessi: non si possono scrivere da qui, solo in un file in <code>~/.aiw/evals</code>.</p>
        <div className="row ev-actions">
          <Button variant="primary" onClick={submit} disabled={save.isPending || (!dirty && !isNew)}>{save.isPending ? "Salvataggio…" : isNew ? "Crea suite" : "Salva"}</Button>
          <Button variant="ghost" onClick={() => { setText(initial); setLocalError(undefined); save.reset(); }} disabled={!dirty || save.isPending}>Annulla modifiche</Button>
        </div>
      </Panel>

      {running && saved && <RunDialog suite={saved} onClose={() => setRunning(false)} />}
      {deleting && saved && (
        <DecisionDialog
          title={`Eliminare «${saved.name}»?`} confirmLabel="Elimina" pending={remove.isPending} error={remove.error}
          onCancel={() => { setDeleting(false); remove.reset(); }}
          onConfirm={() => remove.mutate(saved.id, { onSuccess: () => { setDeleting(false); navigate("/evals/suites", { replace: true }); } })}
        >
          La suite sparisce; i report già fatti restano.
        </DecisionDialog>
      )}
    </div>
  );
}
