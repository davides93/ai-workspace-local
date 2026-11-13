import { useMemo, useState } from "react";
import { Button, EmptyState, Icon, QueryState, SaveBar, UnsavedChangesDialog } from "../../../shared/ui";
import { keyed, keyedOne, moveRow, unkeyed, type Keyed } from "../../../shared/lib/keyedList";
import { useDraft } from "../../../shared/lib/useDraft";
import { useUnsavedGuard } from "../../../shared/lib/useUnsavedGuard";
import type { PolicyRule } from "../../../shared/api/types";
import { useAnalyzeCommand, usePolicy, useSetPolicy, useToolList } from "../api";
import { globProblem } from "../model/glob";
import { checkRules, normalizeRules, runProbe, type Probe } from "../model/policy";
import { RuleList, type RuleMark } from "./RuleList";
import { RuleTester } from "./RuleTester";

import { t, tNode } from "../../../shared/i18n";
const NO_MARKS = new Map<number, RuleMark>();
const FIRST_PROBE: Probe = { tool: "", subject: "", mode: "ask", risk: "exec", sessionAllowed: false, sandboxAvailable: true };

/** Rules `tool(pattern) → allow | ask | deny`, evaluated on top of the approval mode of every agent. */
export function PolicyEditor() {
  const policy = usePolicy();
  const tools = useToolList();
  const save = useSetPolicy();
  const [focusUid, setFocusUid] = useState<number | null>(null);
  const [probe, setProbe] = useState<Probe>(FIRST_PROBE);
  const { draft, setDraft, dirty, reset } = useDraft<PolicyRule[], Keyed<PolicyRule>[]>({
    saved: policy.data?.rules, fromSaved: keyed, toSaved: unkeyed, normalize: normalizeRules,
  });
  const rows = useMemo(() => draft ?? [], [draft]);

  const guard = useUnsavedGuard(dirty, { onDiscard: reset });
  const issues = useMemo(() => checkRules(unkeyed(rows), globProblem), [rows]);
  const hasError = issues.some((i) => i.level === "error");
  // A shell command is judged one simple command at a time: the backend says how it splits the line.
  const analysis = useAnalyzeCommand(probe.tool.trim() === "shell" ? probe.subject.trim() : "");
  const analysed = probe.tool.trim() === "shell" && probe.subject.trim() !== "" ? analysis.data : undefined;
  const result = useMemo(() => runProbe(unkeyed(rows), probe, analysed), [rows, probe, analysed]);
  const marks = useMemo(() => {
    if (!result) return NO_MARKS;
    const m = new Map<number, RuleMark>();
    for (const i of result.matches) m.set(i, "match");
    if (result.decision.by === "rule" && result.decision.ruleIndex !== undefined) m.set(result.decision.ruleIndex, "winner");
    return m;
  }, [result]);

  const add = () => {
    const row = keyedOne<PolicyRule>({ tool: "", pattern: "", action: "ask" });
    setDraft((r) => [...r, row]);
    setFocusUid(row.uid);
  };
  const change = (uid: number, patch: Partial<PolicyRule>) => setDraft((r) => r.map((x) => (x.uid === uid ? { ...x, value: { ...x.value, ...patch } } : x)));
  const submit = async (): Promise<boolean> => {
    try {
      await save.mutateAsync(normalizeRules(unkeyed(rows)));
      return true;
    } catch {
      return false; // the central error toast already told why
    }
  };

  return (
    <QueryState query={policy}>
      {() => (
        <div className="stack stack-lg">
          <details className="explainer">
            <summary><Icon name="info" size={16} /> <b>{t("security.come_si_decide")}</b> <span className="muted">{t("security.nega_prevale_su_consenti_che")}</span></summary>
            <ol className="how-list">
              <li>{tNode("security.una_regola_che_combacia_vince", { b: <b>{t("security.nega")}</b> })}</li>
              <li>{tNode("security.altrimenti_vince_una_regola_poi", { b: <b>{t("security.consenti")}</b>, b2: <b>{t("security.chiedi")}</b> })}</li>
              <li>{t("security.senza_regole_che_combaciano_decide")}</li>
            </ol>
            <h4 className="text-sm">{t("security.comandi_concatenati")}</h4>
            <p className="text-sm muted">
              {tNode("security.un_comando_di_shell_viene", { code: <code>;</code>, code2: <code>&amp;&amp;</code>, code3: <code>||</code>, code4: <code>|</code>, code5: <code>&amp;</code>, code6: <code>$(…)</code>, code7: <code>&lt;(…)</code>, code8: <code>{t("security.sh_c")}</code>, code9: <code>{t("security.env_var_comando")}</code>, code10: <code>{t("security.xargs")}</code>, code11: <code>{t("security.find_exec")}</code>, code12: <code>{t("security.sudo")}</code>, b: <b>{t("security.nega")}</b>, b2: <b>{t("security.uno_qualsiasi")}</b>, b3: <b>{t("security.consenti")}</b>, b4: <b>{t("security.ogni")}</b>, code13: <code>{t("security.shell_git")}</code>, code14: <code>{t("security.git_status")}</code>, code15: <code>{t("security.git_status_curl_evil_sh")}</code>, code16: <code>{t("security.eval")}</code>, code17: <code>{t("security.sh_c")}</code>, code18: <code>{t("security.sh_c")}</code>, code19: <code>{t("security.ssh_host_comando")}</code>, code20: <code>{t("security.docker_run")}</code> })}</p>
            <p className="text-sm muted">{t("security.la_precedenza_dipende_dall_azione")}</p>
          </details>

          {rows.length === 0 ? (
            <EmptyState
              icon="shield" title={t("security.nessuna_regola")}
              description={t("security.senza_regole_decide_solo_la")}
              action={<Button variant="primary" icon="plus" onClick={add}>{t("security.aggiungi_una_regola")}</Button>}
            />
          ) : (
            <div className="stack">
              <div className="rule-head" aria-hidden="true"><span /><span>{t("security.tool_glob")}</span><span>{t("security.pattern_glob_facoltativo")}</span><span>{t("security.azione")}</span><span /></div>
              <RuleList rows={rows} issues={issues} marks={marks} focusUid={focusUid}
                onChange={change}
                onMove={(uid, d) => setDraft((r) => moveRow(r, uid, d))}
                onRemove={(uid) => setDraft((r) => r.filter((x) => x.uid !== uid))} />
              <div><Button icon="plus" onClick={add}>{t("security.aggiungi_regola")}</Button></div>
            </div>
          )}

          <RuleTester probe={probe} onProbe={setProbe} result={result} tools={tools.data ?? []} analysis={analysed} />

          <p className="faint text-sm row"><Icon name="info" size={14} /> {" "}{tNode("security.il_pattern_confronta_il_comando", { code: <code>*</code> })}</p>

          <SaveBar
            dirty={dirty} saving={save.isPending} canSave={!hasError} blocker={t("security.correggi_le_regole_evidenziate")}
            saveLabel={t("security.salva_regole")}
            onSave={() => void submit()}
            onDiscard={reset}
          />
          <UnsavedChangesDialog guard={guard} subject={t("security.le_regole_di_policy")} onSave={submit} saving={save.isPending} canSave={!hasError} />
        </div>
      )}
    </QueryState>
  );
}
