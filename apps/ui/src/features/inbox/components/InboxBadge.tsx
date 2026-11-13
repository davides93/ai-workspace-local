import { useOpenProposalCount } from "../api";

import { t } from "../../../shared/i18n";
/** The number of proposals waiting, next to the section's name; nothing when there are none. */
export function InboxBadge() {
  const n = useOpenProposalCount();
  if (n === 0) return null;
  return <span className="nav-badge" aria-label={n === 1 ? t("inbox.1_proposta_in_attesa") : t("inbox.proposte_in_attesa", { n })}>{n}</span>;
}
