import { useState } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import type { ChannelKind } from "../../../shared/api/types";
import { Button, Segmented } from "../../../shared/ui";
import { useCreateChannel } from "../api";
import { CHANNEL_KINDS, kindInfo } from "../model/channels";

/** Pick what to connect, read the steps, create the channel (off, nothing connects yet). */
export function AddChannel({ onDone }: { onDone: () => void }) {
  const [kind, setKind] = useState<ChannelKind>("telegram");
  const create = useCreateChannel();
  const info = kindInfo(kind);
  return (
    <div className="panel chn-add">
      <h2>Aggiungi un canale</h2>
      <Segmented<ChannelKind> label="Che cosa vuoi collegare" value={kind} onChange={setKind} options={CHANNEL_KINDS.map((k) => ({ id: k.id, label: k.label }))} />
      <p className="muted text-sm">{info.blurb}</p>
      <ol className="chn-list">{info.steps.map((s) => <li key={s}>{s}</li>)}</ol>
      {create.error && <p className="chn-error" role="alert">{errorMessage(create.error)}</p>}
      <div className="row">
        <Button variant="primary" disabled={create.isPending} onClick={() => create.mutate({ kind, name: info.label }, { onSuccess: onDone })}>Crea il canale {info.label}</Button>
        <Button variant="ghost" onClick={onDone}>Annulla</Button>
      </div>
      <p className="faint text-xs">Il canale nasce spento: non si collega a nulla finché non lo accendi.</p>
    </div>
  );
}
