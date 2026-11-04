import { useMemo, useState } from "react";
import { Link, parseHash, setQuery, useQueryParams } from "../../../app/router";
import { Button, EmptyState, Icon, Panel, QueryState, Skeleton, TabPanel, Tabs, Toggle, DecisionDialog, SaveBar, UnsavedChangesDialog } from "../../../shared/ui";
import { useDraft } from "../../../shared/lib/useDraft";
import { slugify, uniqueSlug } from "../../../shared/lib/slug";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { WorkflowDef } from "../../../shared/api/types";
import {
  useAgentChoices, useDeleteWorkflow, useProfileChoices, useRunWorkflow, useSaveWorkflow, useToolChoices, useWorkflowRuns, useWorkflows,
} from "../api";
import {
  blankWorkflow, copyOf, defFromPlain, hasErrors, keyDraft, plainFromDef, previewJson, unkeyDraft, validateDraft, type KeyedDraft, type PlainDraft,
} from "../model/draft";
import { AdvancedJson } from "./AdvancedJson";
import { EditorTab } from "./EditorTab";
import { RunDialog } from "./RunDialog";
import { RunDrawer } from "./RunDrawer";
import { RunsPanel } from "./RunsPanel";

type DetailTab = "editor" | "runs" | "advanced";
export const NEW_WORKFLOW = "new";

/** Moving between the tabs, the run drawer or the same workflow keeps the form; anything else leaves it. */
const keepsForm = (from: string, to: string): boolean => {
  const a = parseHash(from);
  const b = parseHash(to);
  return a.path === b.path && a.query.from === b.query.from;
};

/** The enabled switch is saved at once from the list, so the form neither shows nor compares it. */
const withoutEnabled = (p: PlainDraft): PlainDraft => ({ ...p, enabled: false });

