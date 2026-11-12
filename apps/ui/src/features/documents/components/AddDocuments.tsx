import { useRef, useState, type DragEvent } from "react";
import { Button, Field, Icon, Input, Meter } from "../../../shared/ui";
import { errorMessage } from "../../../shared/lib/errors";
import type { KbJob } from "../../../shared/api/types";
import { useAddFiles, useAddPath, useCancelJob, useReindex } from "../api";
import { isRunning, jobPercent, jobProgressText, jobSummary } from "../model/kb";

interface Props {
  collectionId: string;
  jobs: KbJob[];
  developer: boolean;
}

/** Adds files (picker or drag and drop) and, in developer mode, a folder by path; shows the jobs of the collection. */
export function AddDocuments({ collectionId, jobs, developer }: Props) {
  const add = useAddFiles();
  const addPath = useAddPath();
  const reindex = useReindex();
  const cancel = useCancelJob();
  const picker = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);
  const [notes, setNotes] = useState<string[]>([]);
  const [path, setPath] = useState("");

  const send = (files: File[]) => {
    if (files.length === 0) return;
    add.mutate({ collectionId, files }, { onSuccess: (r) => setNotes(r.errors) });
  };
  const carriesFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
  const onDrop = (e: DragEvent) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    setDragging(false);
    send(Array.from(e.dataTransfer.files));
  };
  const mine = jobs.filter((j) => j.collection_id === collectionId).slice(0, 3);
  const running = mine.some(isRunning);

  return (
    <section className="kb-add" aria-label="Aggiungi documenti">
      <div
        className={`kb-drop${dragging ? " is-dragging" : ""}`}
        onDragOver={(e) => { if (carriesFiles(e)) { e.preventDefault(); setDragging(true); } }}
        onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false); }}
        onDrop={onDrop}
      >
        <Icon name="file" size={20} />
        <p>Trascina qui i file, oppure scegli dal computer.</p>
        <p className="faint text-xs">Testo, Markdown, codice e PDF: fino a 10 MB ciascuno.</p>
        <input ref={picker} type="file" multiple hidden tabIndex={-1} aria-hidden="true" data-testid="kb-picker" onChange={(e) => { send(Array.from(e.target.files ?? [])); e.target.value = ""; }} />
        <Button icon="plus" disabled={add.isPending} onClick={() => picker.current?.click()}>{add.isPending ? "Invio in corso…" : "Scegli file"}</Button>
      </div>
      {add.isError && <p className="kb-error text-sm" role="alert">{errorMessage(add.error)}</p>}
      {notes.length > 0 && (
        <ul className="kb-notes text-sm" aria-label="File lasciati fuori">
          {notes.map((n) => <li key={n}>{n}</li>)}
        </ul>
      )}

      {developer && (
        <form className="kb-path" onSubmit={(e) => { e.preventDefault(); if (path.trim()) addPath.mutate({ collectionId, path }, { onSuccess: () => setPath("") }); }}>
          <Field label="Aggiungi una cartella o un file dal computer" htmlFor="kb-path" hint="Percorso assoluto, ad esempio /home/nome/Documenti. Funziona solo da questo computer." error={addPath.isError ? errorMessage(addPath.error) : undefined}>
            <Input id="kb-path" mono value={path} placeholder="/home/nome/Documenti" onChange={(e) => setPath(e.target.value)} />
          </Field>
          <div className="kb-path-actions">
            <Button type="submit" icon="file" disabled={!path.trim() || addPath.isPending}>Aggiungi cartella</Button>
            <Button type="button" variant="ghost" icon="refresh" disabled={running || reindex.isPending} onClick={() => reindex.mutate(collectionId)}>Reindicizza</Button>
          </div>
          {reindex.isError && <p className="kb-error text-sm" role="alert">{errorMessage(reindex.error)}</p>}
        </form>
      )}

      {mine.length > 0 && (
        <ul className="kb-jobs" aria-label="Operazioni">
          {mine.map((j) => (
            <li key={j.id} className="kb-job">
              {isRunning(j) ? (
                <>
                  <div className="kb-job-head">
                    <b>In corso</b>
                    <span className="faint text-sm">{jobProgressText(j)}</span>
                    <span className="grow" />
                    <Button size="sm" variant="ghost" onClick={() => cancel.mutate(j.id)}>Interrompi</Button>
                  </div>
                  <Meter value={jobPercent(j)} max={100} label="Avanzamento" />
                </>
              ) : (
                <>
                  <p className="text-sm">{jobSummary(j)}</p>
                  {j.skipped.length > 0 && (
                    <ul className="kb-notes text-xs">
                      {j.skipped.map((s) => <li key={`${s.name}|${s.reason}`}><b>{s.name}</b>: {s.reason}</li>)}
                      {j.skipped_total > j.skipped.length && <li>…e altri {j.skipped_total - j.skipped.length}.</li>}
                    </ul>
                  )}
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
