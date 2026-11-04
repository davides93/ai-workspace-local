import type { Dispatch, SetStateAction } from "react";
import { Field, Input, Textarea, FormSection, FormSections } from "../../../shared/ui";
import type { WorkflowRun } from "../../../shared/api/types";
import type { Issue, KeyedDraft } from "../model/draft";
import { InputsEditor } from "./InputsEditor";
import { JsonPreview, ValidationList } from "./Preview";
import { StepEditor } from "./StepEditor";
import type { StepCatalog } from "./StepForms";
import { TriggerEditor } from "./TriggerEditor";

/** The visual editor: general data, trigger, inputs and steps, with the checks and the live JSON below. */
export function EditorTab({ draft, setDraft, issues, catalog, workflowId, savedSecret, enabled, lastRun, json }: {
  draft: KeyedDraft;
  setDraft: Dispatch<SetStateAction<KeyedDraft>>;
  issues: Issue[];
  catalog: StepCatalog;
  /** Empty while the workflow has not been saved. */
  workflowId: string;
  savedSecret: string;
  enabled: boolean;
  lastRun?: WorkflowRun;
  json: string;
}) {
  const nameIssue = issues.find((i) => i.scope === "meta" && i.field === "name");
  const set = <K extends keyof KeyedDraft>(k: K, v: KeyedDraft[K]) => setDraft((d) => ({ ...d, [k]: v }));
  const stepUids = draft.steps.map((s) => s.uid);
  return (
    <FormSections>
      <FormSection id="general" title="Generale">
        <div className="form-grid">
          <Field label="Nome" htmlFor="wf-name" error={nameIssue?.message}>
            <Input id="wf-name" value={draft.name} onChange={(e) => set("name", e.target.value)} placeholder="Digest mattutino dei download" autoComplete="off" aria-invalid={!!nameIssue} />
          </Field>
          <Field label="Descrizione" htmlFor="wf-desc" hint="Facoltativa: ricorda a cosa serve.">
            <Textarea id="wf-desc" rows={2} className="plain" value={draft.description} onChange={(e) => set("description", e.target.value)} />
          </Field>
        </div>
      </FormSection>

      <FormSection id="trigger" title="Trigger" hint="Quando parte da solo.">
        <TriggerEditor
          trigger={draft.trigger} onChange={(t) => set("trigger", t)} issues={issues}
          workflowId={workflowId} savedSecret={savedSecret} enabled={enabled} lastRun={lastRun}
        />
      </FormSection>

      <FormSection id="inputs" title="Input" hint="Valori predefiniti, citabili con {{inputs.nome}}.">
        <InputsEditor rows={draft.inputs} setRows={(u) => setDraft((d) => ({ ...d, inputs: typeof u === "function" ? u(d.inputs) : u }))} issues={issues} />
      </FormSection>

      <FormSection id="steps" title="Passi" hint="Si eseguono nell'ordine dell'elenco.">
        <StepEditor
          rows={draft.steps} setRows={(u) => setDraft((d) => ({ ...d, steps: typeof u === "function" ? u(d.steps) : u }))}
          issues={issues} catalog={catalog} inputKeys={draft.inputs.map((r) => r.value.key.trim()).filter(Boolean)} lastRun={lastRun}
        />
        {issues.some((i) => i.scope === "steps") && <p className="row-warn">{issues.find((i) => i.scope === "steps")?.message}</p>}
      </FormSection>

      <FormSection id="check" title="Controlli e anteprima" hint="Aggiornati mentre scrivi.">
        <ValidationList issues={issues} stepUids={stepUids} onGo={(uid) => document.getElementById(`step-${uid}`)?.scrollIntoView({ block: "center", behavior: "smooth" })} />
        <JsonPreview json={json} />
      </FormSection>
    </FormSections>
  );
}
