import { useState } from "react";
import { Button, Dialog, Icon } from "../../../shared/ui";
import type { ToolImage } from "../model/items";
import { imageExtension } from "../model/tools";

import { t, t as tr } from "../../../shared/i18n";
/** Screenshot of a tool call: a thumbnail that opens at full size in a dialog. */
export function ImageThumb({ image, label, filename }: { image: ToolImage; label: string; /** Name offered by «Salva» (default: «schermata»). */ filename?: string }) {
  const [open, setOpen] = useState(false);
  const src = `data:${image.mime};base64,${image.data}`;
  return (
    <>
      <button type="button" className="ws-thumb" onClick={() => setOpen(true)} aria-haspopup="dialog" aria-label={t("chat.apri_a_schermo_intero", { label })}>
        <img src={src} alt="" loading="lazy" decoding="async" />
        <span className="ws-thumb-zoom" aria-hidden="true"><Icon name="external" size={14} /></span>
      </button>
      {open && (
        <Dialog
          title={label}
          size="lg"
          onClose={() => setOpen(false)}
          footer={
            <>
              <a className="btn" href={src} download={filename ?? `schermata.${imageExtension(image.mime)}`}><Icon name="download" size={15} /> {" "}{tr("chat.salva")}</a>
              <Button variant="primary" onClick={() => setOpen(false)}>{t("chat.chiudi")}</Button>
            </>
          }
        >
          <img className="ws-image-full" src={src} alt={label} />
        </Dialog>
      )}
    </>
  );
}
