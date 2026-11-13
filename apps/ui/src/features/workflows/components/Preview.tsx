import { Button, CodeBlock, Icon, Pill } from "../../../shared/ui";
import type { Issue } from "../model/draft";

import { t } from "../../../shared/i18n";
const where = (i: Issue): string => {
  if (i.scope === "step") return `Passo ${(i.step ?? 0) + 1}`;
  if (i.scope === "inputs") return `Input ${(i.step ?? 0) + 1}`;
  return { meta: "Generale", trigger: "Trigger", steps: "Passi", inputs: "Input" }[i.scope];
};

/** What is wrong with the workflow: errors block saving, warnings do not. `onGo` brings the step into view. */
export function ValidationList({ issues, stepUids, onGo }: { issues: Issue[]; stepUids: number[]; onGo: (uid: number) => void }) {
  const errors = issues.filter((i) => i.level === "error");
  const warns = issues.filter((i) => i.level === "warn");
  return (
    <div className="validation" role="status" aria-live="polite">
      <div className="row">
        {issues.length === 0 ? <Pill tone="ok"><Icon name="check" size={13} /> {t("workflows.nessun_problema")}</Pill> : (
          <>
            {errors.length > 0 && <Pill tone="crit">{t("workflows.n_errors", { n: errors.length })}</Pill>}
            {warns.length > 0 && <Pill tone="warn">{t("workflows.n_warnings", { n: warns.length })}</Pill>}
            <span className="faint text-xs">{errors.length > 0 ? t("workflows.gli_errori_impediscono_di_salvare") : t("workflows.gli_avvisi_non_impediscono_di")}</span>
          </>
        )}
      </div>
      {issues.length > 0 && (
        <ul className="issue-list">
          {[...errors, ...warns].map((i, n) => {
            const uid = i.scope === "step" && i.step !== undefined ? stepUids[i.step] : undefined;
            return (
              <li key={n} className={i.level}>
                <Icon name={i.level === "error" ? "alert" : "info"} size={14} />
                <span><b>{where(i)}</b>: {i.message}</span>
                {uid !== undefined && <Button size="sm" variant="ghost" onClick={() => onGo(uid)}>{t("workflows.vai")}</Button>}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** The JSON the backend will receive (the webhook secret is masked), live as the form changes. */
export function JsonPreview({ json }: { json: string }) {
  return (
    <div className="json-preview">
      <CodeBlock title="workflow.json" code={json} />
    </div>
  );
}
