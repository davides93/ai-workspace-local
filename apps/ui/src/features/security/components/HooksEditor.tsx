import { useEffect, useState } from "react";
import { Button, EmptyState, Field, IconButton, Input, QueryState, Select } from "../../../shared/ui";
import type { HookConfig, HookEvent } from "../../../shared/api/types";
import { useHooks, useSetHooks } from "../api";

const EVENTS: { id: HookEvent; label: string; tools: boolean }[] = [
  { id: "SessionStart", label: "Avvio sessione", tools: false },
  { id: "UserPromptSubmit", label: "Invio del messaggio", tools: false },
  { id: "PreToolUse", label: "Prima di un tool", tools: true },
  { id: "PostToolUse", label: "Dopo un tool", tools: true },
  { id: "Stop", label: "Fine del run", tools: false },
];

const sameHooks = (a: HookConfig[], b: HookConfig[]) => JSON.stringify(a) === JSON.stringify(b);

/** Shell commands run on lifecycle events; exit code 2 blocks the action. */
export function HooksEditor() {
  const hooks = useHooks();
  const save = useSetHooks();
  const [list, setList] = useState<HookConfig[]>([]);
  const [draft, setDraft] = useState<HookConfig>({ event: "PreToolUse", matcher: "", command: "", timeout_s: 30 });
  const saved = hooks.data?.hooks;

  useEffect(() => {
    if (saved) setList(structuredClone(saved));
  }, [saved]);

  const dirty = !!saved && !sameHooks(saved, list);
  const usesMatcher = EVENTS.find((e) => e.id === draft.event)?.tools ?? false;
  const add = () => {
    if (!draft.command.trim()) return;
    setList([...list, { event: draft.event, matcher: usesMatcher && draft.matcher?.trim() ? draft.matcher.trim() : null, command: draft.command.trim(), timeout_s: draft.timeout_s }]);
    setDraft({ ...draft, command: "", matcher: "" });
  };

  return (
    <QueryState query={hooks}>
      {() => (
        <div className="stack stack-lg">
          <div className="table-wrap">
            <table className="data" aria-label="Hook">
              <thead><tr><th>Evento</th><th>Tool (glob)</th><th>Comando</th><th>Timeout</th><th><span className="sr-only">Rimuovi</span></th></tr></thead>
              <tbody>
                {list.map((h, i) => (
                  <tr key={`${h.event}-${h.command}-${i}`}>
                    <td>{EVENTS.find((e) => e.id === h.event)?.label ?? h.event}<div className="mono faint text-xs">{h.event}</div></td>
                    <td className="mono">{h.matcher || <span className="faint">tutti</span>}</td>
                    <td className="mono text-xs cell-detail">{h.command}</td>
                    <td className="num">{h.timeout_s} s</td>
                    <td><IconButton aria-label={`Rimuovi l'hook ${h.command}`} icon="trash" size={16} onClick={() => setList(list.filter((_, j) => j !== i))} /></td>
                  </tr>
                ))}
                {list.length === 0 && <tr><td colSpan={5}><EmptyState icon="terminal" title="Nessun hook" description="Un hook esegue un comando quando succede qualcosa: puoi controllare, modificare o bloccare le azioni degli agenti." /></td></tr>}
              </tbody>
            </table>
          </div>

          <form className="row form-inline" onSubmit={(e) => { e.preventDefault(); add(); }}>
            <Field label="Evento" htmlFor="hook-event">
              <Select id="hook-event" value={draft.event} onChange={(e) => setDraft({ ...draft, event: e.target.value as HookEvent })}>
                {EVENTS.map((e) => <option key={e.id} value={e.id}>{e.label}</option>)}
              </Select>
            </Field>
            {usesMatcher && (
              <Field label="Tool (glob)" htmlFor="hook-matcher">
                <Input id="hook-matcher" mono placeholder="shell" value={draft.matcher ?? ""} onChange={(e) => setDraft({ ...draft, matcher: e.target.value })} />
              </Field>
            )}
            <Field label="Comando" htmlFor="hook-command" className="grow">
              <Input id="hook-command" mono placeholder="./.aiw/hooks/check.sh" value={draft.command} onChange={(e) => setDraft({ ...draft, command: e.target.value })} />
            </Field>
            <Field label="Timeout (s)" htmlFor="hook-timeout">
              <Input id="hook-timeout" className="num input-num" type="number" min={1} max={600} value={draft.timeout_s} onChange={(e) => setDraft({ ...draft, timeout_s: Math.max(1, parseInt(e.target.value || "30", 10)) })} />
            </Field>
            <Button type="submit" icon="plus" disabled={!draft.command.trim()}>Aggiungi</Button>
          </form>

          <div className="row">
            <Button variant="primary" onClick={() => save.mutate(list)} disabled={!dirty || save.isPending}>Salva hook</Button>
            {dirty && <Button variant="ghost" onClick={() => saved && setList(structuredClone(saved))}>Annulla modifiche</Button>}
            {dirty && <span className="faint text-sm">Modifiche non salvate</span>}
          </div>
        </div>
      )}
    </QueryState>
  );
}
