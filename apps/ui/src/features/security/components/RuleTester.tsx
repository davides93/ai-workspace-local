import { Field, Input, Panel, Pill, Select, type PillTone } from "../../../shared/ui";
import { APPROVAL_MODES, RISK_META } from "../../../shared/lib/policyModes";
import type { CommandAnalysis, PolicyMode, ToolInfo, ToolRisk } from "../../../shared/api/types";
import { DECISION_LABEL, type DecisionKind, type Probe, type ProbeResult } from "../model/policy";

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
    <Panel title="Prova una chiamata" icon="search">
      <p className="muted text-sm mb-3">Indica cosa farebbe un agente: la prova usa le regole qui sopra, anche non salvate, e la stessa logica del backend.</p>
      <div className="tester-grid">
        <Field label="Tool" htmlFor="probe-tool">
          <Input id="probe-tool" mono list="probe-tools" placeholder="shell" value={probe.tool} onChange={(e) => pickTool(e.target.value)} autoComplete="off" />
          <datalist id="probe-tools">{tools.map((t) => <option key={t.name} value={t.name} />)}</datalist>
        </Field>
        <Field label="Soggetto" htmlFor="probe-subject" hint="Comando, percorso o URL della chiamata">
          <Input id="probe-subject" mono placeholder="git status --short" value={probe.subject} onChange={(e) => set({ subject: e.target.value })} autoComplete="off" />
        </Field>
        <Field label="Modalità dell'agente" htmlFor="probe-mode">
          <Select id="probe-mode" value={probe.mode} onChange={(e) => set({ mode: e.target.value as PolicyMode })}>
            {APPROVAL_MODES.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
          </Select>
        </Field>
        <Field label="Rischio del tool" htmlFor="probe-risk" hint={known ? "Preso dal catalogo dei tool" : "Tool sconosciuto: scegli il rischio"}>
          <Select id="probe-risk" value={probe.risk} disabled={!!known} onChange={(e) => set({ risk: e.target.value as ToolRisk })}>
            {RISKS.map((r) => <option key={r} value={r}>{RISK_META[r].label}</option>)}
          </Select>
        </Field>
      </div>
      <div className="row tester-flags">
        <label className="check-inline"><input type="checkbox" checked={probe.sessionAllowed} onChange={(e) => set({ sessionAllowed: e.target.checked })} /> Già consentito per la sessione</label>
        {needsSandbox && (
          <label className="check-inline"><input type="checkbox" checked={probe.sandboxAvailable} onChange={(e) => set({ sandboxAvailable: e.target.checked })} /> Sandbox disponibile su questo sistema</label>
        )}
      </div>

      {analysis && (
        <p className="text-sm muted" aria-live="polite">
          {analysis.commands.length === 0 ? "Nessun comando da eseguire." : analysis.commands.length === 1 ? "Letto come un comando: " : `Letto come ${analysis.commands.length} comandi: `}
          {analysis.commands.map((c, i) => <span key={`${i}-${c}`}>{i > 0 && " · "}<code>{c}</code></span>)}
          {!analysis.confident && <> — <b>non analizzabile con certezza</b>: nessuna regola Consenti può valere.</>}
        </p>
      )}
      <div className="verdict" role="status" aria-live="polite" data-kind={result?.decision.kind}>
        {result ? (
          <>
            <Pill tone={TONE[result.decision.kind]}>{DECISION_LABEL[result.decision.kind]}</Pill>
            <span>
              {result.decision.by === "rule" && result.decision.ruleIndex !== undefined
                ? <>Decide la regola <b>{result.decision.ruleIndex + 1}</b> ({result.decision.reason}).</>
                : <>Nessuna regola decide: {result.decision.reason}.</>}
              {result.matches.length > 1 && <> Regole che combaciano: {result.matches.map((i) => i + 1).join(", ")}.</>}
            </span>
          </>
        ) : (
          <span className="faint">Indica un tool per vedere l'esito.</span>
        )}
      </div>
    </Panel>
  );
}
