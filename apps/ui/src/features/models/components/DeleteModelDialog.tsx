import { Button, Dialog, KeyValue } from "../../../shared/ui";
import { formatBytes } from "../../../shared/lib/format";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import { useProfiles } from "../../agents/api";
import { useDeleteModel } from "../api";
import { originInfo } from "../model/origin";
import { profilesUsingModel } from "../model/profile";

/** Confirmation that spells out what is deleted: the files, the space freed and the profiles that will break. */
export function DeleteModelDialog({ model, onClose }: { model: LocalModel; onClose: () => void }) {
  const remove = useDeleteModel();
  const profiles = useProfiles();
  const using: InferenceProfile[] = profilesUsingModel(profiles.data ?? [], model.id);
  return (
    <Dialog
      title="Elimina il modello" size="sm" onClose={onClose}
      footer={
        <>
          <Button data-autofocus onClick={onClose}>Annulla</Button>
          <Button variant="danger" icon="trash" disabled={remove.isPending} onClick={() => remove.mutate(model.id, { onSuccess: onClose })}>
            {remove.isPending ? "Elimino…" : "Elimina dal disco"}
          </Button>
        </>
      }
    >
      <p>Stai per eliminare <b>{model.name}</b> dal disco. L'azione non si può annullare, ma potrai riscaricarlo.</p>
      <KeyValue
        items={[
          ["ID", <span className="mono" key="id">{model.id}</span>],
          ["Origine", originInfo(model.origin).label],
          ["Spazio liberato", formatBytes(model.size_bytes)],
          !!model.path && ["File", <span className="mono mh-path" key="p">{model.path}</span>],
        ]}
      />
      <p className="faint text-sm">
        Vengono tolti solo i file di questo modello. I file condivisi con un'altra variante (per esempio il proiettore mmproj di un modello
        vision) restano finché li usa un'altra variante.
      </p>
      {using.length > 0 && (
        <p className="mh-alert warn" role="alert">
          {using.length === 1 ? "Il profilo" : "I profili"} <b>{using.map((p) => p.name).join(", ")}</b>{" "}
          {using.length === 1 ? "usa" : "usano"} questo modello e smetter{using.length === 1 ? "à" : "anno"} di funzionare finché non ne scegli un altro.
        </p>
      )}
    </Dialog>
  );
}
