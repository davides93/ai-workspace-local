import { useCallback, useRef, useState } from "react";
import { addWithLimits } from "../../shared/lib/attachments";
import { toast } from "../../shared/lib/toast";
import { toPending, type PendingFile } from "./model/pending";

/**
 * Files chosen for the next message. `add` keeps what fits the limits (the same as the server's)
 * and tells the person, in Italian, about everything else. They are not part of the draft: they
 * go away with the send and come back if it fails.
 */
export function useAttachments() {
  const [files, setFilesState] = useState<PendingFile[]>([]);
  // The latest list, so that the limits are checked (and said) once per batch, outside any state updater.
  const latest = useRef(files);
  const setFiles = useCallback((next: PendingFile[]) => {
    latest.current = next;
    setFilesState(next);
  }, []);

  const add = useCallback((picked: File[], opts: { pasted?: boolean } = {}) => {
    if (picked.length === 0) return;
    const { files: next, errors } = addWithLimits(latest.current, picked.map((f) => toPending(f, opts.pasted)));
    setFiles(next);
    errors.forEach((e) => toast(e, "crit"));
  }, [setFiles]);
  const remove = useCallback((key: string) => setFiles(latest.current.filter((f) => f.key !== key)), [setFiles]);
  const clear = useCallback(() => setFiles([]), [setFiles]);
  /** Puts back the files of a message that could not be sent, before anything chosen since. */
  const restore = useCallback((back: PendingFile[]) => setFiles(addWithLimits(back, latest.current).files), [setFiles]);

  return { files, add, remove, clear, restore };
}

export type AttachmentsState = ReturnType<typeof useAttachments>;
