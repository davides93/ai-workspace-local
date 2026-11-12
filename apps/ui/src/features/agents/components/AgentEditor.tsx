import { useMemo, useState } from "react";
import { Link } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { Button, EmptyState, Icon, Panel, Pill, QueryState, Skeleton, DecisionDialog, FormSection, FormSections, SaveBar, UnsavedChangesDialog } from "../../../shared/ui";
import { useDraft } from "../../../shared/lib/useDraft";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { AgentProfile } from "../../../shared/api/types";
import { useAgents, useAgentTools, useDeleteAgent, useProfiles, useRuntimeList, useSaveAgent } from "../api";
import { blankAgent, duplicateAgent, hasErrors, idForNew, isBuiltinAgent, NEW_AGENT, normalizeAgent, validateAgent } from "../model/agents";
import { buildCatalog } from "../model/tools";
import { BudgetSection } from "./BudgetSection";
import { KnowledgeSection } from "./KnowledgeSection";
import { IdentitySection } from "./IdentitySection";
import { ModelSection } from "./ModelSection";
import { OutputSection } from "./OutputSection";
import { RolesSection } from "./RolesSection";
import { RoutingSection } from "./RoutingSection";
import { SecuritySection } from "./SecuritySection";
import { ToolPicker } from "./ToolPicker";

