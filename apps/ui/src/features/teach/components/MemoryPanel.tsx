import { useEffect, useRef, useState } from "react";
import { useApi } from "../../../shared/api/context";
import { downloadFile } from "../../../shared/lib/download";
import { errorMessage } from "../../../shared/lib/errors";
import { toast } from "../../../shared/lib/toast";
import { Button, DecisionDialog, EmptyState, ErrorState, Field, Icon, Input, Pill, SkeletonLines } from "../../../shared/ui";
import type { Memory } from "../../../shared/api/types";
import { useApproveMemory, useDeleteAllMemories, useForgetMemory, useMemories, useRejectMemory, useRestoreMemory, useUpdateMemory } from "../api";
import { exportFileName, exportJson, splitMemories } from "../model/memories";
import { MemoryItem } from "./MemoryItem";

import { t as tr } from "../../../shared/i18n";
/** How long "Dimenticato… Annulla" stays on screen. */
const UNDO_MS = 8000;

/**
 * "Cosa so": what the assistant remembers, by kind. Search, edit in place, "Ricorda sempre", forget with undo,
 * the proposals waiting for a yes or no, and the export / delete-all of everything.
 */
export function MemoryPanel() {
  const api = useApi();
  const [search, setSearch] = useState("");
  const [q, setQ] = useState("");
  useEffect(() => {
    const t = setTimeout(() => setQ(search.trim()), 250);
    return () => clearTimeout(t);
  }, [search]);
  const memories = useMemories(q ? { q } : {});
  const update = useUpdateMemory();
  const forget = useForgetMemory();
  const restore = useRestoreMemory();
  const approve = useApproveMemory();
  const reject = useRejectMemory();
  const deleteAll = useDeleteAllMemories();
  const [undo, setUndo] = useState<Memory | null>(null);
  const undoTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [confirmAll, setConfirmAll] = useState(false);
  const [exporting, setExporting] = useState(false);
  useEffect(() => () => clearTimeout(undoTimer.current), []);

  const { proposed, groups, known } = splitMemories(memories.data);
  const searching = q !== "";

  const onForget = (m: Memory) => {
    forget.mutate(m.id);
    clearTimeout(undoTimer.current);
    setUndo(m);
    undoTimer.current = setTimeout(() => setUndo(null), UNDO_MS);
  };
  const onUndo = () => {
    if (!undo) return;
    restore.mutate(undo);
    clearTimeout(undoTimer.current);
    setUndo(null);
  };
  // Export what the assistant knows right now, whatever the search box says.
  const onExport = async () => {
    setExporting(true);
    try {
      const all = await api.memories();
      downloadFile(exportFileName(), exportJson(all));
    } catch (e) {
      toast(tr("teach.non_sono_riuscito_a_esportare", { p: errorMessage(e) }), "crit");
    } finally {
      setExporting(false);
    }
  };
  const busyId = (id: string) => (update.isPending && update.variables?.id === id) || (approve.isPending && approve.variables?.id === id) || (reject.isPending && reject.variables === id);

  return (
    <div className="th-mems">
      <header className="th-mems-head">
        <h2>{tr("teach.cosa_so")}</h2>
        {memories.data && <Pill tone={known ? "info" : "neutral"}>{known}</Pill>}
        <span className="grow" />
        <Button size="sm" variant="ghost" icon="download" disabled={exporting || !memories.data?.length} onClick={() => void onExport()}>{tr("teach.esporta")}</Button>
        <Button size="sm" variant="ghost" icon="trash" disabled={!memories.data?.length} onClick={() => setConfirmAll(true)}>{tr("teach.cancella_tutto")}</Button>
      </header>
      <p className="faint text-sm">{tr("teach.resta_tutto_su_questo_dispositivo")}</p>

      <Field label={tr("teach.cerca_tra_i_ricordi")} htmlFor="th-search" className="th-search">
        <Input id="th-search" type="search" placeholder={tr("teach.per_esempio_caffe")} value={search} onChange={(e) => setSearch(e.target.value)} />
      </Field>

      {memories.isPending && <SkeletonLines lines={4} />}
      {memories.isError && !memories.data && <ErrorState title={tr("teach.non_riesco_a_leggere_i")} error={memories.error} onRetry={() => void memories.refetch()} />}

      {memories.data && proposed.length > 0 && (
        <section className="th-group proposals" aria-label={tr("teach.da_confermare")}>
          <h3><Icon name="alert" size={14} /> {tr("teach.da_confermare")} <Pill tone="warn">{proposed.length}</Pill></h3>
          <ul>
            {proposed.map((m) => (
              <MemoryItem key={m.id} memory={m} busy={busyId(m.id)} onSave={(patch) => update.mutate({ id: m.id, patch })} onForget={() => onForget(m)}
                onApprove={() => approve.mutate(m)} onReject={() => reject.mutate(m.id)} />
            ))}
          </ul>
        </section>
      )}

      {memories.data && groups.map((g) => (
        <section key={g.kind} className="th-group" aria-label={g.label}>
          <h3>{g.label} <span className="faint text-xs">{g.items.length}</span></h3>
          <ul>
            {g.items.map((m) => (
              <MemoryItem key={m.id} memory={m} busy={busyId(m.id)} onSave={(patch) => update.mutate({ id: m.id, patch })} onForget={() => onForget(m)}
                onApprove={() => approve.mutate(m)} onReject={() => reject.mutate(m.id)} />
            ))}
          </ul>
        </section>
      ))}

      {memories.data && known === 0 && proposed.length === 0 && (
        searching
          ? <EmptyState icon="search" title={tr("teach.nessun_ricordo_corrisponde")} description={tr("teach.prova_con_un_altra_parola")} action={<Button onClick={() => setSearch("")}>{tr("teach.cancella_la_ricerca")}</Button>} />
          : <EmptyState icon="bulb" title={tr("teach.non_so_ancora_nulla")} description={tr("teach.racconta_qualcosa_nella_chat_quello")} />
      )}

      {undo && (
        <div className="th-undo" role="status">
          <span>{tr("teach.dimenticato", { p: undo.text.length > 48 ? `${undo.text.slice(0, 48)}…` : undo.text })}</span>
          <Button size="sm" onClick={onUndo}>{tr("teach.annulla")}</Button>
        </div>
      )}

      {confirmAll && (
        <DecisionDialog
          title={tr("teach.cancellare_tutti_i_ricordi")} confirmLabel={tr("teach.cancella_tutto")} icon="trash" pending={deleteAll.isPending} error={deleteAll.error}
          onCancel={() => { setConfirmAll(false); deleteAll.reset(); }}
          onConfirm={() => deleteAll.mutate(undefined, { onSuccess: () => { setConfirmAll(false); setUndo(null); } })}
        >
          <p>{tr("teach.l_assistente_dimentichera_tutto_quello", { p: known ? ` (${known} ${known === 1 ? "ricordo" : "ricordi"})` : "" })}</p>
        </DecisionDialog>
      )}
    </div>
  );
}
