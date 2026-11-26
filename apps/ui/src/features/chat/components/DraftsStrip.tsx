import { useState } from "react";
import { Button, Sheet } from "../../../shared/ui";
import { t } from "../../../shared/i18n";
import { useChannels, useEmailDrafts } from "../../channels/api";
import { DraftCard } from "../../channels/components/DraftCard";
import { needsOwner } from "../../channels/model/drafts";

/**
 * Simple interface: the email drafts the assistant prepared, waiting for the owner, in a strip above the message box (the Canali
 * page, where they are reviewed in Developer mode, is not there). The button opens the same cards: nothing is sent until the owner
 * approves exactly what a card shows. Nothing is shown when no draft waits.
 */
export function DraftsStrip() {
  const drafts = useEmailDrafts();
  const channels = useChannels();
  const [open, setOpen] = useState(false);
  const waiting = (drafts.data ?? []).filter(needsOwner).sort((a, b) => b.created_at.localeCompare(a.created_at));
  if (waiting.length === 0) return null;
  const email = (channels.data ?? []).filter((c) => c.kind === "email");
  return (
    <section className="ws-drafts" aria-label={t("chat.bozze_da_rivedere_titolo")}>
      <Button icon="send" aria-haspopup="dialog" onClick={() => setOpen(true)}>{t("chat.bozze_da_rivedere", { n: waiting.length })}</Button>
      {open && (
        <Sheet title={t("chat.bozze_da_rivedere_titolo")} onClose={() => setOpen(false)}>
          <p className="muted text-sm">{t("chat.bozze_da_rivedere_nota")}</p>
          <div className="stack stack-lg">
            {waiting.map((d) => <DraftCard key={d.id} draft={d} channels={email} onGone={() => undefined} />)}
          </div>
        </Sheet>
      )}
    </section>
  );
}
