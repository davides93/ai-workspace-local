import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { qk } from "../../../shared/query/keys";
import { Button, Icon, Pill } from "../../../shared/ui";
import type { Session } from "../../../shared/api/types";
import { useWorktree, useWorktreeApply, useWorktreeDiscard } from "../api";

const FILE_STATUS: Record<string, string> = { A: "aggiunto", M: "modificato", D: "eliminato", R: "rinominato", C: "copiato", T: "tipo cambiato" };

/** Changes of an isolated session: review the diff, apply it to the workspace or throw it away. */
export function WorktreePanel({ session, running }: { session: Session; running: boolean }) {
  const qc = useQueryClient();
  const status = useWorktree(session.id, !running);
  const apply = useWorktreeApply();
  const discard = useWorktreeDiscard();
  const [showDiff, setShowDiff] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const wt = session.worktree!;
  const st = status.data;
  const busy = apply.isPending || discard.isPending;

  // Refresh whenever a run ends (the agent may have changed files).
  useEffect(() => {
    if (!running) void qc.invalidateQueries({ queryKey: qk.sessions.worktree(session.id) });
  }, [running, qc, session.id]);

  const onDiscard = () => {
    if (!confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    discard.mutate({ sessionId: session.id, branch: wt.branch });
    setConfirmDiscard(false);
  };

  const files = st?.files ?? [];
  return (
    <section className="ws-insp-section ws-wt">
      <div className="ws-insp-head">
        <h3>Worktree</h3>
        <Pill className="mono"><Icon name="branch" size={13} /> {wt.branch}</Pill>
      </div>
      <span className="faint mono ws-wt-path" title={wt.path}>{wt.path}</span>
      {st && files.length === 0 && <span className="faint text-sm">Nessuna modifica da applicare.</span>}
      {files.length > 0 && (
        <ul className="ws-changed" aria-label="File modificati">
          {files.map((f) => (
            <li key={f.path}>
              <span className={`ws-fs ws-fs-${f.status}`} title={FILE_STATUS[f.status] ?? f.status}>{f.status}</span>
              <span className="sr-only">{FILE_STATUS[f.status] ?? f.status}: </span>
              <span className="mono">{f.path}</span>
            </li>
          ))}
        </ul>
      )}
      <div className="row">
        <Button size="sm" disabled={files.length === 0} onClick={() => setShowDiff((v) => !v)} aria-expanded={showDiff}>
          {showDiff ? "Nascondi differenze" : "Differenze"}
        </Button>
        <Button size="sm" variant="primary" icon="check" disabled={busy || running || files.length === 0} onClick={() => apply.mutate(session.id, { onSuccess: () => setShowDiff(false) })}>
          Applica al workspace
        </Button>
        <Button size="sm" variant="danger" disabled={busy || running} onClick={onDiscard} onBlur={() => setConfirmDiscard(false)}>
          {confirmDiscard ? "Conferma: elimina" : "Scarta"}
        </Button>
      </div>
      {running && <span className="faint text-xs">Applica e scarta tornano disponibili alla fine del run.</span>}
      {showDiff && st && <Diff text={st.diff} truncated={st.truncated} />}
    </section>
  );
}

function Diff({ text, truncated }: { text: string; truncated: boolean }) {
  const lines = text.split("\n");
  const cls = (l: string) =>
    l.startsWith("+++") || l.startsWith("---") || l.startsWith("diff ") || l.startsWith("new file") || l.startsWith("index ")
      ? "meta"
      : l.startsWith("+") ? "add" : l.startsWith("-") ? "del" : l.startsWith("@@") ? "hunk" : undefined;
  return (
    <pre className="ws-diff" aria-label="Differenze" tabIndex={0}>
      <code>
        {lines.map((l, i) => <span key={i} className={cls(l)}>{l || " "}</span>)}
        {truncated && <span className="meta">… differenze troncate</span>}
      </code>
    </pre>
  );
}
