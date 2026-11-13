import { Button, Dialog, KeyValue } from "../../../shared/ui";
import { formatBytes } from "../../../shared/lib/format";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import { useProfiles } from "../../agents/api";
import { useDeleteModel } from "../api";
import { originInfo } from "../model/origin";
import { profilesUsingModel } from "../model/profile";

import { t, tNode, t as tr } from "../../../shared/i18n";
/** Confirmation that spells out what is deleted: the files, the space freed and the profiles that will break. */
export function DeleteModelDialog({ model, onClose }: { model: LocalModel; onClose: () => void }) {
  const remove = useDeleteModel();
  const profiles = useProfiles();
  const using: InferenceProfile[] = profilesUsingModel(profiles.data ?? [], model.id);
  return (
    <Dialog
      title={t("models.elimina_il_modello")} size="sm" onClose={onClose}
      footer={
        <>
          <Button data-autofocus onClick={onClose}>{t("models.annulla")}</Button>
          <Button variant="danger" icon="trash" disabled={remove.isPending} onClick={() => remove.mutate(model.id, { onSuccess: onClose })}>
            {remove.isPending ? t("models.elimino") : t("models.elimina_dal_disco")}
          </Button>
        </>
      }
    >
      <p>{tNode("models.stai_per_eliminare_dal_disco", { name: <b>{model.name}</b> })}</p>
      <KeyValue
        items={[
          ["ID", <span className="mono" key="id">{model.id}</span>],
          [tr("models.origine"), originInfo(model.origin).label],
          [tr("models.spazio_liberato"), formatBytes(model.size_bytes)],
          !!model.path && [tr("models.file"), <span className="mono mh-path" key="p">{model.path}</span>],
        ]}
      />
      <p className="faint text-sm">
        {t("models.vengono_tolti_solo_i_file")}</p>
      {using.length > 0 && (
        <p className="mh-alert warn" role="alert">
          {tNode(using.length === 1 ? "models.delete_in_use_one" : "models.delete_in_use_other", { names: <b>{using.map((p) => p.name).join(", ")}</b> })}
        </p>
      )}
    </Dialog>
  );
}
