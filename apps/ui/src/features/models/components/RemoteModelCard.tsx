import { Button, Chip, Icon, Pill } from "../../../shared/ui";
import { formatCompact, formatParams, relTime } from "../../../shared/lib/format";
import type { RemoteModel } from "../../../shared/api/types";
import { CapabilityChips, ChipRow } from "./common";

import { t } from "../../../shared/i18n";
/** Search result: who publishes it, how popular it is, which formats and capabilities, and the way in to its variants. */
export function RemoteModelCard({ model: m, onOpen }: { model: RemoteModel; onOpen: () => void }) {
  return (
    <article className="mh-card" aria-label={m.name}>
      <header className="mh-card-head">
        <div className="mh-title">
          <h3 className="ellipsis" title={m.id}>{m.name}</h3>
          <span className="faint text-xs ellipsis">{m.publisher}{m.license ? ` · ${m.license}` : ""}</span>
        </div>
        {m.gated && (
          <Pill tone="warn" title="Repository gated: per scaricarlo serve un token con accesso al modello">
            <Icon name="lock" size={12} /> {" "}{t("models.gated")}</Pill>
        )}
      </header>

      {m.description && <p className="mh-desc">{m.description}</p>}

      <ChipRow label={t("models.formati")}>{m.formats.map((f) => <Chip key={f}>.{f}</Chip>)}</ChipRow>
      <CapabilityChips items={m.capabilities} />

      <footer className="mh-card-foot">
        <span className="mh-meta faint text-xs num">
          {m.downloads !== undefined && <span>{t("models.download_2", { p: formatCompact(m.downloads) })}</span>}
          {m.likes !== undefined && m.likes > 0 && <span>{t("models.like", { p: formatCompact(m.likes) })}</span>}
          {m.params_b !== undefined && <span>{formatParams(m.params_b)}</span>}
          {m.updated_at && <span>{t("models.agg", { p: relTime(m.updated_at) })}</span>}
        </span>
        <Button size="sm" className="ml-auto" icon="list" aria-label={`Varianti di ${m.name}`} onClick={onOpen}>{t("models.varianti")}</Button>
      </footer>
    </article>
  );
}
