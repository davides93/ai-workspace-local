import { Link } from "../../../app/router";
import { cx } from "../../../shared/lib/cx";
import { Icon } from "../../../shared/ui";
import type { RunNoticeInfo } from "../model/runNotice";

/**
 * How a run ended, when it did not end well: stopped by the user (neutral), runtime failure
 * (critical, with a way to the runtimes), blocked by a hook (warning). `live` announces it to
 * assistive technology; a note rebuilt from history is read in order instead.
 */
export function RunNote({ info, message, live = false }: { info: RunNoticeInfo; message?: string; live?: boolean }) {
  return (
    <div className={cx("ws-note", `ws-note-${info.tone}`)} role={live ? (info.tone === "crit" ? "alert" : "status") : undefined}>
      <Icon name={info.icon} size={18} />
      <div className="ws-note-body">
        <b>{info.title}</b>
        {message && <span className="ws-note-msg">{message}</span>}
        {info.hint && <span className="ws-note-hint">{info.hint}</span>}
        {info.link && (
          <Link to={info.link.to} className="ws-note-link">
            {info.link.label} <Icon name="forward" size={12} />
          </Link>
        )}
      </div>
    </div>
  );
}