/** The detail of `#/agents/:agentId`: a form in sections (identity, model, tools, response, security). */
export function AgentEditor({ agentId, from }: { agentId: string; from?: string }) {
  const agents = useAgents();
  const profiles = useProfiles();
  const tools = useAgentTools();
  const runtimes = useRuntimeList();
  const save = useSaveAgent();
  const [uiMode] = useUiMode();
  const del = useDeleteAgent();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const isNew = agentId === NEW_AGENT;
  const list = useMemo(() => agents.data ?? [], [agents.data]);
  const defaultProfile = profiles.data?.[0]?.id ?? "";

  // What the form starts from: the saved agent, a copy of another one, or a blank agent.
  const saved = useMemo<AgentProfile | undefined>(() => {
    if (!agents.data) return undefined;
    if (!isNew) return list.find((a) => a.id === agentId);
    const source = from ? list.find((a) => a.id === from) : undefined;
    return source ? duplicateAgent(source, list) : blankAgent(defaultProfile);
  }, [agents.data, list, agentId, isNew, from, defaultProfile]);

  const { draft, setDraft, dirty, reset } = useDraft<AgentProfile, AgentProfile>({
    saved, fromSaved: (a) => structuredClone(a), toSaved: (a) => a, normalize: normalizeAgent, scope: `${agentId}|${from ?? ""}`,
  });
  // The response-schema text can be invalid JSON, which the draft never holds: its error blocks saving, and a
  // discard remounts the field so that it shows the saved schema again.
  const [schemaError, setSchemaError] = useState<string | null>(null);
  const [schemaKey, setSchemaKey] = useState(0);
  const discard = () => { reset(); setSchemaError(null); setSchemaKey((k) => k + 1); };
  const guard = useUnsavedGuard(dirty, { onDiscard: discard });
  const catalog = useMemo(() => buildCatalog(tools.data ?? []), [tools.data]);
  const issues = useMemo(
    () => (draft
      ? validateAgent(draft, {
          profileIds: (profiles.data ?? []).map((p) => p.id),
          profilesLoaded: !!profiles.data,
          otherNames: list.filter((a) => a.id !== draft.id).map((a) => a.name),
          knownTools: tools.data ? catalog.map((t) => t.name) : undefined,
        })
      : []),
    [draft, profiles.data, list, tools.data, catalog],
  );

  // A blank new form does not complain about what the user has not had the time to fill in.
  const shown = isNew && !dirty ? [] : issues;

  const submit = async (): Promise<boolean> => {
    if (!draft || hasErrors(issues) || schemaError) return false;
    try {
      const out = await save.mutateAsync(normalizeAgent(isNew ? { ...draft, id: idForNew(draft.name, list) } : draft));
      if (isNew) guard.go(`/agents/${encodeURIComponent(out.id)}`);
      return true;
    } catch {
      return false; // the central error toast already told why
    }
  };
  const remove = () => del.mutate(agentId, { onSuccess: () => { setConfirmDelete(false); guard.go("/agents", { replace: true }); } });

  if (agents.isPending) return <Panel><Skeleton height={220} /></Panel>;
  if (agents.isError && !agents.data) return <QueryState query={agents}>{() => null}</QueryState>;
  if (!draft || !saved) {
    return (
      <Panel>
        <EmptyState icon="agent" title="Agente non trovato" description="Questo agente non esiste (più). Sceglilo dall'elenco o creane uno nuovo."
          action={<Link to="/agents" className="btn">Torna agli agenti</Link>} />
      </Panel>
    );
  }

  const builtin = !isNew && isBuiltinAgent(agentId);
  const change = (patch: Partial<AgentProfile>) => setDraft((d) => ({ ...d, ...patch }));
  return (
    <div className="stack stack-lg">
      <Panel
        title={<>
          <Link to="/agents" className="md-back only-phone" aria-label="Torna agli agenti"><Icon name="back" size={16} /> Agenti</Link>
          <span className="agent-title">{draft.name.trim() || (isNew ? "Nuovo agente" : "Senza nome")}{builtin && <Pill tone="neutral">integrato</Pill>}</span>
        </>}
        actions={!isNew && <div className="detail-actions">
          <Link to={`/agents/${NEW_AGENT}?from=${encodeURIComponent(agentId)}`} className="btn"><Icon name="copy" size={15} /> Duplica</Link>
          <Button variant="danger" icon="trash" disabled={builtin} onClick={() => setConfirmDelete(true)} aria-describedby={builtin ? "builtin-note" : undefined}>Elimina</Button>
        </div>}
      >
        {builtin && <p id="builtin-note" className="faint text-sm mb-3">Gli agenti integrati si possono modificare e duplicare, ma non eliminare.</p>}
        {from && isNew && <p className="faint text-sm mb-3">Copia di un agente esistente: il nuovo agente nasce con le stesse impostazioni.</p>}
        <FormSections>
          <FormSection id="identity" title="Identità" hint="Chi è e come si comporta.">
            <IdentitySection agent={draft} issues={shown} onChange={change} isNew={isNew} />
          </FormSection>
          <FormSection id="model" title="Modello" hint="Quale motore risponde.">
            <ModelSection agent={draft} profiles={profiles.data ?? []} runtimes={runtimes.data ?? []} issues={shown} onChange={change} />
          </FormSection>
          {uiMode === "developer" && (
            <FormSection id="roles" title="Ruoli dei modelli" hint="Più modelli nello stesso run.">
              <RolesSection agent={draft} profiles={profiles.data ?? []} onChange={(next) => setDraft(() => next)} />
            </FormSection>
          )}
          <FormSection id="routing" title="Router" hint="Più profili, scelti da quello che funziona meglio.">
            <RoutingSection agent={draft} savedAgent={isNew ? undefined : saved} profiles={profiles.data ?? []} onChange={change} />
          </FormSection>
          <FormSection id="tools" title="Tool" hint="Cosa può fare oltre a rispondere.">
            <QueryState query={tools}>
              {() => <ToolPicker value={draft.tools} onChange={(t) => change({ tools: t })} catalog={catalog} mode={draft.mode} sandbox={draft.sandbox} />}
            </QueryState>
          </FormSection>
          {uiMode === "developer" && (
            <FormSection id="knowledge" title="Documenti" hint="Le raccolte che può consultare.">
              <KnowledgeSection agent={draft} onChange={(next) => setDraft(() => next)} />
            </FormSection>
          )}
          <FormSection id="output" title="Risposta" hint="Il formato della risposta finale.">
            <OutputSection key={`${agentId}|${from ?? ""}|${schemaKey}`} agent={draft} onChange={change} onError={setSchemaError} />
          </FormSection>
          <FormSection id="budget" title="Budget" hint="Quanto può consumare un run.">
            <BudgetSection agent={draft} issues={shown} onChange={change} />
          </FormSection>
          <FormSection id="security" title="Sicurezza" hint="Che cosa può fare da solo.">
            <SecuritySection agent={draft} issues={shown} onChange={change} />
          </FormSection>
        </FormSections>
      </Panel>

      <SaveBar
        dirty={dirty} isNew={isNew} saving={save.isPending} canSave={!hasErrors(issues) && !schemaError} blocker="Correggi i campi evidenziati"
        saveLabel={isNew ? "Crea agente" : "Salva"} onSave={() => void submit()} onDiscard={discard}
      />

      <UnsavedChangesDialog guard={guard} subject={`l'agente «${draft.name.trim() || "senza nome"}»`} onSave={submit} saving={save.isPending} canSave={!hasErrors(issues) && !schemaError} />
      {confirmDelete && (
        <DecisionDialog
          title="Elimina l'agente" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setConfirmDelete(false); del.reset(); }} onConfirm={remove}
        >
          <p>Vuoi eliminare <b>{saved.name}</b>? Le sessioni già create con questo agente non potranno più proseguire.</p>
          <p className="faint text-sm">Se un workflow lo usa ancora, l'eliminazione viene rifiutata e vedrai quali.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
