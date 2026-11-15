import { useEffect, useId, useRef, useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { formatBytes, formatDateTime } from "../../../shared/lib/format";
import { toast } from "../../../shared/lib/toast";
import type { ChannelInfo, EmailDraft } from "../../../shared/api/types";
import { Button, DecisionDialog, Field, Icon, Input, KeyValue, Pill } from "../../../shared/ui";
import { Link } from "../../../app/router";
import { useCancelDraft, useDeleteDraft, useRejectDraft, useSendDraft } from "../api";
import { DRAFT_TONE, canEdit, canResend, canSend, canStop, draftStatusLabel, draftTitle, isClosed, recipientsSummary, shortHash, sourceLabel } from "../model/drafts";
import { DraftEditor } from "./DraftEditor";

import { t } from "../../../shared/i18n";

type Panel = "none" | "edit" | "send" | "resend" | "reject";

/**
 * One draft, laid out for the review: who it is from and to (hidden copies included), the subject, the whole text, the
 * attachments and the fingerprint of what is shown. «Approva e invia» asks again, with the recipients, and sends the
 * fingerprint it showed: if the draft changed in between, nothing goes out.
 */
export function DraftCard({ draft, channels, onGone }: { draft: EmailDraft; channels: ChannelInfo[]; onGone: () => void }) {
  const send = useSendDraft();
  const reject = useRejectDraft();
  const cancel = useCancelDraft();
  const remove = useDeleteDraft();
  const [panel, setPanel] = useState<Panel>("none");
  const [confirmDelete, setConfirmDelete] = useState(false);
  // What the owner was shown when they opened the confirmation: it is what they approve, whatever the list says a moment later.
  const [armed, setArmed] = useState<{ hash: string } | null>(null);
  const card = useRef<HTMLElement>(null);
  const before = useRef<Panel>("none");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const title = useId();
  const sum = recipientsSummary(draft);
  const fail = (e: unknown) => setError(errorMessage(e));

  const openSend = (kind: "send" | "resend") => {
    setError(null);
    send.reset();
    setArmed({ hash: draft.content_hash });
    setPanel(kind);
  };
  const doSend = (resend: boolean) => {
    if (!armed) return;
    setError(null);
    send.mutate({ id: draft.id, body: { approved_hash: armed.hash, resend } }, {
      onSuccess: () => { setPanel("none"); setArmed(null); toast(t("channels.bozza_inviata_msg")); onGone(); },
    });
  };

  // The draft changed under an open confirmation (the assistant revised it): what was shown is not what would go, so the
  // confirmation closes and asks to look again.
  useEffect(() => {
    if ((panel === "send" || panel === "resend") && armed && draft.content_hash !== armed.hash) {
      setPanel("none");
      setArmed(null);
      setError(t("channels.bozza_cambiata"));
    }
  }, [panel, armed, draft.content_hash]);

  // Keyboard focus: a panel that opens takes it, and the card gets it back when the panel closes (the button that opened
  // it is gone by then).
  useEffect(() => {
    if (panel === "none" && (before.current === "edit" || before.current === "reject")) card.current?.focus();
    before.current = panel;
  }, [panel]);

  return (
    <article className="panel draft" aria-labelledby={title} ref={card} tabIndex={-1}>
      <header className="draft-head">
        <h3 id={title} className="grow">{draftTitle(draft)}</h3>
        <Pill tone={DRAFT_TONE[draft.status]}>{draftStatusLabel(draft.status)}</Pill>
      </header>
      <p className="faint text-xs">
        {draft.created_by === "assistant" ? t("channels.bozza_da_assistente") : t("channels.bozza_da_te")} · {t("channels.bozza_origine", { source: sourceLabel(draft) })} · {t("channels.bozza_revisione", { n: draft.revision })}
      </p>
      {draft.created_by === "assistant" && draft.status === "draft" && <p className="chn-warn"><Icon name="alert" size={14} /> {t("channels.bozza_avviso_assistente")}</p>}
      {draft.duplicate_of && <p className="chn-warn"><Icon name="alert" size={14} /> {t("channels.bozza_duplicata")}</p>}
      {draft.note && <p className={draft.status === "rejected" || draft.status === "cancelled" ? "muted text-sm" : "chn-error"} role={draft.status === "failed" || draft.status === "uncertain" ? "alert" : undefined}>{draft.note}</p>}

      {panel === "edit" ? (
        <DraftEditor draft={draft} channels={channels} onDone={() => setPanel("none")} />
      ) : (
        <>
          <KeyValue
            items={[
              [t("channels.bozza_da"), <span key="f" className="mono">{draft.from || "—"}</span>],
              [t("channels.bozza_a"), draft.to.length ? <span key="t" className="mono">{draft.to.join(", ")}</span> : <span key="t" className="faint">{t("channels.bozza_nessun_destinatario")}</span>],
              draft.cc.length > 0 && [t("channels.bozza_cc"), <span key="c" className="mono">{draft.cc.join(", ")}</span>],
              draft.bcc.length > 0 && [t("channels.bozza_ccn"), <span key="b" className="mono">{draft.bcc.join(", ")}</span>],
              [t("channels.bozza_oggetto"), draftTitle(draft)],
            ]}
          />
          <div className="field">
            <span className="chn-label">{t("channels.bozza_testo")}</span>
            <p className="draft-body">{draft.body || <span className="faint">—</span>}</p>
          </div>
          <div className="field">
            <span className="chn-label">{t("channels.bozza_allegati")}</span>
            {draft.attachments.length === 0 ? (
              <span className="faint text-sm">{t("channels.bozza_nessun_allegato")}</span>
            ) : (
              <ul className="draft-files">
                {draft.attachments.map((a) => (
                  <li key={a.sha256}>
                    <span className="grow"><strong>{a.name}</strong> · {formatBytes(a.size)} · <span className="mono">{a.mime}</span> · <span className="mono faint">{shortHash(a.sha256)}</span></span>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <p className="faint text-xs">{t("channels.bozza_impronta", { hash: shortHash(draft.content_hash) })}{draft.sent_at && ` · ${t("channels.bozza_inviata_il", { date: formatDateTime(draft.sent_at) })}`}</p>
        </>
      )}

      {draft.status === "uncertain" && <p className="chn-warn"><Icon name="alert" size={14} /> {t("channels.bozza_incerta")}</p>}
      {error && <p className="chn-error" role="alert">{error}</p>}

      {panel === "reject" && (
        <form className="draft-reject" onSubmit={(e) => { e.preventDefault(); reject.mutate({ id: draft.id, reason: reason.trim() || undefined }, { onSuccess: () => { setPanel("none"); toast(t("channels.bozza_rifiutata_msg")); onGone(); }, onError: fail }); }}>
          <Field label={t("channels.bozza_motivo")} htmlFor={`${title}-reason`}>
            <Input id={`${title}-reason`} value={reason} maxLength={300} autoFocus onChange={(e) => setReason(e.target.value)} />
          </Field>
          <div className="row">
            <Button variant="danger" type="submit" disabled={reject.isPending}>{t("channels.bozza_rifiuta_conferma")}</Button>
            <Button variant="ghost" onClick={() => setPanel("none")}>{t("channels.bozza_conferma_no")}</Button>
          </div>
        </form>
      )}

      {/* The buttons stay while the confirmation is open: the dialog gives the focus back to the one that opened it. */}
      {(panel === "none" || panel === "send" || panel === "resend") && (
        <div className="row draft-actions">
          {canSend(draft) && <Button variant="primary" icon="send" disabled={draft.to.length === 0} onClick={() => openSend("send")}>{t("channels.bozza_invia")}</Button>}
          {canResend(draft) && <Button variant="danger" onClick={() => openSend("resend")}>{t("channels.bozza_invia_comunque")}</Button>}
          {canEdit(draft) && <Button icon="edit" onClick={() => setPanel("edit")}>{t("channels.bozza_modifica")}</Button>}
          {canStop(draft) && <Button onClick={() => { setError(null); setPanel("reject"); }}>{t("channels.bozza_rifiuta")}</Button>}
          {canStop(draft) && <Button variant="ghost" disabled={cancel.isPending} onClick={() => cancel.mutate(draft.id, { onSuccess: () => { toast(t("channels.bozza_annullata_msg")); onGone(); }, onError: fail })}>{t("channels.bozza_annulla")}</Button>}
          {draft.session_id && <Link to={`/chat/${draft.session_id}`} className="btn ghost">{t("channels.bozza_apri_chat")}</Link>}
          <span className="grow" />
          {(isClosed(draft) || draft.status === "uncertain" || draft.status === "failed") && (
            <Button variant="ghost" icon="trash" onClick={() => setConfirmDelete(true)}>{t("channels.bozza_elimina")}</Button>
          )}
        </div>
      )}

      {confirmDelete && (
        <DecisionDialog
          title={t("channels.bozza_elimina_titolo")}
          confirmLabel={t("channels.bozza_si_elimina")}
          pending={remove.isPending}
          error={remove.error ?? undefined}
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => remove.mutate(draft.id, { onSuccess: () => { setConfirmDelete(false); onGone(); } })}
        >
          <p>{t("channels.bozza_elimina_testo")}</p>
        </DecisionDialog>
      )}

      {(panel === "send" || panel === "resend") && (
        <DecisionDialog
          title={t("channels.bozza_conferma_titolo")}
          tone="primary"
          icon="send"
          confirmLabel={t("channels.bozza_conferma_invia")}
          cancelLabel={t("channels.bozza_conferma_no")}
          pending={send.isPending}
          error={send.error ?? undefined}
          onCancel={() => { setPanel("none"); setArmed(null); }}
          onConfirm={() => doSend(panel === "resend")}
        >
          <p>{t("channels.bozza_conferma_testo", { to: sum.to, cc: sum.cc, bcc: sum.bcc, n: draft.attachments.length })}</p>
          <p className="faint text-sm"><strong>{draftTitle(draft)}</strong> · {t("channels.bozza_impronta", { hash: shortHash(draft.content_hash) })}</p>
          {panel === "resend" && <p className="chn-warn"><Icon name="alert" size={14} /> {t("channels.bozza_incerta")}</p>}
        </DecisionDialog>
      )}
    </article>
  );
}
