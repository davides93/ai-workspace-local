import { useMemo, useState, type FormEvent } from "react";
import { Link } from "../../../app/router";
import { Button, Chip, Chips, Dialog, ErrorState, Field, Input, Select, SkeletonLines } from "../../../shared/ui";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import { useProfiles, useSaveProfile } from "../../agents/api";
import { applyModelToProfile, newProfileFor, pickRuntime, runtimeOptions } from "../model/profile";

import { t, tNode } from "../../../shared/i18n";
const NEW = "__new__";
const FORM_ID = "use-in-profile-form";

/** Pick the inference profile that gets this model and the runtime that will run it. */
export function UseInProfileDialog({ model, onClose }: { model: LocalModel; onClose: () => void }) {
  const profiles = useProfiles();
  if (!profiles.data) {
    return (
      <Dialog title={t("models.usa_nel_profilo")} size="md" onClose={onClose} footer={<Button onClick={onClose}>{t("models.chiudi")}</Button>}>
        {profiles.isError ? <ErrorState error={profiles.error} onRetry={() => void profiles.refetch()} /> : <SkeletonLines lines={4} />}
      </Dialog>
    );
  }
  return <ProfileDialog model={model} profiles={profiles.data} onClose={onClose} />;
}

function ProfileDialog({ model, profiles, onClose }: { model: LocalModel; profiles: InferenceProfile[]; onClose: () => void }) {
  const save = useSaveProfile();
  const options = useMemo(() => runtimeOptions(model), [model]);
  const [target, setTarget] = useState(NEW);
  const profile = profiles.find((p) => p.id === target);
  const [runtimeChoice, setRuntimeChoice] = useState<string | null>(null);
  const [name, setName] = useState(model.name);

  // The runtime follows the chosen profile until the user picks one by hand.
  const runtime = runtimeChoice ?? pickRuntime(model, profile?.runtime_id);
  const noRuntime = pickRuntime(model) === null;
  const nameMissing = target === NEW && !name.trim();
  const canSave = !!runtime && !nameMissing && !save.isPending;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!canSave || !runtime) return;
    const next = profile ? applyModelToProfile(profile, model, runtime) : newProfileFor(model, runtime, name, profiles.map((p) => p.id));
    save.mutate(next, { onSuccess: onClose });
  };

  return (
    <Dialog
      title={t("models.usa_nel_profilo")} size="md" onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("models.annulla")}</Button>
          <Button type="submit" form={FORM_ID} variant="primary" icon="check" disabled={!canSave}>{save.isPending ? t("models.salvo") : t("models.usa_nel_profilo")}</Button>
        </>
      }
    >
      <form id={FORM_ID} className="form-grid" onSubmit={submit} noValidate>
        <div className="stack stack-sm">
          <b>{model.name}</b>
          <Chips><Chip>.{model.format}</Chip>{model.quant && <Chip>{model.quant}</Chip>}<Chip>{model.id}</Chip></Chips>
        </div>

        <Field label={t("models.profilo_di_inferenza")} htmlFor="uip-profile" hint={t("models.gli_agenti_che_usano_questo")}>
          <Select id="uip-profile" value={target} data-autofocus onChange={(e) => { setTarget(e.target.value); setRuntimeChoice(null); }}>
            {profiles.map((p) => <option key={p.id} value={p.id}>{p.name}{p.model === model.id ? t("models.gia_in_uso") : ""}</option>)}
            <option value={NEW}>{t("models.nuovo_profilo")}</option>
          </Select>
        </Field>

        {target === NEW && (
          <Field label={t("models.nome_del_nuovo_profilo")} htmlFor="uip-name" error={nameMissing ? "Dai un nome al profilo." : undefined}>
            <Input id="uip-name" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} aria-invalid={nameMissing ? true : undefined} />
          </Field>
        )}

        <Field label={t("models.runtime")} htmlFor="uip-runtime" hint={t("models.solo_i_runtime_pronti_su")}>
          <Select id="uip-runtime" value={runtime ?? ""} disabled={noRuntime} onChange={(e) => setRuntimeChoice(e.target.value)}>
            {runtime === null && <option value="">{t("models.nessun_runtime_disponibile")}</option>}
            {options.map((r) => (
              <option key={r.id} value={r.id} disabled={!r.available}>
                {r.name}{r.available ? "" : t("models.non_disponibile", { p: r.reason ? `: ${r.reason}` : "" })}
              </option>
            ))}
          </Select>
        </Field>

        {noRuntime && (
          <p className="mh-alert warn" role="alert">
            {tNode("models.nessun_runtime_in_grado_di", { link: <Link to="/runtimes">{t("models.apri_i_runtime")}</Link> }, { format: model.format })}</p>
        )}
        {profile && !noRuntime && (
          <p className="faint text-sm">
            «{profile.name}» passerà da <span className="mono">{profile.model}</span> ({profile.runtime_id}) a <span className="mono">{model.id}</span>
            {runtime ? ` (${runtime})` : ""}. I parametri di campionamento restano quelli del profilo.
          </p>
        )}
      </form>
    </Dialog>
  );
}
