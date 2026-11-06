import { useMemo, useState, type FormEvent } from "react";
import { Link } from "../../../app/router";
import { Button, Chip, Chips, Dialog, ErrorState, Field, Input, Select, SkeletonLines } from "../../../shared/ui";
import type { InferenceProfile, LocalModel } from "../../../shared/api/types";
import { useProfiles, useSaveProfile } from "../../agents/api";
import { applyModelToProfile, newProfileFor, pickRuntime, runtimeOptions } from "../model/profile";

const NEW = "__new__";
const FORM_ID = "use-in-profile-form";

/** Pick the inference profile that gets this model and the runtime that will run it. */
export function UseInProfileDialog({ model, onClose }: { model: LocalModel; onClose: () => void }) {
  const profiles = useProfiles();
  if (!profiles.data) {
    return (
      <Dialog title="Usa nel profilo" size="md" onClose={onClose} footer={<Button onClick={onClose}>Chiudi</Button>}>
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
      title="Usa nel profilo" size="md" onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annulla</Button>
          <Button type="submit" form={FORM_ID} variant="primary" icon="check" disabled={!canSave}>{save.isPending ? "Salvo…" : "Usa nel profilo"}</Button>
        </>
      }
    >
      <form id={FORM_ID} className="form-grid" onSubmit={submit} noValidate>
        <div className="stack stack-sm">
          <b>{model.name}</b>
          <Chips><Chip>.{model.format}</Chip>{model.quant && <Chip>{model.quant}</Chip>}<Chip>{model.id}</Chip></Chips>
        </div>

        <Field label="Profilo di inferenza" htmlFor="uip-profile" hint="Gli agenti che usano questo profilo useranno il modello scelto.">
          <Select id="uip-profile" value={target} data-autofocus onChange={(e) => { setTarget(e.target.value); setRuntimeChoice(null); }}>
            {profiles.map((p) => <option key={p.id} value={p.id}>{p.name}{p.model === model.id ? " (già in uso)" : ""}</option>)}
            <option value={NEW}>Nuovo profilo…</option>
          </Select>
        </Field>

        {target === NEW && (
          <Field label="Nome del nuovo profilo" htmlFor="uip-name" error={nameMissing ? "Dai un nome al profilo." : undefined}>
            <Input id="uip-name" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} aria-invalid={nameMissing ? true : undefined} />
          </Field>
        )}

        <Field label="Runtime" htmlFor="uip-runtime" hint="Solo i runtime pronti su questo device possono essere scelti.">
          <Select id="uip-runtime" value={runtime ?? ""} disabled={noRuntime} onChange={(e) => setRuntimeChoice(e.target.value)}>
            {runtime === null && <option value="">Nessun runtime disponibile</option>}
            {options.map((r) => (
              <option key={r.id} value={r.id} disabled={!r.available}>
                {r.name}{r.available ? "" : ` · non disponibile${r.reason ? `: ${r.reason}` : ""}`}
              </option>
            ))}
          </Select>
        </Field>

        {noRuntime && (
          <p className="mh-alert warn" role="alert">
            Nessun runtime in grado di aprire il formato .{model.format} è pronto su questo device. <Link to="/runtimes">Apri i runtime</Link>.
          </p>
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
