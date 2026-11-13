import { Link } from "../../../app/router";
import { relTime } from "../../../shared/lib/format";
import { Panel, Pill } from "../../../shared/ui";
import { useProposals } from "../api";
import { badgeCount, latestBriefing } from "../model/proposals";

import { t } from "../../../shared/i18n";
/** Panoramica: the latest morning briefing and how many proposals wait. Stays out of the way when there is neither. */
export function BriefingPanel() {
  const all = useProposals();
  const list = all.data ?? [];
  const briefing = latestBriefing(list);
  const open = badgeCount(list);
  if (!briefing && open === 0) return null;
  return (
    <Panel title={t("inbox.dal_tuo_assistente")} icon="bulb" actions={open > 0 && <Pill tone="warn">{open === 1 ? t("inbox.1_proposta") : t("inbox.proposte_2", { open })}</Pill>}>
      <div className="stack">
        {briefing && (
          <div>
            <p className="text-sm">{briefing.noticed}</p>
            <span className="faint text-xs">{briefing.title} · {relTime(briefing.created_at)}</span>
          </div>
        )}
        {open > 0 && <Link to="/inbox" className="text-sm">{t("inbox.guarda_le_proposte")}</Link>}
      </div>
    </Panel>
  );
}
