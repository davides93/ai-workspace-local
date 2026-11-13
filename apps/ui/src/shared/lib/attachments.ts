import type { AttachmentKind } from "../api/types";

import { t } from "../i18n";
/** Same limits as the server (crates/aiw-core `MAX_ATTACHMENTS_*`): checked here to answer before sending. */
export const MAX_ATTACHMENTS = 8;
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
export const MAX_ATTACHMENTS_TOTAL_BYTES = 25 * 1024 * 1024;

/** «1,4 MB», «12 KB», «300 byte»: the size of a file in words anyone reads. */
export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} byte`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1).replace(".", ",") : Math.round(kb)} KB`;
  return `${(kb / 1024).toFixed(1).replace(".", ",")} MB`;
}

const IMAGE_MIMES = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const TEXT_EXTENSIONS = new Set([
  "txt", "md", "markdown", "csv", "tsv", "json", "xml", "html", "htm", "css", "js", "mjs", "ts", "tsx", "jsx", "py", "rs", "go", "java", "c", "h", "cpp", "sh", "yaml", "yml", "toml", "ini", "log", "sql",
]);

const extensionOf = (name: string) => (name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "");

/**
 * What a file looks like before it is sent (the server decides for good from the content): used for
 * the icon and the preview of the chips.
 */
export function guessKind(name: string, mime: string): AttachmentKind {
  const m = mime.toLowerCase();
  if (IMAGE_MIMES.includes(m)) return "image";
  if (m === "application/pdf" || extensionOf(name) === "pdf") return "document";
  if (m.startsWith("text/") || TEXT_EXTENSIONS.has(extensionOf(name))) return "text";
  return "file";
}

/** A name for a pasted image, which has none of its own («Immagine 14-32-05.png»). */
export function pastedName(mime: string, at: Date = new Date()): string {
  const ext = mime.split("/")[1]?.replace("jpeg", "jpg").replace(/[^a-z0-9]/gi, "") || "png";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `Immagine ${pad(at.getHours())}-${pad(at.getMinutes())}-${pad(at.getSeconds())}.${ext}`;
}

export interface Candidate {
  name: string;
  size: number;
}

export interface Accepted<T extends Candidate> {
  /** What stays attached: the old ones, then the new ones that fit. */
  files: T[];
  /** One Italian sentence per refused file or limit. */
  errors: string[];
}

/** Adds `incoming` to `current` while the limits hold; everything that does not fit is left out with the reason. */
export function addWithLimits<T extends Candidate>(current: T[], incoming: T[]): Accepted<T> {
  const files = [...current];
  const errors: string[] = [];
  let total = files.reduce((n, f) => n + f.size, 0);
  for (const f of incoming) {
    if (files.length >= MAX_ATTACHMENTS) {
      errors.push(t("lib.puoi_allegare_al_massimo_file", { max: MAX_ATTACHMENTS, name: f.name }));
    } else if (f.size === 0) {
      errors.push(t("lib.e_vuoto_non_c_e", { name: f.name }));
    } else if (f.size > MAX_ATTACHMENT_BYTES) {
      errors.push(t("lib.e_troppo_grande_ogni_file", { name: f.name, size: humanSize(f.size), max: humanSize(MAX_ATTACHMENT_BYTES) }));
    } else if (total + f.size > MAX_ATTACHMENTS_TOTAL_BYTES) {
      errors.push(t("lib.non_entra_in_totale_gli", { name: f.name, max: humanSize(MAX_ATTACHMENTS_TOTAL_BYTES) }));
    } else {
      files.push(f);
      total += f.size;
    }
  }
  return { files, errors };
}
