import type { AttachmentKind, AttachmentUpload } from "../../../shared/api/types";
import { guessKind, pastedName } from "../../../shared/lib/attachments";
import { blobToBase64 } from "../../../shared/lib/base64";
import type { UserAttachment } from "./items";

/** A file chosen for the next message, not sent yet. */
export interface PendingFile {
  key: string;
  file: File;
  name: string;
  size: number;
  mime: string;
  kind: AttachmentKind;
}

let counter = 0;

/** Names like «image.png» are what a browser gives to any pasted picture: they get a name of their own. */
const GENERIC_PASTED = /^image\.(png|jpe?g|gif|webp)$/i;

export function toPending(file: File, pasted = false): PendingFile {
  const mime = file.type || "";
  const name = pasted && GENERIC_PASTED.test(file.name) ? pastedName(mime) : file.name || "allegato";
  return { key: `att-${Date.now()}-${counter++}`, file, name, size: file.size, mime, kind: guessKind(name, mime) };
}

/** The bytes of the chosen files as the API wants them (base64). */
export async function toUploads(files: PendingFile[]): Promise<AttachmentUpload[]> {
  return Promise.all(files.map(async (f) => ({ name: f.name, mime: f.mime, data: await blobToBase64(f.file) })));
}

/** The attachments of a message just sent: what the row shows before the backend has stored it. */
export function localAttachments(files: PendingFile[], uploads: AttachmentUpload[]): UserAttachment[] {
  return files.map((f, i) => ({ id: f.key, name: f.name, mime: f.mime || "application/octet-stream", size: f.size, kind: f.kind, data: uploads[i]?.data }));
}
