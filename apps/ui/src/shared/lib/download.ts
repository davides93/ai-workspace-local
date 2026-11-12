import { base64ToBytes } from "./base64";

/** Offers `content` to the user as a file download (a Blob behind a temporary link). */
export function downloadFile(filename: string, content: string, mime = "application/json"): void {
  const url = URL.createObjectURL(new Blob([content], { type: `${mime};charset=utf-8` }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.rel = "noopener";
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoked on the next tick: some browsers start the download asynchronously.
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Offers some bytes (given as base64) to the user as a file download. */
export function downloadBase64(filename: string, base64: string, mime = "application/octet-stream"): void {
  const bytes = base64ToBytes(base64);
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.rel = "noopener";
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
