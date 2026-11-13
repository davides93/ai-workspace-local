import { useEffect, useMemo, useState } from "react";
import { Button, EmptyState, Field, Icon, Input, Panel, QueryState, Select, RowControls, SaveBar, UnsavedChangesDialog } from "../../../shared/ui";
import { keyed, keyedOne, moveRow, unkeyed, type Keyed } from "../../../shared/lib/keyedList";
import { useDraft } from "../../../shared/lib/useDraft";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { HookConfig, HookEvent } from "../../../shared/api/types";
import { useHooks, useSetHooks } from "../api";
import { globProblem } from "../model/glob";
import { blankHook, checkHooks, HOOK_EVENTS, hookEvent, normalizeHooks } from "../model/hooks";

import { t, tNode } from "../../../shared/i18n";
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
          <Panel className="caution" title={t("security.gli_hook_eseguono_comandi_sul")} icon="alert">
            <p className="text-sm">
              {t("security.ogni_hook_e_un_comando")}</p>
            <ul className="how-list text-sm">
              <li>{tNode("security.il_comando_riceve_l_evento", { code: <code>2</code> })}</li>
              <li>{t("security.se_il_comando_non_parte")}</li>
              <li>{t("security.gli_hook_dello_stesso_evento")}</li>
            </ul>
          </Panel>

          {rows.length === 0 ? (
            <EmptyState
              icon="terminal" title={t("security.nessun_hook")}
              description={t("security.un_hook_esegue_un_comando")}
              action={<Button variant="primary" icon="plus" onClick={add}>{t("security.aggiungi_un_hook")}</Button>}
            />
          ) : (
            <div className="stack">
              <ol className="hook-list" aria-label={t("security.hook")}>
                {rows.map(({ uid, value: h }, i) => {
                  const info = hookEvent(h.event);
                  const n = i + 1;
                  const mine = issues.filter((x) => x.index === i);
                  return (
                    <li key={uid} className="hook-row">
                      <span className="rule-n" aria-hidden="true">{n}</span>
                      <Field label={t("security.evento_dell_hook", { n })} htmlFor={`hook-event-${uid}`} className="hook-event">
                        <Select id={`hook-event-${uid}`} value={h.event} onChange={(e) => change(uid, { event: e.target.value as HookEvent })}>
                          {HOOK_EVENTS.map((e) => <option key={e.id} value={e.id}>{e.label}{e.active ? "" : t("security.non_ancora_attivo")}</option>)}
                        </Select>
                      </Field>
                      <Field label={t("security.filtro_sul_tool_dell_hook", { n })} htmlFor={`hook-matcher-${uid}`} className="hook-matcher">
                        <Input id={`hook-matcher-${uid}`} mono disabled={!info?.tools} placeholder={info?.tools ? t("security.tutti_shell_write") : t("security.non_si_applica")} value={info?.tools ? h.matcher ?? "" : ""} onChange={(e) => change(uid, { matcher: e.target.value })} />
                      </Field>
                      <Field label={t("security.timeout_in_secondi_dell_hook", { n })} htmlFor={`hook-timeout-${uid}`} className="hook-timeout">
                        <Input id={`hook-timeout-${uid}`} className="num" type="number" min={1} max={600} value={Number.isNaN(h.timeout_s) ? "" : h.timeout_s} onChange={(e) => change(uid, { timeout_s: e.target.value === "" ? Number.NaN : Number(e.target.value) })} />
                      </Field>
                      <div className="rule-buttons">
                        <RowControls index={i} count={rows.length} noun={t("security.l_hook", { n })} onMove={(d) => setDraft((r) => moveRow(r, uid, d))} onRemove={() => setDraft((r) => r.filter((x) => x.uid !== uid))} />
                      </div>
                      <Field label={t("security.comando_dell_hook", { n })} htmlFor={`hook-command-${uid}`} className="hook-command">
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
              <div><Button icon="plus" onClick={add}>{t("security.aggiungi_hook")}</Button></div>
            </div>
          )}

          <SaveBar
            dirty={dirty} saving={save.isPending} canSave={!hasError} blocker={t("security.correggi_gli_hook_evidenziati")}
            saveLabel={t("security.salva_hook")}
            onSave={() => void submit()}
            onDiscard={reset}
          />
          <UnsavedChangesDialog guard={guard} subject={t("security.gli_hook")} onSave={submit} saving={save.isPending} canSave={!hasError} />
        </div>
      )}
    </QueryState>
  );
}
