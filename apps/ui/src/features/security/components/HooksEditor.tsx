import { useEffect, useMemo, useState } from "react";
import { Button, EmptyState, Field, Icon, Input, Panel, QueryState, Select, RowControls, SaveBar, UnsavedChangesDialog } from "../../../shared/ui";
import { keyed, keyedOne, moveRow, unkeyed, type Keyed } from "../../../shared/lib/keyedList";
import { useDraft } from "../../../shared/lib/useDraft";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { HookConfig, HookEvent } from "../../../shared/api/types";
import { useHooks, useSetHooks } from "../api";
import { globProblem } from "../model/glob";
import { blankHook, checkHooks, HOOK_EVENTS, hookEvent, normalizeHooks } from "../model/hooks";

/** Shell commands run on lifecycle events; exit code 2 blocks the action. */
export function HooksEditor() {
  const hooks = useHooks();
  const save = useSetHooks();
  const [focusUid, setFocusUid] = useState<number | null>(null);
  const { draft, setDraft, dirty, reset } = useDraft<HookConfig[], Keyed<HookConfig>[]>({
    saved: hooks.data?.hooks, fromSaved: keyed, toSaved: unkeyed, normalize: normalizeHooks,
  });
  const rows = useMemo(() => draft ?? [], [draft]);
  const guard = useUnsavedGuard(dirty, { onDiscard: reset });
  const issues = useMemo(() => checkHooks(unkeyed(rows), globProblem), [rows]);
  const hasError = issues.some((i) => i.level === "error");

  useEffect(() => {
    if (focusUid !== null) document.getElementById(`hook-event-${focusUid}`)?.focus();
  }, [focusUid]);

  const add = () => {
    const row = keyedOne(blankHook());
    setDraft((r) => [...r, row]);
    setFocusUid(row.uid);
  };
  const change = (uid: number, patch: Partial<HookConfig>) => setDraft((r) => r.map((x) => (x.uid === uid ? { ...x, value: { ...x.value, ...patch } } : x)));
  const submit = async (): Promise<boolean> => {
    try {
      await save.mutateAsync(normalizeHooks(unkeyed(rows)));
      return true;
    } catch {
      return false;
    }
  };

  return (
    <QueryState query={hooks}>
      {() => (
        <div className="stack stack-lg">
          <Panel className="caution" title="Gli hook eseguono comandi sul tuo computer" icon="alert">
            <p className="text-sm">
              Ogni hook è un comando di shell avviato con i tuoi privilegi, come se lo lanciassi tu: non passa dalla sandbox né dalle regole di policy.
              Aggiungi solo comandi e script di cui ti fidi.
            </p>
            <ul className="how-list text-sm">
              <li>Il comando riceve l'evento in JSON su stdin. Uscire con codice <code>2</code> blocca l'azione (il messaggio su stderr diventa il motivo).</li>
              <li>Se il comando non parte o supera il timeout viene ignorato e l'azione prosegue: per bloccare serve l'uscita con codice 2.</li>
              <li>Gli hook dello stesso evento girano nell'ordine dell'elenco; il primo che blocca ferma gli altri.</li>
            </ul>
          </Panel>

          {rows.length === 0 ? (
            <EmptyState
              icon="terminal" title="Nessun hook"
              description="Un hook esegue un comando quando succede qualcosa: puoi controllare, modificare o bloccare le azioni degli agenti."
              action={<Button variant="primary" icon="plus" onClick={add}>Aggiungi un hook</Button>}
            />
          ) : (
            <div className="stack">
              <ol className="hook-list" aria-label="Hook">
                {rows.map(({ uid, value: h }, i) => {
                  const info = hookEvent(h.event);
                  const n = i + 1;
                  const mine = issues.filter((x) => x.index === i);
                  return (
                    <li key={uid} className="hook-row">
                      <span className="rule-n" aria-hidden="true">{n}</span>
                      <Field label={`Evento dell'hook ${n}`} htmlFor={`hook-event-${uid}`} className="hook-event">
                        <Select id={`hook-event-${uid}`} value={h.event} onChange={(e) => change(uid, { event: e.target.value as HookEvent })}>
                          {HOOK_EVENTS.map((e) => <option key={e.id} value={e.id}>{e.label}{e.active ? "" : " (non ancora attivo)"}</option>)}
                        </Select>
                      </Field>
                      <Field label={`Filtro sul tool dell'hook ${n}`} htmlFor={`hook-matcher-${uid}`} className="hook-matcher">
                        <Input id={`hook-matcher-${uid}`} mono disabled={!info?.tools} placeholder={info?.tools ? "tutti · shell, write_*" : "non si applica"} value={info?.tools ? h.matcher ?? "" : ""} onChange={(e) => change(uid, { matcher: e.target.value })} />
                      </Field>
                      <Field label={`Timeout in secondi dell'hook ${n}`} htmlFor={`hook-timeout-${uid}`} className="hook-timeout">
                        <Input id={`hook-timeout-${uid}`} className="num" type="number" min={1} max={600} value={Number.isNaN(h.timeout_s) ? "" : h.timeout_s} onChange={(e) => change(uid, { timeout_s: e.target.value === "" ? Number.NaN : Number(e.target.value) })} />
                      </Field>
                      <div className="rule-buttons">
                        <RowControls index={i} count={rows.length} noun={`l'hook ${n}`} onMove={(d) => setDraft((r) => moveRow(r, uid, d))} onRemove={() => setDraft((r) => r.filter((x) => x.uid !== uid))} />
                      </div>
                      <Field label={`Comando dell'hook ${n}`} htmlFor={`hook-command-${uid}`} className="hook-command">
                        <Input id={`hook-command-${uid}`} mono placeholder="./.aiw/hooks/check.sh" value={h.command} onChange={(e) => change(uid, { command: e.target.value })} autoComplete="off" spellCheck={false} />
                      </Field>
                      {info && (
                        <p className="hook-info text-xs faint">
                          <Icon name="info" size={12} /> Riceve <code>{info.stdin}</code>. {info.can}
                        </p>
                      )}
                      {mine.length > 0 && (
                        <ul className="rule-issues">
                          {mine.map((x) => <li key={x.message} className={x.level === "error" ? "field-error" : "row-warn"}>{x.message}</li>)}
                        </ul>
                      )}
                    </li>
                  );
                })}
              </ol>
              <div><Button icon="plus" onClick={add}>Aggiungi hook</Button></div>
            </div>
          )}

          <SaveBar
            dirty={dirty} saving={save.isPending} canSave={!hasError} blocker="Correggi gli hook evidenziati"
            saveLabel="Salva hook"
            onSave={() => void submit()}
            onDiscard={reset}
          />
          <UnsavedChangesDialog guard={guard} subject="gli hook" onSave={submit} saving={save.isPending} canSave={!hasError} />
        </div>
      )}
    </QueryState>
  );
}
