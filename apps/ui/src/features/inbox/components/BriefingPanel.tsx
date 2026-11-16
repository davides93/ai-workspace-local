import { Link } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { relTime } from "../../../shared/lib/format";
import { Panel, Pill } from "../../../shared/ui";
import { useProposals, useTasks } from "../api";
import { badgeCount, latestBriefing } from "../model/proposals";
import { taskSummary } from "../model/tasks";

import { t } from "../../../shared/i18n";
/**
 * Panoramica: the latest morning briefing, how many proposals wait and how many background tasks are at work or paused.
 * Stays out of the way when there is nothing of that. The tasks line is for Developer mode: the simple interface manages
 * them from the chat, and has no page to link to.
 */
export function BriefingPanel() {
  const all = useProposals();
  const tasks = useTasks();
  const [mode] = useUiMode();
  const list = all.data ?? [];
  const briefing = latestBriefing(list);
  const open = badgeCount(list);
  const working = mode === "developer" ? taskSummary(tasks.data) : undefined;
  if (!briefing && open === 0 && !working) return null;
  return (
    <Panel title={t("inbox.dal_tuo_assistente")} icon="bulb" actions={open > 0 && <Pill tone="warn">{open === 1 ? t("inbox.1_proposta") : t("inbox.proposte_2", { open })}</Pill>}>
      <div className="stack">
        {briefing && (
          <div>
            <p className="text-sm">{briefing.noticed}</p>
            <span className="faint text-xs">{briefing.title} · {relTime(briefing.created_at)}</span>
          </div>
        )}
        {working && (
          <p className="text-sm">{working}{" · "}<Link to="/inbox">{t("inbox.task_overview_link")}</Link></p>
        )}
        {open > 0 && <Link to="/inbox" className="text-sm">{t("inbox.guarda_le_proposte")}</Link>}
      </div>
    </Panel>
  );
}
