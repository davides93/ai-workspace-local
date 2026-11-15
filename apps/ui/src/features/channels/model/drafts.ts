import type { DraftStatus, EmailDraft } from "../../../shared/api/types";
import type { PillTone } from "../../../shared/ui";

import { t } from "../../../shared/i18n";

/** Tone of the status pill: what needs the owner (waiting, uncertain, failed) stands out from what is over. */
export const DRAFT_TONE: Record<DraftStatus, PillTone> = { draft: "info", sending: "run", sent: "ok", failed: "crit", uncertain: "warn", rejected: "neutral", cancelled: "neutral" };

export function draftStatusLabel(s: DraftStatus): string {
  switch (s) {
    case "draft": return t("channels.bozza_stato_draft");
    case "sending": return t("channels.bozza_stato_sending");
    case "sent": return t("channels.bozza_stato_sent");
    case "failed": return t("channels.bozza_stato_failed");
    case "uncertain": return t("channels.bozza_stato_uncertain");
    case "rejected": return t("channels.bozza_stato_rejected");
    case "cancelled": return t("channels.bozza_stato_cancelled");
  }
}

/** A draft the owner still has to deal with. */
export const needsOwner = (d: Pick<EmailDraft, "status">): boolean => d.status === "draft" || d.status === "failed" || d.status === "uncertain" || d.status === "sending";

/** Can the content still be edited (a draft that failed becomes a draft again when it is changed)? */
export const canEdit = (d: Pick<EmailDraft, "status">): boolean => d.status === "draft" || d.status === "failed";

/** Can the owner refuse or withdraw it (never once it may have gone)? */
export const canStop = canEdit;

/** Can it be approved (an uncertain one only with an explicit «send anyway»)? */
export const canSend = (d: Pick<EmailDraft, "status">): boolean => d.status === "draft" || d.status === "failed";

export const canResend = (d: Pick<EmailDraft, "status">): boolean => d.status === "uncertain";

export const isClosed = (d: Pick<EmailDraft, "status">): boolean => d.status === "sent" || d.status === "rejected" || d.status === "cancelled";

/** Drafts to show: the ones that need the owner (newest first), and the closed ones only when asked for. */
export function visibleDrafts(list: EmailDraft[], showClosed: boolean): EmailDraft[] {
  return list.filter((d) => needsOwner(d) || showClosed).sort((a, b) => b.created_at.localeCompare(a.created_at));
}

/** The subject, or the words that say there is none (a draft can be saved before it is finished). */
export const draftTitle = (d: Pick<EmailDraft, "subject">): string => d.subject.trim() || t("channels.bozza_senza_oggetto");

/** First characters of the content hash: enough to tell two versions apart, the full value is what the approval carries. */
export const shortHash = (hash: string): string => hash.slice(0, 12);

/** The recipients, for the confirmation: «a x, y» plus the copies when there are any. */
export function recipientsSummary(d: Pick<EmailDraft, "to" | "cc" | "bcc">): { to: string; cc: string; bcc: string } {
  return {
    to: d.to.length ? d.to.join(", ") : t("channels.bozza_nessun_destinatario"),
    cc: d.cc.length ? t("channels.bozza_conferma_cc", { list: d.cc.join(", ") }) : "",
    bcc: d.bcc.length ? t("channels.bozza_conferma_ccn", { list: d.bcc.join(", ") }) : "",
  };
}

/** Where the draft came from, in plain words (`mail:<id>` and the like are kept as they are, they are the reference). */
export const sourceLabel = (d: Pick<EmailDraft, "source" | "created_by">): string => d.source || d.created_by;
