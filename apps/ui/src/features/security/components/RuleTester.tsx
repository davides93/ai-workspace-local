import { Field, Input, Panel, Pill, Select, type PillTone } from "../../../shared/ui";
import { APPROVAL_MODES, RISK_META } from "../../../shared/lib/policyModes";
import type { CommandAnalysis, PolicyMode, ToolInfo, ToolRisk } from "../../../shared/api/types";
import { DECISION_LABEL, type DecisionKind, type Probe, type ProbeResult } from "../model/policy";

import { t as tr, tNode as trNode } from "../../../shared/i18n";
const TONE: Record<DecisionKind, PillTone> = { allow: "ok", ask: "warn", deny: "crit" };
const RISKS = Object.keys(RISK_META) as ToolRisk[];

/** Asks "what would happen if an agent did this?" of the rules being edited, before they are saved. */
export function RuleTester({ probe, onProbe, result, tools, analysis }: {
  probe: Probe;
  onProbe: (next: Probe) => void;
  result: ProbeResult | null;
  tools: ToolInfo[];
  /** How the backend splits a shell command line (only for the shell tool). */
  analysis?: CommandAnalysis;
}) {
  const set = (patch: Partial<Probe>) => onProbe({ ...probe, ...patch });
  const known = tools.find((t) => t.name === probe.tool.trim());
  const pickTool = (tool: string) => {
    const t = tools.find((x) => x.name === tool.trim());
    set(t ? { tool, risk: t.risk } : { tool });
  };
  const needsSandbox = probe.mode === "auto" && probe.risk === "exec";

  return (
    <Panel title={tr("security.prova_una_chiamata")} icon="search">
      <p className="muted text-sm mb-3">{tr("security.indica_cosa_farebbe_un_agente")}</p>
      <div className="tester-grid">
        <Field label={tr("security.tool")} htmlFor="probe-tool">
          <Input id="probe-tool" mono list="probe-tools" placeholder={tr("security.shell")} value={probe.tool} onChange={(e) => pickTool(e.target.value)} autoComplete="off" />
          <datalist id="probe-tools">{tools.map((t) => <option key={t.name} value={t.name} />)}</datalist>
        </Field>
        <Field label={tr("security.soggetto")} htmlFor="probe-subject" hint={tr("security.comando_percorso_o_url_della")}>
          <Input id="probe-subject" mono placeholder={tr("security.git_status_short")} value={probe.subject} onChange={(e) => set({ subject: e.target.value })} autoComplete="off" />
        </Field>
        <Field label={tr("security.modalita_dell_agente")} htmlFor="probe-mode">
          <Select id="probe-mode" value={probe.mode} onChange={(e) => set({ mode: e.target.value as PolicyMode })}>
            {APPROVAL_MODES.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
          </Select>
        </Field>
        <Field label={tr("security.rischio_del_tool")} htmlFor="probe-risk" hint={known ? tr("security.preso_dal_catalogo_dei_tool") : tr("security.tool_sconosciuto_scegli_il_rischio")}>
          <Select id="probe-risk" value={probe.risk} disabled={!!known} onChange={(e) => set({ risk: e.target.value as ToolRisk })}>
            {RISKS.map((r) => <option key={r} value={r}>{RISK_META[r].label}</option>)}
          </Select>
        </Field>
      </div>
      <div className="row tester-flags">
        <label className="check-inline"><input type="checkbox" checked={probe.sessionAllowed} onChange={(e) => set({ sessionAllowed: e.target.checked })} /> {" "}{tr("security.gia_consentito_per_la_sessione")}</label>
        {needsSandbox && (
          <label className="check-inline"><input type="checkbox" checked={probe.sandboxAvailable} onChange={(e) => set({ sandboxAvailable: e.target.checked })} /> {" "}{tr("security.sandbox_disponibile_su_questo_sistema")}</label>
        )}
      </div>

      {analysis && (
        <p className="text-sm muted" aria-live="polite">
          {analysis.commands.length === 0 ? tr("security.nessun_comando_da_eseguire") : analysis.commands.length === 1 ? tr("security.letto_come_un_comando") : tr("security.letto_come_comandi", { length: analysis.commands.length })}
          {analysis.commands.map((c, i) => <span key={`${i}-${c}`}>{i > 0 && " · "}<code>{c}</code></span>)}
          {!analysis.confident && <> {" "}{trNode("security.nessuna_regola_consenti_puo_valere", { b: <b>{tr("security.non_analizzabile_con_certezza")}</b> })}</>}
        </p>
      )}
      <div className="verdict" role="status" aria-live="polite" data-kind={result?.decision.kind}>
        {result ? (
          <>
            <Pill tone={TONE[result.decision.kind]}>{DECISION_LABEL[result.decision.kind]}</Pill>
            <span>
              {result.decision.by === "rule" && result.decision.ruleIndex !== undefined
                ? <>{trNode("security.rule_decides", { index: <b>{result.decision.ruleIndex + 1}</b>, reason: result.decision.reason })}</>
                : <>{tr("security.nessuna_regola_decide", { reason: result.decision.reason })}</>}
              {result.matches.length > 1 && <> {tr("security.matching_rules", { list: result.matches.map((i) => i + 1).join(", ") })}</>}
            </span>
          </>
        ) : (
          <span className="faint">{tr("security.indica_un_tool_per_vedere")}</span>
        )}
      </div>
    </Panel>
  );
}
