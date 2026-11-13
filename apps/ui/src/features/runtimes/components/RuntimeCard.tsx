import { Link } from "../../../app/router";
import type { RuntimeInfo } from "../../../shared/api/types";
import { cx } from "../../../shared/lib/cx";
import { Button, Chip, Chips, InlineText, StatusPill } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
/** One runtime of the catalog: what it is, where it can run, its state and the main action. */
export function RuntimeCard({ runtime: r, deviceAccels, busy, error, onAct, onOpen }: {
  runtime: RuntimeInfo;
  /** Accelerators of this device: a runtime accelerator that is present is highlighted. */
  deviceAccels: Set<string>;
  busy: boolean;
  error?: string;
  onAct: (action: "start" | "stop") => void;
  onOpen: () => void;
}) {
  const unsupported = r.status === "unsupported";
  const active = r.status === "running" || r.status === "starting";
  const pathLike = !!r.status_detail && (r.status_detail.includes("/") || r.status_detail.includes("\\"));
  return (
    <article className={cx("rt-card", r.status === "running" && "is-running", unsupported && "is-dim")} aria-label={r.name}>
      <div className="rt-top">
        <div className="t">
          <b>{r.name}</b>
          <span className="faint text-xs">{r.vendor}</span>
        </div>
        <span className="kind-badge">{r.kind}</span>
      </div>
      <div className="row">
        <StatusPill status={r.status} />
        {r.status_detail && r.status !== "installable" && (
          <span className={cx("faint mono ellipsis text-xs maxw-full", pathLike && "dir-rtl")} title={r.status_detail}>{r.status_detail}</span>
        )}
      </div>
      <p className="rt-desc"><InlineText text={r.description} /></p>
      <Chips label={t("runtimes.acceleratori_e_formati")}>
        {r.accelerators.map((a) => <Chip key={a} hot={deviceAccels.has(a)} title={deviceAccels.has(a) ? t("runtimes.presente_su_questo_device") : undefined}>{a}</Chip>)}
        {r.formats.map((f) => <Chip key={f}>.{f}</Chip>)}
      </Chips>
      {r.loaded_models.length > 0 && <div className="mono faint ellipsis text-xs" title={r.loaded_models.join(", ")}>{t("runtimes.caricato", { models: r.loaded_models.join(", ") })}</div>}
      {error && <p role="alert" className="crit-text text-sm">{error}</p>}
      <div className="rt-foot">
        {active && r.kind === "system" ? (
          <Link to="/models?origin=system" className="btn primary">{t("runtimes.vedi_i_modelli_di_sistema")}</Link>
        ) : active ? (
          <Button icon="stop" disabled={busy} aria-label={`Ferma ${r.name}`} onClick={() => onAct("stop")}>{t("runtimes.ferma")}</Button>
        ) : r.status === "installable" && r.install_auto ? (
          <Button variant="primary" icon="download" aria-label={`Installa ${r.name}`} onClick={onOpen}>{t("runtimes.installa")}</Button>
        ) : r.status === "installable" ? (
          <span className="mono faint ellipsis text-xs" title={r.install_hint}>{r.install_hint ?? t("runtimes.installa_dal_sito_del_progetto")}</span>
        ) : !unsupported ? (
          <Button variant="primary" icon="play" disabled={busy} aria-label={t("runtimes.avvia", { name: r.name })} onClick={() => onAct("start")}>{busy ? t("runtimes.avvio") : t("runtimes.avvia_2")}</Button>
        ) : (
          <span className="faint text-xs">{t("runtimes.disponibile_su", { platforms: r.platforms.join(", ") })}</span>
        )}
        <Button variant="ghost" className="ml-auto" icon="settings" aria-label={`Dettagli e impostazioni di ${r.name}`} onClick={onOpen}>{t("runtimes.dettagli")}</Button>
      </div>
    </article>
  );
}
