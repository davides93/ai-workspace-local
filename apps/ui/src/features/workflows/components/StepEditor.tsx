import { useState, type Dispatch, type SetStateAction } from "react";
import { Button, EmptyState } from "../../../shared/ui";
import { keyedOne, mapKeyed, moveRow, type Keyed } from "../../../shared/lib/keyedList";
import type { WorkflowRun } from "../../../shared/api/types";
import type { Issue } from "../model/draft";
import { changeKind, dropReferences, newStep, renameReferences, STEP_KINDS, stepMeta, type StepDraft, type StepKind } from "../model/steps";
import { StepCard } from "./StepCard";
import type { StepCatalog } from "./StepForms";

import { t } from "../../../shared/i18n";
/** The steps of a workflow as a list of cards: add, remove, reorder and edit each by kind. */
export function StepEditor({ rows, setRows, issues, catalog, inputKeys, lastRun }: {
  rows: Keyed<StepDraft>[];
  setRows: Dispatch<SetStateAction<Keyed<StepDraft>[]>>;
  issues: Issue[];
  catalog: StepCatalog;
  inputKeys: string[];
  lastRun?: WorkflowRun;
}) {
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const steps = rows.map((r) => r.value);
  const defaults = { agent: catalog.agents[0]?.id, team: catalog.teams[0]?.id, profile: catalog.profiles[0]?.id, workflow: catalog.workflows[0]?.id };
  const toggle = (uid: number) => setCollapsed((c) => {
    const n = new Set(c);
    if (!n.delete(uid)) n.add(uid);
    return n;
  });

  const add = (kind: StepKind) => {
    const row = keyedOne(newStep(kind, steps, defaults));
    setRows((r) => [...r, row]);
    // Bring the new card into view once it is rendered.
    requestAnimationFrame(() => document.getElementById(`step-${row.uid}`)?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  };
  const patch = (uid: number, next: StepDraft) => setRows((r) => r.map((x) => (x.uid === uid ? { uid, value: next } : x)));
  const rename = (uid: number, id: string) => setRows((r) => {
    const from = r.find((x) => x.uid === uid)?.value.id;
    if (from === undefined) return r;
    return mapKeyed(r, (all) => renameReferences(all, from, id)).map((x) => (x.uid === uid ? { uid, value: { ...x.value, id } as StepDraft } : x));
  });
  const kind = (uid: number, k: StepKind) => setRows((r) => r.map((x) => (x.uid === uid ? { uid, value: changeKind(x.value, k, r.map((y) => y.value), defaults) } : x)));
  const remove = (uid: number) => setRows((r) => {
    const id = r.find((x) => x.uid === uid)?.value.id;
    return id === undefined ? r : mapKeyed(r, (all) => dropReferences(all, id)).filter((x) => x.uid !== uid);
  });

  return (
    <div className="stack stack-lg">
      {rows.length === 0 ? (
        <EmptyState icon="flow" title={t("workflows.nessun_passo")} description={t("workflows.un_workflow_e_una_sequenza")} />
      ) : (
        <ol className="step-list" aria-label={t("workflows.passi_del_workflow")}>
          {rows.map((r, i) => (
            <StepCard
              key={r.uid} uid={r.uid} index={i} steps={steps} catalog={catalog} inputKeys={inputKeys}
              issues={issues.filter((x) => x.scope === "step" && x.step === i)}
              runStatus={lastRun?.steps.find((s) => s.id === r.value.id)?.status}
              collapsed={collapsed.has(r.uid)} onToggle={() => toggle(r.uid)}
              onChange={(next) => patch(r.uid, next)} onKind={(k) => kind(r.uid, k)} onRename={(id) => rename(r.uid, id)}
              onMove={(d) => setRows((all) => moveRow(all, r.uid, d))} onRemove={() => remove(r.uid)}
            />
          ))}
        </ol>
      )}
      <div className="step-add" role="group" aria-label={t("workflows.aggiungi_un_passo")}>
        <span className="muted text-sm">{t("workflows.aggiungi_un_passo_2")}</span>
        {STEP_KINDS.map((k) => <Button key={k.kind} size="sm" icon={stepMeta(k.kind).icon} title={k.hint} onClick={() => add(k.kind)}>{k.label}</Button>)}
      </div>
    </div>
  );
}
