import { useEffect, useState } from "react";
import { useApi } from "../../../shared/api/context";
import type { AttachmentKind } from "../../../shared/api/types";
import { humanSize } from "../../../shared/lib/attachments";
import { downloadBase64 } from "../../../shared/lib/download";
import { errorMessage, isNotFound } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import { Icon, IconButton } from "../../../shared/ui";
import { useAttachmentData } from "../api";
import type { UserAttachment } from "../model/items";
import type { PendingFile } from "../model/pending";
import { ImageThumb } from "./ImageThumb";

const iconFor = (kind: AttachmentKind) => (kind === "image" ? "image" : "file");

/** A temporary address for a chosen file, released when the chip goes away. */
function useObjectUrl(file: File, enabled: boolean): string | undefined {
  const [url, setUrl] = useState<string>();
  useEffect(() => {
    if (!enabled || typeof URL.createObjectURL !== "function") return;
    const u = URL.createObjectURL(file);
    setUrl(u);
    return () => URL.revokeObjectURL(u);
  }, [file, enabled]);
  return url;
}

function PendingChip({ item, onRemove }: { item: PendingFile; onRemove: (key: string) => void }) {
  const preview = useObjectUrl(item.file, item.kind === "image");
  return (
    <li className="ws-att-chip">
      {preview ? <img className="ws-att-thumb" src={preview} alt="" /> : <span className="ws-att-icon" aria-hidden="true"><Icon name={iconFor(item.kind)} size={16} /></span>}
      <span className="ws-att-name" title={item.name}>{item.name}</span>
      <span className="ws-att-size">{humanSize(item.size)}</span>
      <IconButton icon="close" size={14} className="ws-att-remove" aria-label={`Rimuovi l'allegato ${item.name}`} onClick={() => onRemove(item.key)} />
    </li>
  );
}

/** The files chosen for the message being written, above the box. */
export function PendingAttachments({ items, onRemove }: { items: PendingFile[]; onRemove: (key: string) => void }) {
  if (items.length === 0) return null;
  return (
    <ul className="ws-att-chips" aria-label="Allegati del messaggio">
      {items.map((f) => <PendingChip key={f.key} item={f} onRemove={onRemove} />)}
    </ul>
  );
}

/** Image of a message: the bytes it was sent with, or the ones the backend kept. */
function MessageImage({ sessionId, att }: { sessionId?: string; att: UserAttachment }) {
  const stored = useAttachmentData(sessionId, att.id, !att.data);
  const data = att.data ?? stored.data?.data;
  if (data) return <ImageThumb image={{ mime: att.mime, data }} label={att.name} filename={att.name} />;
  return (
    <span className="ws-att-chip ws-att-loading" role={stored.isError ? undefined : "status"}>
      <span className="ws-att-icon" aria-hidden="true"><Icon name="image" size={16} /></span>
      <span className="ws-att-name">{att.name}</span>
      <span className="ws-att-size">{stored.isError ? "non disponibile" : "carico…"}</span>
    </span>
  );
}

function MessageFile({ sessionId, att }: { sessionId?: string; att: UserAttachment }) {
  const api = useApi();
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      const d = att.data ? { name: att.name, mime: att.mime, data: att.data } : sessionId ? await api.getAttachment(sessionId, att.id) : null;
      if (d) downloadBase64(d.name, d.data, d.mime);
    } catch (e) {
      toast(isNotFound(e) ? `Il file «${att.name}» non c'è più.` : `Non riesco a scaricare «${att.name}»: ${errorMessage(e)}`, "crit");
    } finally {
      setBusy(false);
    }
  };
  return (
    <button type="button" className="ws-att-chip ws-att-file" onClick={() => void save()} disabled={busy} aria-label={`Scarica ${att.name} (${humanSize(att.size)})`}>
      <span className="ws-att-icon" aria-hidden="true"><Icon name={iconFor(att.kind)} size={16} /></span>
      <span className="ws-att-name">{att.name}</span>
      <span className="ws-att-size">{humanSize(att.size)}</span>
      <Icon name="download" size={14} />
    </button>
  );
}

/** The attachments of a user message: thumbnails that open larger, chips that download the file. */
export function MessageAttachments({ items, sessionId }: { items: UserAttachment[]; sessionId?: string }) {
  return (
    <div className="ws-user-atts" role="group" aria-label="Allegati del messaggio">
      {items.map((a) => (a.kind === "image" ? <MessageImage key={a.id} sessionId={sessionId} att={a} /> : <MessageFile key={a.id} sessionId={sessionId} att={a} />))}
    </div>
  );
}
