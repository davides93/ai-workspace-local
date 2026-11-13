import { useEffect, useId, useState } from "react";
import { MAX_FEEDBACK_NOTE_CHARS, type Feedback } from "../../../shared/api/types";
import { cx } from "../../../shared/lib/cx";
import { Button, Field, IconButton, Textarea } from "../../../shared/ui";
import { ratingChange } from "../model/feedback";
import { CopyMessage } from "./CopyMessage";
import { ReadAloud } from "./ReadAloud";

import { t } from "../../../shared/i18n";
interface Props {
  messageId: string;
  text: string;
  /** Final answer of a turn: it can be rated. */
  rateable: boolean;
  rating?: Feedback;
  /** A rating of this message is on its way. */
  pending?: boolean;
  onRate: (messageId: string, rating: 1 | -1, note?: string) => void;
  onClear: (messageId: string) => void;
}

/**
 * What can be done with a reply: copy it, read it aloud and, when it is the final answer of a turn, rate it. The row floats under
 * the reply and shows on hover or focus, except when the reply is rated (the choice stays visible). A thumb pressed
 * again takes the rating back; a thumbs down offers an optional note on what went wrong.
 */
export function MessageActions({ messageId, text, rateable, rating, pending, onRate, onClear }: Props) {
  const [noteOpen, setNoteOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const noteId = useId();
  const value = rating?.rating;

  // The rating was taken back or turned into a thumbs up (also from another client): the note has no place any more.
  useEffect(() => {
    if (value !== -1) setNoteOpen(false);
  }, [value]);

  const click = (clicked: 1 | -1) => {
    const change = ratingChange(rating, clicked);
    if (change.kind === "clear") return onClear(messageId);
    onRate(messageId, change.rating);
    if (change.rating === -1) {
      setDraft("");
      setNoteOpen(true);
    }
  };
  const openNote = () => {
    setDraft(rating?.note ?? "");
    setNoteOpen((open) => !open);
  };
  const showNote = rateable && value === -1 && noteOpen;
  const tooLong = [...draft].length > MAX_FEEDBACK_NOTE_CHARS;

  return (
    <>
      <div className={cx("ws-msg-tools", rating && "is-rated", showNote && "is-open")}>
        <CopyMessage text={text} />
        <ReadAloud text={text} />
        {rateable && (
          <>
            <IconButton
              icon="thumbup" size={15} disabled={pending} aria-pressed={value === 1}
              aria-label={t("chat.risposta_utile")} title={value === 1 ? t("chat.valutata_utile_tocca_per_togliere") : t("chat.risposta_utile")}
              className={cx("ws-rate", value === 1 && "on")} onClick={() => click(1)}
            />
            <IconButton
              icon="thumbdown" size={15} disabled={pending} aria-pressed={value === -1}
              aria-label={t("chat.risposta_da_migliorare")} title={value === -1 ? t("chat.valutata_da_migliorare_tocca_per") : t("chat.risposta_da_migliorare")}
              className={cx("ws-rate", value === -1 && "on")} onClick={() => click(-1)}
            />
            {value === -1 && (
              <IconButton
                icon="edit" size={15} aria-expanded={noteOpen} aria-controls={noteId}
                aria-label={rating?.note ? t("chat.modifica_la_nota") : t("chat.aggiungi_una_nota")} title={rating?.note ? t("chat.modifica_la_nota") : t("chat.aggiungi_una_nota")}
                onClick={openNote}
              />
            )}
          </>
        )}
      </div>
      {showNote && (
        <form
          id={noteId} className="ws-rate-note"
          onSubmit={(e) => {
            e.preventDefault();
            if (tooLong) return;
            onRate(messageId, -1, draft.trim() || undefined);
            setNoteOpen(false);
          }}
        >
          <Field
            label={t("chat.cosa_non_ha_funzionato_facoltativo")} htmlFor={`${noteId}-t`}
            error={tooLong ? `La nota supera i ${MAX_FEEDBACK_NOTE_CHARS} caratteri` : undefined}
          >
            <Textarea id={`${noteId}-t`} rows={2} value={draft} autoFocus onChange={(e) => setDraft(e.target.value)} />
          </Field>
          <div className="ws-rate-note-actions">
            <Button size="sm" variant="primary" type="submit" disabled={tooLong || pending}>{t("chat.salva_nota")}</Button>
            <Button size="sm" variant="ghost" onClick={() => setNoteOpen(false)}>{t("chat.chiudi")}</Button>
          </div>
        </form>
      )}
    </>
  );
}
