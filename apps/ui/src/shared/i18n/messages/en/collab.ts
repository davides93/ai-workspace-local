// English messages of "collab". One entry per line; must have exactly the keys of the Italian file.
import type { collabIt } from "../it/collab";

export const collabEn: Record<keyof typeof collabIt, string> = {
  "collab.open": "Your collaborators",
  "collab.title": "Your collaborators",
  "collab.intro": "A collaborator works for you even when you are away. Say what it should do: if it needs your yes before acting, it asks you here.",
  "collab.name": "Name (optional)",
  "collab.name_placeholder": "For example: Morning mail",
  "collab.job": "What it should do",
  "collab.job_hint": "Write it as you would say it to a person.",
  "collab.job_placeholder": "Every morning look at my mail and draft a reply to the important messages.",
  "collab.err_no_job": "Write what the collaborator should do.",
  "collab.err_too_long": "That is too long: shorten it.",
  "collab.start": "Start the collaborator",
  "collab.started": "The collaborator has started.",
  "collab.start_failed": "I could not start it: {error}",
};
