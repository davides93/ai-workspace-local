import { MAX_TASK_INSTRUCTION_CHARS, type TaskCreate } from "../../shared/api/types";

export type CollaboratorProblem = "no_job" | "too_long";

/** What the person typed for a new collaborator: a name (optional) and what it should do. */
export interface CollaboratorDraft {
  name: string;
  job: string;
}

/** The first thing wrong with the draft, or nothing. Counts characters the way the engine does (code points). */
export function problem(d: CollaboratorDraft): CollaboratorProblem | undefined {
  const job = d.job.trim();
  if (!job) return "no_job";
  if ([...job].length > MAX_TASK_INSTRUCTION_CHARS) return "too_long";
  return undefined;
}

/** The request for the engine. The collaborator is a background job of the default agent: it uses the same budget, quiet hours and confirmations as every other one. */
export function toTask(d: CollaboratorDraft): TaskCreate {
  const name = d.name.trim();
  return { instruction: d.job.trim(), ...(name ? { title: name.slice(0, 60) } : {}) };
}
