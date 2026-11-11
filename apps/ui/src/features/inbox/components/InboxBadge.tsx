import { useOpenProposalCount } from "../api";

/** The number of proposals waiting, next to the section's name; nothing when there are none. */
export function InboxBadge() {
  const n = useOpenProposalCount();
  if (n === 0) return null;
  return <span className="nav-badge" aria-label={n === 1 ? "1 proposta in attesa" : `${n} proposte in attesa`}>{n}</span>;
}
