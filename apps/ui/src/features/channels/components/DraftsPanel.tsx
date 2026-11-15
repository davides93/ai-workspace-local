import { useId, useState } from "react";
import type { ChannelInfo } from "../../../shared/api/types";
import { Button, EmptyState, QueryState, Toggle } from "../../../shared/ui";
import { useEmailDrafts } from "../api";
import { visibleDrafts } from "../model/drafts";
import { DraftCard } from "./DraftCard";
import { DraftEditor } from "./DraftEditor";

import { t } from "../../../shared/i18n";

/**
 * «Bozze email»: what the assistant prepared (or the owner wrote), waiting to be reviewed. Nothing here leaves before
 * the owner approves exactly what is on the card. Shown when there is an email channel to send through, or a draft.
 */
export function DraftsPanel({ channels }: { channels: ChannelInfo[] }) {
  const drafts = useEmailDrafts();
  const [writing, setWriting] = useState(false);
  const [showClosed, setShowClosed] = useState(false);
  const titleId = useId();
  const email = channels.filter((c) => c.kind === "email");
  const any = (drafts.data?.length ?? 0) > 0;
  if (email.length === 0 && !any) return null;
  return (
    <section className="section" aria-labelledby={titleId}>
      <div className="section-head">
        <h2 id={titleId}>{t("channels.bozze_titolo")}</h2>
        {!writing && email.length > 0 && <div className="section-actions"><Button icon="plus" onClick={() => setWriting(true)}>{t("channels.bozze_scrivi")}</Button></div>}
      </div>
      <p className="muted text-sm">{t("channels.bozze_intro")}</p>
      <p className="faint text-sm">{t("channels.bozze_attenzione")}</p>
      {email.length === 0 && <p className="faint text-sm">{t("channels.bozze_senza_canale")}</p>}
      {writing && <div className="panel draft"><h3>{t("channels.bozza_nuova")}</h3><DraftEditor channels={email} onDone={() => setWriting(false)} /></div>}
      <label className="chn-switch">
        <Toggle checked={showClosed} label={t("channels.bozze_mostra_chiuse")} onChange={setShowClosed} />
        <span>{t("channels.bozze_mostra_chiuse")}</span>
      </label>
      <QueryState query={drafts}>
        {(list) => {
          const shown = visibleDrafts(list, showClosed);
          return shown.length === 0 && !writing ? (
            <EmptyState headingLevel={3} icon="send" title={t("channels.bozze_nessuna")} description={t("channels.bozze_nessuna_desc")} />
          ) : (
            <div className="stack stack-lg">{shown.map((d) => <DraftCard key={d.id} draft={d} channels={email} />)}</div>
          );
        }}
      </QueryState>
    </section>
  );
}
