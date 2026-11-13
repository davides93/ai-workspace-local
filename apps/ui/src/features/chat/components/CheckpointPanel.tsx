import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "../../../shared/query/keys";
import { Button, Dialog } from "../../../shared/ui";
import type { CheckpointInfo } from "../../../shared/api/types";
import { useCheckpoints, useRewind } from "../api";

import { t } from "../../../shared/i18n";
/** Files a run changed or created, with the way back: put the originals back and delete what the run created. */
export function CheckpointPanel({ sessionId, running }: { sessionId: string; running: boolean }) {
  const qc = useQueryClient();
  const list = useCheckpoints(sessionId, !running);
  const [target, setTarget] = useState<CheckpointInfo | null>(null);

  // A finished run may have saved files.
  useEffect(() => {
    if (!running) void qc.invalidateQueries({ queryKey: qk.sessions.checkpoints(sessionId) });
  }, [running, qc, sessionId]);

  // Only checkpoints that can still do something are worth a row.
  const items = (list.data ?? []).filter((c) => c.restore.length + c.delete.length > 0);
  if (items.length === 0) return null;
  return (
    <section className="ws-insp-section ws-ckpt">
      <div className="ws-insp-head">
        <h3>{t("chat.file_modificati_dagli_agenti")}</h3>
      </div>
      <ul className="ws-changed" aria-label={t("chat.checkpoint_dei_run")}>
        {items.map((c) => (
          <li key={c.run_id}>
            <span className="mono">{c.run_id}</span>
            <span className="faint text-sm"> {t("chat.da_ripristinare_da_eliminare", { length: c.restore.length, length2: c.delete.length })}</span>
            <Button size="sm" disabled={running} onClick={() => setTarget(c)}>{t("chat.ripristina_i_file")}</Button>
          </li>
        ))}
      </ul>
      {running && <span className="faint text-xs">{t("chat.il_ripristino_torna_disponibile_alla")}</span>}
      {target && <RewindDialog sessionId={sessionId} info={target} onClose={() => setTarget(null)} />}
    </section>
  );
}

function FileList({ title, files }: { title: string; files: string[] }) {
  if (files.length === 0) return null;
  return (
    <>
      <h4 className="text-sm">{title}</h4>
      <ul className="ws-changed" aria-label={title}>
        {files.map((f) => <li key={f}><span className="mono">{f}</span></li>)}
      </ul>
    </>
  );
}

/** Lists exactly what changes before anything is touched. */
function RewindDialog({ sessionId, info, onClose }: { sessionId: string; info: CheckpointInfo; onClose: () => void }) {
  const rewind = useRewind(sessionId);
  return (
    <Dialog
      title={t("chat.ripristina_i_file")}
      size="sm"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} data-autofocus>{t("chat.annulla")}</Button>
          <Button variant="danger" disabled={rewind.isPending} onClick={() => rewind.mutate(info.run_id, { onSuccess: onClose })}>
            {rewind.isPending ? t("chat.ripristino") : t("chat.ripristina")}
          </Button>
        </>
      }
    >
      <p>Il run <code>{info.run_id}</code> torna com'era prima: le modifiche fatte dopo, a mano o da altri run, a questi file andranno perse.</p>
      <FileList title={t("chat.tornano_com_erano")} files={info.restore} />
      <FileList title={t("chat.vengono_eliminati_li_ha_creati")} files={info.delete} />
      <FileList title={t("chat.fuori_dal_workspace_non_vengono")} files={info.external} />
    </Dialog>
  );
}
