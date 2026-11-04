import { useEffect } from "react";
import { Field, Input, Pill, Select, RowControls } from "../../../shared/ui";
import type { Keyed } from "../../../shared/lib/keyedList";
import type { PolicyAction, PolicyRule } from "../../../shared/api/types";
import { ACTION_LABEL, type RuleIssue } from "../model/policy";

export type RuleMark = "winner" | "match";

const ACTIONS: PolicyAction[] = ["allow", "ask", "deny"];

/** Rules as editable rows: position, tool glob, pattern glob, action and the reorder/remove buttons. */
export function RuleList({ rows, issues, marks, onChange, onMove, onRemove, focusUid }: {
  rows: Keyed<PolicyRule>[];
  issues: RuleIssue[];
  /** Rows highlighted by the rule tester, by position. */
  marks: Map<number, RuleMark>;
  onChange: (uid: number, patch: Partial<PolicyRule>) => void;
  onMove: (uid: number, delta: -1 | 1) => void;
  onRemove: (uid: number) => void;
  /** Row just added: its tool field takes the focus. */
  focusUid: number | null;
}) {
  useEffect(() => {
    if (focusUid !== null) document.getElementById(`rule-tool-${focusUid}`)?.focus();
  }, [focusUid]);

  return (
    <ol className="rule-list" aria-label="Regole di policy">
      {rows.map(({ uid, value: r }, i) => {
        const mark = marks.get(i);
        const mine = issues.filter((x) => x.index === i);
        const n = i + 1;
        return (
          <li key={uid} className="rule-row" data-action={r.action} data-mark={mark}>
            <span className="rule-n" aria-hidden="true">{n}</span>
            <Field label={`Tool della regola ${n}`} htmlFor={`rule-tool-${uid}`} className="rule-tool">
              <Input id={`rule-tool-${uid}`} mono placeholder="shell, mcp__github__*" value={r.tool} onChange={(e) => onChange(uid, { tool: e.target.value })} />
            </Field>
            <Field label={`Pattern della regola ${n}`} htmlFor={`rule-pat-${uid}`} className="rule-pattern">
              <Input id={`rule-pat-${uid}`} mono placeholder="qualsiasi · git status*, ~/Workspace/**" value={r.pattern ?? ""} onChange={(e) => onChange(uid, { pattern: e.target.value })} />
            </Field>
            <Field label={`Azione della regola ${n}`} htmlFor={`rule-act-${uid}`} className="rule-action">
              <Select id={`rule-act-${uid}`} value={r.action} onChange={(e) => onChange(uid, { action: e.target.value as PolicyAction })}>
                {ACTIONS.map((a) => <option key={a} value={a}>{ACTION_LABEL[a]}</option>)}
              </Select>
            </Field>
            <div className="rule-buttons">
              {mark && <Pill tone={mark === "winner" ? "run" : "neutral"}>{mark === "winner" ? "decide" : "combacia"}</Pill>}
              <RowControls index={i} count={rows.length} noun={`la regola ${n}`} onMove={(d) => onMove(uid, d)} onRemove={() => onRemove(uid)} />
            </div>
            {mine.length > 0 && (
              <ul className="rule-issues">
                {mine.map((x) => <li key={x.message} className={x.level === "error" ? "field-error" : "row-warn"}>{x.message}</li>)}
              </ul>
            )}
          </li>
        );
      })}
    </ol>
  );
}
