import type { ReactNode } from "react";
import { IconButton } from "./Button";
import { ModalShell } from "./modal";

import { t } from "../i18n";
/** Centered modal dialog (confirmations, short forms). */
export function Dialog({ title, onClose, children, footer, size = "md" }: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
}) {
  return (
    <ModalShell variant="dialog" title={title} onClose={onClose} className={`dialog-${size}`}>
      <div className="overlay-head">
        <h2 className="grow">{title}</h2>
        <IconButton aria-label={t("ui.chiudi")} icon="close" onClick={onClose} />
      </div>
      <div className="overlay-body">{children}</div>
      {footer && <div className="overlay-foot">{footer}</div>}
    </ModalShell>
  );
}

/** Side panel (details, long forms). Same behaviour as Dialog: focus trap, Esc, focus restore, aria-modal. */
export function Drawer({ title, subtitle, onClose, children, footer }: {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <ModalShell variant="drawer" title={title} onClose={onClose}>
      <div className="overlay-head">
        <div className="grow">
          <h2>{title}</h2>
          {subtitle && <div className="muted text-sm">{subtitle}</div>}
        </div>
        <IconButton aria-label={t("ui.chiudi")} icon="close" onClick={onClose} />
      </div>
      <div className="overlay-body">{children}</div>
      {footer && <div className="overlay-foot">{footer}</div>}
    </ModalShell>
  );
}

/** Bottom sheet on phones, centered dialog on wider screens. */
export function Sheet({ title, onClose, children, footer }: { title: string; onClose: () => void; children: ReactNode; footer?: ReactNode }) {
  return (
    <ModalShell variant="sheet" title={title} onClose={onClose}>
      <div className="sheet-grip" aria-hidden="true" />
      <div className="overlay-head">
        <h2 className="grow">{title}</h2>
        <IconButton aria-label={t("ui.chiudi")} icon="close" onClick={onClose} />
      </div>
      <div className="overlay-body">{children}</div>
      {footer && <div className="overlay-foot overlay-foot-stack">{footer}</div>}
    </ModalShell>
  );
}
