import { useMemo, useState } from "react";
import { Link } from "../../../app/router";
import { Button, CopyButton, DecisionDialog, EmptyState, Field, FormSection, FormSections, Icon, Input, Panel, QueryState, SaveBar, Skeleton, Textarea, UnsavedChangesDialog } from "../../../shared/ui";
import { useDraft } from "../../../shared/lib/useDraft";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { TeamDef } from "../../../shared/api/types";
import { BudgetSection } from "../../agents/components/BudgetSection";
import { OutputSection } from "../../agents/components/OutputSection";
import { useAgentOptions, useDeleteTeam, useSaveTeam, useTeams } from "../api";
import { blankTeam, duplicateTeam, hasErrors, idForNew, NEW_TEAM, normalizeTeam, validateTeam } from "../model/teams";
import { MembersSection } from "./MembersSection";

/** The detail of `#/teams/:teamId`: a form in sections (identity, members, response, budget). */
export function TeamEditor({ teamId, from }: { teamId: string; from?: string }) {
  const teams = useTeams();
  const { options, loaded } = useAgentOptions();
  const save = useSaveTeam();
  const del = useDeleteTeam();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const isNew = teamId === NEW_TEAM;
  const list = useMemo(() => teams.data ?? [], [teams.data]);

  // What the form starts from: the saved team, a copy of another one, or a blank team.
  const saved = useMemo<TeamDef | undefined>(() => {
    if (!teams.data) return undefined;
    if (!isNew) return list.find((t) => t.id === teamId);
    const source = from ? list.find((t) => t.id === from) : undefined;
    return source ? duplicateTeam(source, list) : blankTeam();
  }, [teams.data, list, teamId, isNew, from]);

  const { draft, setDraft, dirty, reset } = useDraft<TeamDef, TeamDef>({
    saved, fromSaved: (t) => structuredClone(t), toSaved: (t) => t, normalize: normalizeTeam, scope: `${teamId}|${from ?? ""}`,
  });
  // Same as the agent form: invalid schema text blocks saving, and a discard remounts the field.
  const [schemaError, setSchemaError] = useState<string | null>(null);
  const [schemaKey, setSchemaKey] = useState(0);
  const discard = () => { reset(); setSchemaError(null); setSchemaKey((k) => k + 1); };
  const guard = useUnsavedGuard(dirty, { onDiscard: discard });
  const issues = useMemo(
    () => (draft ? validateTeam(draft, { agentIds: options.filter((o) => !o.remote).map((o) => o.id), agentsLoaded: loaded, otherNames: list.filter((t) => t.id !== draft.id).map((t) => t.name) }) : []),
    [draft, options, loaded, list],
  );
  // A blank new form does not complain about what the user has not had the time to fill in.
  const shown = isNew && !dirty ? [] : issues;

  const submit = async (): Promise<boolean> => {
    if (!draft || hasErrors(issues) || schemaError) return false;
    try {
      const out = await save.mutateAsync(normalizeTeam(isNew ? { ...draft, id: idForNew(draft.name, list) } : draft));
      if (isNew) guard.go(`/teams/${encodeURIComponent(out.id)}`);
      return true;
    } catch {
      return false; // the central error toast already told why
    }
  };
  const remove = () => del.mutate(teamId, { onSuccess: () => { setConfirmDelete(false); guard.go("/teams", { replace: true }); } });

  if (teams.isPending) return <Panel><Skeleton height={220} /></Panel>;
  if (teams.isError && !teams.data) return <QueryState query={teams}>{() => null}</QueryState>;
  if (!draft || !saved) {
    return (
      <Panel>
        <EmptyState icon="team" title="Team non trovato" description="Questo team non esiste (più). Sceglilo dall'elenco o creane uno nuovo." action={<Link to="/teams" className="btn">Torna ai team</Link>} />
      </Panel>
    );
  }

  const change = (patch: Partial<TeamDef>) => setDraft((d) => ({ ...d, ...patch }));
  const nameError = shown.find((i) => i.field === "name" && i.level === "error")?.message;
  const nameWarn = shown.find((i) => i.field === "name" && i.level === "warn")?.message;
  return (
    <div className="stack stack-lg">
      <Panel
        title={<>
          <Link to="/teams" className="md-back only-phone" aria-label="Torna ai team"><Icon name="back" size={16} /> Team</Link>
          <span className="agent-title">{draft.name.trim() || (isNew ? "Nuovo team" : "Senza nome")}</span>
        </>}
        actions={!isNew && <div className="detail-actions">
          <Link to={`/teams/${NEW_TEAM}?from=${encodeURIComponent(teamId)}`} className="btn"><Icon name="copy" size={15} /> Duplica</Link>
          <Button variant="danger" icon="trash" onClick={() => setConfirmDelete(true)}>Elimina</Button>
        </div>}
      >
        {from && isNew && <p className="faint text-sm mb-3">Copia di un team esistente: il nuovo team nasce con le stesse impostazioni.</p>}
        <FormSections>
          <FormSection id="identity" title="Identità" hint="Come si chiama e a che cosa serve.">
            <div className="form-grid">
              <div className="cols-2">
                <Field label="Nome" htmlFor="tm-name" error={nameError} hint={nameWarn}>
                  <Input id="tm-name" value={draft.name} onChange={(e) => change({ name: e.target.value })} placeholder="Revisione del codice" aria-invalid={!!nameError} autoComplete="off" />
                </Field>
                <Field label="Identificativo" htmlFor="tm-id" hint={isNew ? "Si ricava dal nome al primo salvataggio." : "Lo usano le sessioni (team:…): non cambia."}>
                  <div className="copyfield-row">
                    <Input id="tm-id" mono readOnly value={draft.id || "—"} />
                    {draft.id && <CopyButton text={draft.id} />}
                  </div>
                </Field>
              </div>
              <Field label="Descrizione" htmlFor="tm-desc" hint="Compare nell'elenco e, per un supervisore, nelle istruzioni del team.">
                <Textarea id="tm-desc" rows={3} value={draft.description} onChange={(e) => change({ description: e.target.value })} placeholder="Che cosa fa il team, in una o due righe" />
              </Field>
            </div>
          </FormSection>
          <FormSection id="members" title="Composizione" hint="Strategia, membri e chi li coordina.">
            <MembersSection team={draft} options={options} issues={shown} onChange={change} />
          </FormSection>
          <FormSection id="output" title="Risposta" hint="Il formato della risposta finale del team.">
            <OutputSection key={`${teamId}|${from ?? ""}|${schemaKey}`} agent={draft} onChange={change} onError={setSchemaError} />
          </FormSection>
          <FormSection id="budget" title="Budget" hint="Quanto può consumare un run del team.">
            <BudgetSection agent={draft} issues={shown} onChange={change} />
          </FormSection>
        </FormSections>
      </Panel>

      <SaveBar
        dirty={dirty} isNew={isNew} saving={save.isPending} canSave={!hasErrors(issues) && !schemaError} blocker="Correggi i campi evidenziati"
        saveLabel={isNew ? "Crea team" : "Salva"} onSave={() => void submit()} onDiscard={discard}
      />

      <UnsavedChangesDialog guard={guard} subject={`il team «${draft.name.trim() || "senza nome"}»`} onSave={submit} saving={save.isPending} canSave={!hasErrors(issues) && !schemaError} />
      {confirmDelete && (
        <DecisionDialog
          title="Elimina il team" confirmLabel="Elimina" icon="trash" pending={del.isPending} error={del.error}
          onCancel={() => { setConfirmDelete(false); del.reset(); }} onConfirm={remove}
        >
          <p>Vuoi eliminare <b>{saved.name}</b>? Gli agenti che ne fanno parte restano; le sessioni create con il team non potranno più proseguire.</p>
        </DecisionDialog>
      )}
    </div>
  );
}
