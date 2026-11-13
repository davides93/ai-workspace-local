import { useState } from "react";
import { navigate } from "../../../app/router";
import { Button, DecisionDialog, EmptyState, Field, Icon, Panel, Textarea } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { EvalSuite } from "../../../shared/api/types";
import { useDeleteEvalSuite, useSaveEvalSuite } from "../api";
import { blankSuite, parseSuiteText, suiteText } from "../model/evals";
import { OptimizeDialog } from "./OptimizeDialog";
import { RunDialog } from "./RunDialog";

import { t as tr, tNode as trNode, t } from "../../../shared/i18n";
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
  const [optimizing, setOptimizing] = useState(false);
  const [deleting, setDeleting] = useState(false);

  if (!isNew && !saved) {
    return <EmptyState icon="check" title={tr("evals.suite_non_trovata")} description={tr("evals.potrebbe_essere_stata_eliminata_da")} />;
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
        title={saved?.name ?? tr("evals.nuova_suite")} icon="check"
        actions={
          <>
            {saved && <Button variant="primary" icon="play" onClick={() => setRunning(true)} disabled={dirty} title={dirty ? t("evals.salva_le_modifiche_prima_di") : undefined}>{tr("evals.esegui_2")}</Button>}
            {saved && <Button icon="sparkle" onClick={() => setOptimizing(true)} disabled={dirty} title={dirty ? t("evals.salva_le_modifiche_prima_di_2") : t("evals.prova_varianti_del_prompt_di")}>{tr("evals.ottimizza_il_prompt")}</Button>}
            {saved && <Button variant="ghost" icon="trash" onClick={() => setDeleting(true)}>{tr("evals.elimina")}</Button>}
          </>
        }
      >
        {saved?.description && <p className="muted text-sm">{saved.description}</p>}
        <ol className="ev-tasks" aria-label={tr("evals.task_della_suite")}>
          {(shown?.tasks ?? []).map((t) => (
            <li key={t.id}>
              <b className="mono text-sm">{t.id}</b>
              <span className="faint text-xs">{Array.isArray(t.graders) ? t.graders.map((g) => g.kind).join(", ") : ""}</span>
            </li>
          ))}
        </ol>
      </Panel>

      <Panel title={tr("evals.definizione_json")} icon="file">
        <Field
          label={tr("evals.suite")} htmlFor="ev-json" error={error}
          hint={tr("evals.id_nome_task_con_prompt")}
        >
          <Textarea
            id="ev-json" className="mono ev-json" rows={18} spellCheck={false} value={text}
            onChange={(e) => { setText(e.target.value); setLocalError(undefined); save.reset(); }}
          />
        </Field>
        <p className="faint text-xs row"><Icon name="shield" size={13} /> {trNode("evals.i_grader_eseguono_comandi_con", { code: <code>{tr("evals.command")}</code>, code2: <code>~/.aiw/evals</code> })}</p>
        <div className="row ev-actions">
          <Button variant="primary" onClick={submit} disabled={save.isPending || (!dirty && !isNew)}>{save.isPending ? tr("evals.salvataggio") : isNew ? tr("evals.crea_suite") : tr("evals.salva")}</Button>
          <Button variant="ghost" onClick={() => { setText(initial); setLocalError(undefined); save.reset(); }} disabled={!dirty || save.isPending}>{tr("evals.annulla_modifiche")}</Button>
        </div>
      </Panel>

      {running && saved && <RunDialog suite={saved} onClose={() => setRunning(false)} />}
      {optimizing && saved && <OptimizeDialog suite={saved} onClose={() => setOptimizing(false)} />}
      {deleting && saved && (
        <DecisionDialog
          title={t("evals.eliminare", { name: saved.name })} confirmLabel={t("evals.elimina")} pending={remove.isPending} error={remove.error}
          onCancel={() => { setDeleting(false); remove.reset(); }}
          onConfirm={() => remove.mutate(saved.id, { onSuccess: () => { setDeleting(false); navigate("/evals/suites", { replace: true }); } })}
        >
          {tr("evals.la_suite_sparisce_i_report")}</DecisionDialog>
      )}
    </div>
  );
}
