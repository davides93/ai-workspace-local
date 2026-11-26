import { useId, useState } from "react";
import { useSystem } from "../../shared/query/system";
import { Button, Field, Input, Sheet, Textarea } from "../../shared/ui";
import { TasksPanel } from "../inbox/components/TasksPanel";
import { useCreateCollaborator } from "./api";
import { problem, remoteComputer, toTask } from "./model";

import { t } from "../../shared/i18n";
/**
 * «I tuoi collaboratori» (simple mode): say what a collaborator should do and it starts working on its own; below, the ones
 * at work, each with pause, a word to say to it, and the confirmations it waits for. It reuses the background jobs of the
 * assistant, so budget, quiet hours and «ask before acting» apply to it like to everything else.
 */
export function CollaboratorsSheet({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [job, setJob] = useState("");
  const [shown, setShown] = useState(false);
  const create = useCreateCollaborator(() => { setName(""); setJob(""); setShown(false); });
  const id = useId();
  const { data: system } = useSystem();
  // This screen talks to another computer's AIW (the phone paired with the desktop): the work runs there, not on the phone.
  const remote = remoteComputer(system);
  const bad = shown ? problem({ name, job }) : undefined;
  const submit = () => {
    if (problem({ name, job })) { setShown(true); return; }
    create.mutate(toTask({ name, job }));
  };
  return (
    <Sheet title={t("collab.title")} onClose={onClose}>
      <div className="stack stack-lg">
        <p className="muted">{remote ? t("collab.intro_remote", { device: remote }) : t("collab.intro")}</p>
        <form className="stack" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <Field label={t("collab.name")} htmlFor={`${id}-name`}>
            <Input id={`${id}-name`} value={name} maxLength={60} placeholder={t("collab.name_placeholder")} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field
            label={t("collab.job")}
            htmlFor={`${id}-job`}
            hint={t("collab.job_hint")}
            error={bad === "no_job" ? t("collab.err_no_job") : bad === "too_long" ? t("collab.err_too_long") : undefined}
          >
            <Textarea id={`${id}-job`} rows={4} value={job} placeholder={t("collab.job_placeholder")} aria-invalid={bad ? true : undefined} onChange={(e) => setJob(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" icon="sparkle" disabled={create.isPending}>{t("collab.start")}</Button>
        </form>
        <TasksPanel />
      </div>
    </Sheet>
  );
}