/** The detail of `#/workflows/:workflowId`: visual editor, runs and the JSON of the definition. */
export function WorkflowEditor({ workflowId, from }: { workflowId: string; from?: string }) {
  const wfs = useWorkflows();
  const runs = useWorkflowRuns();
  const agents = useAgentChoices();
  const profiles = useProfileChoices();
  const tools = useToolChoices();
  const save = useSaveWorkflow();
  const toggle = useSaveWorkflow({ silent: true });
  const del = useDeleteWorkflow();
  const run = useRunWorkflow();
  const query = useQueryParams();
  const isNew = workflowId === NEW_WORKFLOW;
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [runDialog, setRunDialog] = useState(false);
  const list = useMemo(() => wfs.data ?? [], [wfs.data]);
  const tab: DetailTab = isNew ? (query.tab === "advanced" ? "advanced" : "editor") : query.tab === "runs" || query.tab === "advanced" ? query.tab : "editor";

  const saved = useMemo<WorkflowDef | undefined>(() => {
    if (!wfs.data) return undefined;
    if (!isNew) return list.find((w) => w.id === workflowId);
    const source = from ? list.find((w) => w.id === from) : undefined;
    const id = uniqueSlug(slugify(source ? `copia ${source.name}` : "workflow", "workflow"), list.map((w) => w.id));
    const plain = source ? copyOf(plainFromDef(source), id, list.map((w) => w.name)) : blankWorkflow(id, { agent: agents.data?.[0]?.id });
    return defFromPlain(plain);
  }, [wfs.data, list, workflowId, isNew, from, agents.data]);
  const savedPlain = useMemo(() => (saved ? plainFromDef(saved) : undefined), [saved]);

  const { draft, setDraft, dirty, reset } = useDraft<PlainDraft, KeyedDraft>({
    saved: savedPlain, fromSaved: keyDraft, toSaved: unkeyDraft, normalize: withoutEnabled, scope: `${workflowId}|${from ?? ""}`,
  });
  const guard = useUnsavedGuard(dirty, { onDiscard: reset, keep: keepsForm });
  const plain = useMemo(() => (draft ? unkeyDraft(draft) : null), [draft]);
  // A new workflow takes its id from its name when it is created; the JSON views show that id.
  const liveId = isNew && plain ? uniqueSlug(slugify(plain.name, "workflow"), list.map((w) => w.id)) : plain?.id ?? "";
  const issues = useMemo(
    () => (plain ? validateDraft(plain, { agents: (agents.data ?? []).map((a) => a.id), profiles: (profiles.data ?? []).map((p) => p.id), tools: (tools.data ?? []).map((t) => t.name) }) : []),
    [plain, agents.data, profiles.data, tools.data],
  );
  const catalog = useMemo(() => ({ agents: agents.data ?? [], profiles: profiles.data ?? [], tools: tools.data ?? [] }), [agents.data, profiles.data, tools.data]);

  const secret = saved?.trigger.kind === "webhook" ? saved.trigger.secret ?? "" : "";
  const wfRuns = useMemo(() => (runs.data ?? []).filter((r) => r.workflow_id === workflowId), [runs.data, workflowId]);
  const openRun = wfRuns.find((r) => r.id === query.run);

  // A blank new form does not complain about what the user has not had the time to fill in.
  const shown = isNew && !dirty ? issues.filter((i) => i.level === "warn") : issues;

  const submit = async (): Promise<boolean> => {
    if (!plain || !saved || hasErrors(issues)) return false;
    const id = isNew ? liveId : saved.id;
    try {
      const out = await save.mutateAsync(defFromPlain({ ...plain, id, enabled: isNew ? false : saved.enabled }, secret));
      if (isNew) guard.go(`/workflows/${encodeURIComponent(out.id)}`);
      return true;
    } catch {
      return false; // the central error toast already told why
    }
  };
  const startRun = (inputs: Record<string, unknown>) =>
    run.mutate({ id: workflowId, inputs }, { onSuccess: (r) => { setRunDialog(false); setQuery({ tab: "runs", run: r.id }, { replace: false }); } });

  if (wfs.isPending) return <Panel><Skeleton height={220} /></Panel>;
  if (wfs.isError && !wfs.data) return <QueryState query={wfs}>{() => null}</QueryState>;
  if (!draft || !plain || !saved) {
    return (
      <Panel>
        <EmptyState icon="flow" title="Workflow non trovato" description="Questo workflow non esiste (più). Sceglilo dall'elenco o creane uno nuovo." action={<Link to="/workflows" className="btn">Torna ai workflow</Link>} />
      </Panel>
    );
  }

  const runNow = () => (Object.keys(saved.inputs).length > 0 ? setRunDialog(true) : startRun({}));
  const changeTab = (t: DetailTab) => setQuery({ tab: t === "editor" ? undefined : t, run: undefined });
  return (
    <div className="stack stack-lg">
      <Panel
        title={<>
          <Link to="/workflows" className="md-back only-phone" aria-label="Torna ai workflow"><Icon name="back" size={16} /> Workflow</Link>
          <span className="wf-title">{plain.name.trim() || (isNew ? "Nuovo workflow" : "Senza nome")}</span>
        </>}
        actions={!isNew && <div className="detail-actions">
          <label className="wf-enable check-inline text-sm">
            <Toggle label={saved.enabled ? "Disattiva il workflow" : "Attiva il workflow"} checked={saved.enabled} disabled={toggle.isPending} onChange={(enabled) => toggle.mutate({ ...saved, enabled })} />
            {saved.enabled ? "Attivo" : "Disattivato"}
          </label>
          <Link to={`/workflows/new?from=${encodeURIComponent(saved.id)}`} className="btn"><Icon name="copy" size={15} /> Duplica</Link>
          <Button variant="danger" icon="trash" onClick={() => setConfirmDelete(true)}>Elimina</Button>
          <Button variant="primary" icon="play" onClick={runNow} disabled={dirty || run.isPending} title={dirty ? "Salva le modifiche prima di eseguire" : undefined}>Esegui ora</Button>
        </div>}
      >
        {dirty && !isNew && <p className="faint text-sm mb-3">«Esegui ora» usa la versione salvata: salva le modifiche per provarle.</p>}
        <Tabs
          idPrefix="wfd" label="Sezioni del workflow" value={tab} onChange={changeTab}
          tabs={[{ id: "editor", label: "Editor" }, ...(isNew ? [] : [{ id: "runs" as const, label: "Esecuzioni", count: wfRuns.length }]), { id: "advanced", label: "Avanzato" }]}
        />
        <TabPanel idPrefix="wfd" id={tab}>
          {tab === "editor" && (
            <EditorTab draft={draft} setDraft={setDraft} issues={shown} catalog={catalog} workflowId={isNew ? "" : saved.id} savedSecret={secret} enabled={saved.enabled} lastRun={wfRuns[0]} json={previewJson({ ...plain, id: liveId })} />
          )}
          {tab === "runs" && (
            <RunsPanel runs={wfRuns} selectedId={openRun?.id} onOpen={(id) => setQuery({ run: id }, { replace: false })} onRun={runNow} runDisabled={dirty || run.isPending} />
          )}
          {tab === "advanced" && <AdvancedJson plain={{ ...plain, id: liveId }} workflowId={liveId} onApply={(d) => setDraft(keyDraft({ ...d, id: plain.id }))} />}
        </TabPanel>
      </Panel>

      {tab !== "runs" && (
        <SaveBar
          dirty={dirty} isNew={isNew} saving={save.isPending} canSave={!hasErrors(issues)} blocker="Correggi gli errori indicati nei controlli"
          saveLabel={isNew ? "Crea workflow" : "Salva"} onSave={() => void submit()} onDiscard={reset}
        />
      )}

      <UnsavedChangesDialog guard={guard} subject={`il workflow «${plain.name.trim() || "senza nome"}»`} onSave={submit} saving={save.isPending} canSave={!hasErrors(issues)} />
      {runDialog && <RunDialog workflow={saved} pending={run.isPending} onRun={startRun} onClose={() => setRunDialog(false)} />}
      {openRun && <RunDrawer run={openRun} workflow={saved} onClose={() => setQuery({ run: undefined })} />}
      {confirmDelete && (
        <DecisionDialog
          title="Elimina il workflow" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setConfirmDelete(false); del.reset(); }}
          onConfirm={() => del.mutate(saved.id, { onSuccess: () => { setConfirmDelete(false); guard.go("/workflows", { replace: true }); } })}
        >
          <p>Vuoi eliminare <b>{saved.name}</b>? I suoi trigger smettono di funzionare e l'azione non si può annullare.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
