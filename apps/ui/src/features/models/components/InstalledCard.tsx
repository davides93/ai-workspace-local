import { useMutation } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Button, Chip, CopyField, Dialog, Field, Icon, Input } from "../../../shared/ui";
import { useApi } from "../../../shared/api/context";
import { errorMessage } from "../../../shared/lib/errors";
import { formatBytes, formatContext, formatParams } from "../../../shared/lib/format";
import type { LocalModel } from "../../../shared/api/types";
import { originInfo } from "../model/origin";
import { CapabilityChips, ChipRow, FitPill, OriginChip, RuntimeChips, Stat, Stats } from "./common";

/** One installed model as an instrument module: identity, readouts, fit, runtimes and the three actions. */
export function InstalledCard({ model: m, onUse, onDelete }: { model: LocalModel; onUse: () => void; onDelete: () => void }) {
  const [preview, setPreview] = useState(false);
  const [details, setDetails] = useState(false);
  const detailsId = useId();
  const origin = originInfo(m.origin);
  return (
    <article className="mh-card" data-fit={m.fit || undefined} aria-label={m.name}>
      <header className="mh-card-head">
        <div className="mh-title">
          <h3 className="ellipsis" title={m.name}>{m.name}</h3>
          <span className="mono faint text-xs ellipsis" title={m.id}>{m.id}</span>
        </div>
        <OriginChip origin={m.origin} />
      </header>

      <ChipRow label="Formato">
        <Chip>.{m.format}</Chip>
        {m.quant && <Chip>{m.quant}</Chip>}
      </ChipRow>
      <Stats>
        <Stat label="Dimensione">{formatBytes(m.size_bytes)}</Stat>
        <Stat label="Parametri">{formatParams(m.params_b)}</Stat>
        <Stat label="Contesto">{formatContext(m.context_length)}</Stat>
      </Stats>

      <div className="stack stack-sm">
        <div className="row"><FitPill fit={m.fit} detail={m.fit_detail} /></div>
        {m.fit_detail && <span className="faint text-xs">{m.fit_detail}</span>}
      </div>
      <CapabilityChips items={m.capabilities} />
      <RuntimeChips runtimes={m.runtime_compat} explain />

      <div className="mh-card-foot">
        {m.origin === "system" && <Button variant="primary" size="sm" onClick={() => setPreview(true)}>Prova in chat</Button>}
        {preview && <SystemPreview model={m} onClose={() => setPreview(false)} />}
        <Button variant={m.origin === "system" ? undefined : "primary"} size="sm" icon="link" onClick={onUse}>Usa nel profilo</Button>
        {m.origin !== "system" && <Button size="sm" icon="file" aria-expanded={details} aria-controls={detailsId} onClick={() => setDetails((v) => !v)}>
          {details ? "Nascondi percorso" : "Mostra percorso"}
        </Button>}
        {m.removable ? (
          <Button size="sm" variant="danger" icon="trash" onClick={onDelete} aria-label={`Elimina ${m.name}`}>Elimina</Button>
        ) : (
          <span className="mh-readonly faint text-xs" title={origin.description}>
            <Icon name="lock" size={13} /> Sola lettura · gestito da {origin.owner}
          </span>
        )}
      </div>

      {details && (
        <div id={detailsId} className="stack">
          {m.path ? <CopyField label="Percorso del file" value={m.path} /> : <p className="faint text-sm">Il percorso non è disponibile per questo modello.</p>}
          <CopyField label="ID da usare nei profili e nella CLI" value={m.id} />
          {m.source && <CopyField label="Provenienza" value={m.source} />}
        </div>
      )}
    </article>
  );
}

export function SystemPreview({ model, onClose }: { model: LocalModel; onClose: () => void }) {
  const [prompt, setPrompt] = useState("");
  const api = useApi();
  const run = useMutation({
    mutationFn: (input: string) => api.previewSystemModel(model.id, input),
    meta: { handled: true },
  });
  return (
    <Dialog title={`Prova ${model.name}`} onClose={onClose} footer={<Button onClick={onClose}>Chiudi</Button>}>
      <form className="stack" onSubmit={(event) => {
        event.preventDefault();
        if (prompt.trim() && !run.isPending) run.mutate(prompt.trim());
      }}>
        <Field label="Messaggio" htmlFor="system-preview-prompt">
          <Input id="system-preview-prompt" value={prompt} onChange={(event) => setPrompt(event.target.value)}
            maxLength={2000} data-autofocus />
        </Field>
        <Button type="submit" variant="primary" disabled={!prompt.trim() || run.isPending}>
          {run.isPending ? "Attendo la risposta…" : "Invia"}
        </Button>
        {run.isError && <p role="alert" className="crit-text">{errorMessage(run.error)}</p>}
        {run.data && <p className="prewrap" aria-live="polite">{run.data}</p>}
      </form>
    </Dialog>
  );
}
